use std::fs::{self, File};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail, ensure};
use ext4_lwext4::{Ext4Fs, FileBlockDevice, OpenFlags};

use super::archive::unpack_with_tar;
use super::packages::Package;

pub(super) fn rebuild_rootfs(
    rootfs_tar: &Path,
    apk_dir: &Path,
    resolved: &[Package],
    rootfs_dir: &Path,
    guest_agent: &Path,
    configure_pip_cache: bool,
    configure_npm_cache: bool,
) -> anyhow::Result<()> {
    if rootfs_dir.exists() {
        fs::remove_dir_all(rootfs_dir)
            .with_context(|| format!("removing {}", rootfs_dir.display()))?;
    }
    fs::create_dir_all(rootfs_dir)?;
    unpack_with_tar(rootfs_tar, rootfs_dir)?;
    for package in resolved {
        unpack_with_tar(&apk_dir.join(package.file_name()), rootfs_dir)?;
    }
    create_rootfs_mountpoints(rootfs_dir)?;
    install_guest_agent(rootfs_dir, guest_agent)?;
    write_guest_init(rootfs_dir)?;
    write_package_manager_defaults(rootfs_dir, configure_pip_cache, configure_npm_cache)?;
    ensure_real_bash(rootfs_dir)?;
    Ok(())
}

pub(super) fn prepare_cache_dir(
    cache_dir: &Path,
    apk_dir: &Path,
    resolved: &[Package],
) -> anyhow::Result<()> {
    if cache_dir.exists() {
        fs::remove_dir_all(cache_dir)
            .with_context(|| format!("removing {}", cache_dir.display()))?;
    }
    for relative in ["apk", "pip/wheels", "npm"] {
        fs::create_dir_all(cache_dir.join(relative))?;
    }
    for package in resolved {
        let file_name = package.file_name();
        fs::copy(
            apk_dir.join(&file_name),
            cache_dir.join("apk").join(&file_name),
        )
        .with_context(|| format!("copying {file_name} into image cache"))?;
    }
    fs::copy(
        apk_dir.join("manifest.txt"),
        cache_dir.join("apk").join("manifest.txt"),
    )?;
    Ok(())
}

pub(super) fn prepare_pip_cache(
    cache_dir: &Path,
    arch: &str,
    requirements: &[String],
) -> anyhow::Result<()> {
    if requirements.is_empty() {
        return Ok(());
    }
    let platform = match arch {
        "aarch64" => "musllinux_1_2_aarch64",
        "x86_64" => "musllinux_1_2_x86_64",
        _ => bail!("unsupported pip cache arch: {arch}"),
    };
    let mut command = Command::new("python3");
    command
        .arg("-m")
        .arg("pip")
        .arg("download")
        .arg("--dest")
        .arg(cache_dir.join("pip/wheels"))
        .arg("--only-binary=:all:")
        .arg("--platform")
        .arg(platform)
        .arg("--implementation")
        .arg("cp")
        .arg("--python-version")
        .arg("312")
        .arg("--abi")
        .arg("cp312");
    command.args(requirements);
    run_command(command, "downloading pip cache")
}

pub(super) fn prepare_npm_cache(cache_dir: &Path, packages: &[String]) -> anyhow::Result<()> {
    if packages.is_empty() {
        return Ok(());
    }
    for package in packages {
        let mut command = Command::new("npm");
        command
            .arg("cache")
            .arg("add")
            .arg(package)
            .arg("--cache")
            .arg(cache_dir.join("npm"))
            .arg("--prefer-online")
            .arg("--no-audit")
            .arg("--fund=false");
        run_command(command, "downloading npm cache")?;
    }
    Ok(())
}

pub(super) fn extract_guest_agent(rootfs_image: &Path, destination: &Path) -> anyhow::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let device = FileBlockDevice::open(rootfs_image)
        .with_context(|| format!("opening base rootfs {}", rootfs_image.display()))?;
    let fs = Ext4Fs::mount(device, true).context("mounting base rootfs")?;
    let mut input = fs
        .open("/usr/local/bin/sagens-guest-agent", OpenFlags::READ)
        .context("opening guest agent from base rootfs")?;
    let mut output =
        File::create(destination).with_context(|| format!("creating {}", destination.display()))?;
    io::copy(&mut input, &mut output).context("extracting guest agent")?;
    drop(input);
    fs.umount().context("unmounting base rootfs")?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn create_rootfs_mountpoints(rootfs_dir: &Path) -> anyhow::Result<()> {
    for relative in [
        "proc",
        "sys",
        "dev",
        "home",
        "tmp",
        "workspace",
        "usr/local/bin",
        "var/cache/apk",
        "opt/sagens-cache",
    ] {
        fs::create_dir_all(rootfs_dir.join(relative))?;
    }
    Ok(())
}

fn install_guest_agent(rootfs_dir: &Path, guest_agent: &Path) -> anyhow::Result<()> {
    let target = rootfs_dir.join("usr/local/bin/sagens-guest-agent");
    fs::copy(guest_agent, &target)
        .with_context(|| format!("copying guest agent to {}", target.display()))?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn write_guest_init(rootfs_dir: &Path) -> anyhow::Result<()> {
    fs::write(
        rootfs_dir.join("etc/inittab"),
        concat!(
            "::sysinit:/bin/mount -t proc proc /proc\n",
            "::sysinit:/bin/mount -t sysfs sysfs /sys\n",
            "::sysinit:/bin/mount -t devtmpfs devtmpfs /dev\n",
            "::once:/usr/local/bin/sagens-guest-agent\n",
            "::shutdown:/bin/umount -a -r\n"
        ),
    )?;
    Ok(())
}

fn write_package_manager_defaults(
    rootfs_dir: &Path,
    configure_pip_cache: bool,
    configure_npm_cache: bool,
) -> anyhow::Result<()> {
    if configure_pip_cache {
        fs::write(
            rootfs_dir.join("etc/pip.conf"),
            "[global]\nno-index = true\nfind-links = /opt/sagens-cache/pip/wheels\n",
        )?;
    }
    if configure_npm_cache {
        fs::write(
            rootfs_dir.join("etc/npmrc"),
            "offline=true\nprefer-offline=true\ncache=/opt/sagens-cache/npm\n",
        )?;
    }
    Ok(())
}

fn run_command(mut command: Command, context: &str) -> anyhow::Result<()> {
    let output = command
        .output()
        .with_context(|| format!("{context}: spawning helper command"))?;
    if output.status.success() {
        return Ok(());
    }
    bail!(
        "{context} failed with status {}: {}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn ensure_real_bash(rootfs_dir: &Path) -> anyhow::Result<()> {
    let bin_bash = rootfs_dir.join("bin/bash");
    ensure!(
        bin_bash.is_file(),
        "expected Alpine rootfs to contain a real /bin/bash, but {} is missing",
        bin_bash.display()
    );
    let metadata = fs::symlink_metadata(&bin_bash)
        .with_context(|| format!("reading {}", bin_bash.display()))?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "expected {} to be a real bash binary, but it is a symlink",
        bin_bash.display()
    );
    ensure_usr_bin_bash(rootfs_dir)
}

fn ensure_usr_bin_bash(rootfs_dir: &Path) -> anyhow::Result<()> {
    let usr_bin_bash = rootfs_dir.join("usr/bin/bash");
    if usr_bin_bash.exists() {
        return Ok(());
    }
    if let Some(parent) = usr_bin_bash.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let link_target = PathBuf::from("/bin/bash");
    std::os::unix::fs::symlink(&link_target, &usr_bin_bash).with_context(|| {
        format!(
            "linking {} -> {}",
            usr_bin_bash.display(),
            link_target.display()
        )
    })?;
    Ok(())
}
