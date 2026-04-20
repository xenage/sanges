use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

pub(super) struct Args {
    pub(super) arch: String,
    pub(super) work_dir: PathBuf,
    pub(super) output_dir: PathBuf,
    pub(super) guest_agent: PathBuf,
    pub(super) min_image_mib: u64,
}

impl Args {
    pub(super) fn parse() -> anyhow::Result<Self> {
        let mut arch = default_guest_arch()?.to_owned();
        let mut work_dir = PathBuf::from(".box-artifacts");
        let mut output_dir: Option<PathBuf> = None;
        let mut guest_agent = None;
        let mut min_image_mib = 256_u64;

        let mut args = env::args_os().skip(1);
        while let Some(flag) = args.next() {
            match flag.to_string_lossy().as_ref() {
                "--arch" => arch = next_string(&mut args, "--arch")?,
                "--work-dir" => work_dir = next_path(&mut args, "--work-dir")?,
                "--output-dir" => output_dir = Some(next_path(&mut args, "--output-dir")?),
                "--guest-agent" => guest_agent = Some(next_path(&mut args, "--guest-agent")?),
                "--min-image-mib" => {
                    let value = next_string(&mut args, "--min-image-mib")?;
                    min_image_mib = value
                        .parse()
                        .with_context(|| format!("invalid --min-image-mib value: {value}"))?;
                }
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => bail!("unknown flag: {other}"),
            }
        }

        let guest_agent = guest_agent.context("--guest-agent is required")?;
        validate_arch(&arch)?;
        let output_dir =
            output_dir.unwrap_or_else(|| PathBuf::from(format!("artifacts/alpine-{arch}")));
        Ok(Self {
            arch,
            work_dir: work_dir.canonicalize().unwrap_or(work_dir),
            output_dir: output_dir.canonicalize().unwrap_or(output_dir),
            guest_agent: guest_agent.canonicalize().unwrap_or(guest_agent),
            min_image_mib,
        })
    }
}

fn print_help() {
    println!(
        "usage: build-alpine-guest [--arch aarch64|x86_64] [--work-dir DIR] [--output-dir DIR] --guest-agent PATH [--min-image-mib N]"
    );
}

fn default_guest_arch() -> anyhow::Result<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") | ("linux", "aarch64") => Ok("aarch64"),
        ("macos", "x86_64") | ("linux", "x86_64") => Ok("x86_64"),
        (os, arch) => bail!("unsupported host platform for guest build defaults: {os}/{arch}"),
    }
}

fn validate_arch(arch: &str) -> anyhow::Result<()> {
    match arch {
        "aarch64" | "x86_64" => Ok(()),
        _ => bail!("unsupported guest arch: {arch}"),
    }
}

pub(super) fn rootfs_tar_name(arch: &str) -> String {
    format!("alpine-minirootfs-{}-{arch}.tar.gz", crate::ALPINE_VERSION)
}

fn next_path(args: &mut impl Iterator<Item = OsString>, flag: &str) -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(next_string(args, flag)?))
}

fn next_string(args: &mut impl Iterator<Item = OsString>, flag: &str) -> anyhow::Result<String> {
    args.next()
        .map(|value| value.to_string_lossy().into_owned())
        .with_context(|| format!("missing value for {flag}"))
}

pub(super) fn absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}
