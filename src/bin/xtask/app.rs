use std::env;
use std::path::PathBuf;

use anyhow::{Context, bail};

use super::cargo_ops::{cargo_test, create_package, run_shell_e2e};
use super::host::build_distribution_binary;
use super::signing;
use super::types::{Profile, absolutize, repo_root};

pub(super) fn run() -> anyhow::Result<()> {
    let root = repo_root()?;
    signing::load_repo_env(&root)?;
    let task = env::args().nth(1).unwrap_or_else(|| "help".into());
    match task.as_str() {
        "dev" => DevArgs::parse(env::args().skip(2))?.run(),
        "package" => PackageArgs::parse(env::args().skip(2))?.run(),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => bail!("unknown xtask subcommand: {other}"),
    }
}

fn print_help() {
    println!(
        "usage: cargo run --bin xtask -- <dev|package> [options]\n\n\
         dev:     build a local sagens binary with embedded standalone assets\n\
         package: build a standalone release binary with embedded assets"
    );
}

struct DevArgs {
    profile: Profile,
    run_tests: bool,
    run_e2e: bool,
    refresh_guest: bool,
    python_package_root: Option<PathBuf>,
}

impl DevArgs {
    fn parse(args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let mut profile = Profile::Debug;
        let mut run_tests = false;
        let mut run_e2e = false;
        let mut refresh_guest = false;
        let mut python_package_root = None;
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--release" => profile = Profile::Release,
                "--test" => run_tests = true,
                "--e2e" => run_e2e = true,
                "--refresh-guest" => refresh_guest = true,
                "--python-package-root" => {
                    python_package_root = Some(PathBuf::from(
                        args.next()
                            .context("missing value for --python-package-root")?,
                    ));
                }
                "-h" | "--help" => {
                    println!(
                        "usage: cargo run --bin xtask -- dev [--release] [--test] [--e2e] [--refresh-guest] [--python-package-root DIR]"
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
            python_package_root,
        })
    }

    fn run(self) -> anyhow::Result<()> {
        let root = repo_root()?;
        let built = build_distribution_binary(&root, self.profile, self.refresh_guest)?;
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
    python_package_root: Option<PathBuf>,
}

impl PackageArgs {
    fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let mut profile = Profile::Release;
        let mut version = String::from("dev");
        let mut out_dir = PathBuf::from("dist");
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
                "--python-package-root" => {
                    python_package_root = Some(PathBuf::from(
                        args.next()
                            .context("missing value for --python-package-root")?,
                    ));
                }
                "-h" | "--help" => {
                    println!(
                        "usage: cargo run --bin xtask -- package [--profile debug|release] [--version VERSION] [--out-dir DIR] [--python-package-root DIR]"
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
            python_package_root,
        })
    }

    fn run(self) -> anyhow::Result<()> {
        let root = repo_root()?;
        let built = build_distribution_binary(&root, self.profile, true)?;
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
