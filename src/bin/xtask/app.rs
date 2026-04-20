use std::env;
use std::path::PathBuf;

use anyhow::{Context, bail};

use super::cargo_ops::{cargo_test, create_package, run_shell_e2e};
use super::host::build_distribution_binary;
use super::runtime::{clean_runtime_dir, ensure_runtime_bundle, maybe_init_submodules};
use super::signing;
use super::types::{Platform, Profile, absolutize, repo_root};

pub(super) fn run() -> anyhow::Result<()> {
    let root = repo_root()?;
    signing::load_repo_env(&root)?;
    let task = env::args().nth(1).unwrap_or_else(|| "help".into());
    match task.as_str() {
        "dev" => DevArgs::parse(env::args().skip(2))?.run(),
        "package" => PackageArgs::parse(env::args().skip(2))?.run(),
        "build-runtime" => RuntimeArgs::parse(env::args().skip(2))?.run(),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => bail!("unknown xtask subcommand: {other}"),
    }
}

fn print_help() {
    println!(
        "usage: cargo run --bin xtask -- <dev|package|build-runtime> [options]\n\n\
         dev:     build a local sagens binary with embedded standalone assets\n\
         package: build a standalone release binary with embedded assets\n\
         build-runtime: rebuild third_party/runtime/<platform> from third_party/upstream/libkrun"
    );
}

struct DevArgs {
    profile: Profile,
    run_tests: bool,
    run_e2e: bool,
    refresh_guest: bool,
    refresh_runtime: bool,
    python_package_root: Option<PathBuf>,
}

impl DevArgs {
    fn parse(args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let mut profile = Profile::Debug;
        let mut run_tests = false;
        let mut run_e2e = false;
        let mut refresh_guest = false;
        let mut refresh_runtime = false;
        let mut python_package_root = None;
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--release" => profile = Profile::Release,
                "--test" => run_tests = true,
                "--e2e" => run_e2e = true,
                "--refresh-guest" => refresh_guest = true,
                "--refresh-runtime" => refresh_runtime = true,
                "--python-package-root" => {
                    python_package_root = Some(PathBuf::from(
                        args.next()
                            .context("missing value for --python-package-root")?,
                    ));
                }
                "-h" | "--help" => {
                    println!(
                        "usage: cargo run --bin xtask -- dev [--release] [--test] [--e2e] [--refresh-guest] [--refresh-runtime] [--python-package-root DIR]"
                    );
                    std::process::exit(0);
                }
                other => bail!("unknown dev flag: {other}"),
            }
        }
        Ok(Self {
            profile,
            run_tests,
            run_e2e,
            refresh_guest,
            refresh_runtime,
            python_package_root,
        })
    }

    fn run(self) -> anyhow::Result<()> {
        let root = repo_root()?;
        let built = build_distribution_binary(
            &root,
            self.profile,
            self.refresh_runtime,
            self.refresh_guest,
        )?;
        stage_python_binary_if_requested(&root, self.python_package_root.as_deref(), &built.path)?;
        if self.run_tests {
            cargo_test(&root, self.profile, None, &[])?;
        }
        if self.run_e2e {
            run_shell_e2e(&root, "scripts/e2e-standalone.sh", &built.path)?;
            run_shell_e2e(&root, "scripts/e2e-checkpoint.sh", &built.path)?;
        }
        println!("built {}", built.path.display());
        Ok(())
    }
}

struct PackageArgs {
    profile: Profile,
    version: String,
    out_dir: PathBuf,
    refresh_runtime: bool,
    python_package_root: Option<PathBuf>,
}

impl PackageArgs {
    fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let mut profile = Profile::Release;
        let mut version = String::from("dev");
        let mut out_dir = PathBuf::from("dist");
        let mut refresh_runtime = false;
        let mut python_package_root = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--profile" => {
                    profile = Profile::parse(&args.next().context("missing value for --profile")?)?
                }
                "--version" => version = args.next().context("missing value for --version")?,
                "--out-dir" => {
                    out_dir = PathBuf::from(args.next().context("missing value for --out-dir")?)
                }
                "--refresh-runtime" => refresh_runtime = true,
                "--python-package-root" => {
                    python_package_root = Some(PathBuf::from(
                        args.next()
                            .context("missing value for --python-package-root")?,
                    ));
                }
                "-h" | "--help" => {
                    println!(
                        "usage: cargo run --bin xtask -- package [--profile debug|release] [--version VERSION] [--out-dir DIR] [--refresh-runtime] [--python-package-root DIR]"
                    );
                    std::process::exit(0);
                }
                other => bail!("unknown package flag: {other}"),
            }
        }
        Ok(Self {
            profile,
            version,
            out_dir,
            refresh_runtime,
            python_package_root,
        })
    }

    fn run(self) -> anyhow::Result<()> {
        let root = repo_root()?;
        let built = build_distribution_binary(&root, self.profile, self.refresh_runtime, true)?;
        stage_python_binary_if_requested(&root, self.python_package_root.as_deref(), &built.path)?;
        let out_dir = absolutize(&root, &self.out_dir);
        std::fs::create_dir_all(&out_dir)
            .with_context(|| format!("creating {}", out_dir.display()))?;
        create_package(
            &out_dir,
            &built.artifact_platform,
            &self.version,
            &built.path,
        )
    }
}

fn stage_python_binary_if_requested(
    root: &std::path::Path,
    python_package_root: Option<&std::path::Path>,
    binary: &std::path::Path,
) -> anyhow::Result<()> {
    if let Some(package_root) = python_package_root {
        let staged = signing::stage_python_binary_payload(&absolutize(root, package_root), binary)?;
        println!("staged {}", staged.display());
    }
    Ok(())
}

struct RuntimeArgs {
    platform: Platform,
    refresh: bool,
}

impl RuntimeArgs {
    fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let mut platform = Platform::current()?;
        let mut refresh = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--platform" => {
                    platform =
                        Platform::parse(&args.next().context("missing value for --platform")?)?
                }
                "--refresh" => refresh = true,
                "-h" | "--help" => {
                    println!(
                        "usage: cargo run --bin xtask -- build-runtime [--platform PLATFORM] [--refresh]"
                    );
                    std::process::exit(0);
                }
                other => bail!("unknown build-runtime flag: {other}"),
            }
        }
        Ok(Self { platform, refresh })
    }

    fn run(self) -> anyhow::Result<()> {
        let root = repo_root()?;
        maybe_init_submodules(&root)?;
        if self.refresh {
            clean_runtime_dir(&root, self.platform)?;
        }
        let runtime = ensure_runtime_bundle(&root, self.platform)?;
        println!(
            "runtime bundle ready at {} ({})",
            root.join("third_party")
                .join("runtime")
                .join(self.platform.as_str())
                .display(),
            runtime.source.label()
        );
        Ok(())
    }
}
