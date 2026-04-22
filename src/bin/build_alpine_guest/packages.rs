use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::{Context, bail, ensure};
use reqwest::blocking::Client;

use super::image::unpack_with_tar;
use super::verify::extract_member_from_tar_gz;

#[derive(Clone, Debug)]
pub(super) struct Package {
    pub(super) name: String,
    version: String,
    repo: String,
    depends: Vec<String>,
    provides: Vec<String>,
}

pub(super) fn validate_guest_agent_binary(path: &Path, arch: &str) -> anyhow::Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    ensure!(
        bytes.len() >= 0x14,
        "guest agent binary is too small to validate: {}",
        path.display()
    );
    ensure!(
        &bytes[..4] == b"\x7FELF",
        "guest agent must be a Linux ELF binary for guest arch {arch}, got a non-ELF file: {}",
        path.display()
    );
    ensure!(
        bytes[4] == 2,
        "guest agent must be a 64-bit ELF binary for guest arch {arch}: {}",
        path.display()
    );
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    let expected_machine = match arch {
        "aarch64" => 183,
        "x86_64" => 62,
        _ => bail!("unsupported guest arch for ELF validation: {arch}"),
    };
    ensure!(
        machine == expected_machine,
        "guest agent ELF machine mismatch for arch {arch}: expected {expected_machine}, got {machine} ({})",
        path.display()
    );
    Ok(())
}

pub(super) fn load_indexes(
    index_dir: &Path,
) -> anyhow::Result<(BTreeMap<String, Package>, BTreeMap<String, String>)> {
    let mut packages = BTreeMap::new();
    let mut providers = BTreeMap::new();
    for repo in ["main", "community"] {
        let index_path = index_dir.join(format!("APKINDEX-{repo}"));
        let content = fs::read_to_string(&index_path)
            .with_context(|| format!("reading {}", index_path.display()))?;
        for block in content.trim().split("\n\n") {
            let mut fields = BTreeMap::<String, Vec<String>>::new();
            for line in block.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    fields
                        .entry(key.to_owned())
                        .or_default()
                        .push(value.to_owned());
                }
            }
            let Some(name) = first_field(&fields, "P") else {
                continue;
            };
            let Some(version) = first_field(&fields, "V") else {
                continue;
            };
            let package = Package {
                name: name.to_owned(),
                version: version.to_owned(),
                repo: repo.to_owned(),
                depends: join_fields(&fields, "D"),
                provides: join_fields(&fields, "p"),
            };
            providers
                .entry(package.name.clone())
                .or_insert_with(|| package.name.clone());
            for entry in &package.provides {
                providers
                    .entry(strip_constraint(entry))
                    .or_insert_with(|| package.name.clone());
            }
            packages.insert(package.name.clone(), package);
        }
    }
    Ok((packages, providers))
}

pub(super) fn resolve_packages(
    packages: &BTreeMap<String, Package>,
    providers: &BTreeMap<String, String>,
    wanted: &[&str],
) -> anyhow::Result<Vec<Package>> {
    let mut resolved = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<String> = wanted.iter().map(|name| (*name).to_owned()).collect();
    while let Some(token) = stack.pop() {
        let Some(name) = normalize_dep(providers, &token) else {
            continue;
        };
        if !seen.insert(name.clone()) {
            continue;
        }
        let package = packages
            .get(&name)
            .with_context(|| format!("missing package in APKINDEX: {name}"))?
            .clone();
        stack.extend(package.depends.iter().cloned());
        resolved.push(package);
    }
    resolved.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(resolved)
}

pub(super) fn download_packages(
    client: &Client,
    apk_dir: &Path,
    arch: &str,
    resolved: &[Package],
) -> anyhow::Result<()> {
    let manifest = resolved
        .iter()
        .map(|package| package.name.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(apk_dir.join("manifest.txt"), format!("{manifest}\n"))?;
    for package in resolved {
        let file_name = format!("{}-{}.apk", package.name, package.version);
        let target = apk_dir.join(&file_name);
        let url = format!("{}/{}/{arch}/{file_name}", crate::BASE_URL, package.repo);
        let response = client
            .get(&url)
            .send()
            .with_context(|| format!("requesting {url}"))?
            .error_for_status()
            .with_context(|| format!("downloading {url}"))?;
        let bytes = response.bytes().with_context(|| format!("reading {url}"))?;
        fs::write(&target, &bytes).with_context(|| format!("writing {}", target.display()))?;
    }
    Ok(())
}

pub(super) fn rebuild_rootfs(
    rootfs_tar: &Path,
    apk_dir: &Path,
    resolved: &[Package],
    rootfs_dir: &Path,
    guest_agent: &Path,
) -> anyhow::Result<()> {
    if rootfs_dir.exists() {
        fs::remove_dir_all(rootfs_dir)
            .with_context(|| format!("removing {}", rootfs_dir.display()))?;
    }
    fs::create_dir_all(rootfs_dir)?;
    unpack_with_tar(rootfs_tar, rootfs_dir)?;
    for package in resolved {
        let archive = apk_dir.join(format!("{}-{}.apk", package.name, package.version));
        unpack_with_tar(&archive, rootfs_dir)?;
    }
    for relative in [
        "proc",
        "sys",
        "dev",
        "home",
        "tmp",
        "workspace",
        "usr/local/bin",
        "var/cache/apk",
    ] {
        fs::create_dir_all(rootfs_dir.join(relative))?;
    }
    let target = rootfs_dir.join("usr/local/bin/sagens-guest-agent");
    fs::copy(guest_agent, &target)
        .with_context(|| format!("copying guest agent to {}", target.display()))?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
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
    ensure_real_bash(rootfs_dir)?;
    Ok(())
}

pub(super) fn extract_kernel(
    apk_dir: &Path,
    package: &Package,
    destination: &Path,
) -> anyhow::Result<()> {
    let archive_path = apk_dir.join(format!("{}-{}.apk", package.name, package.version));
    extract_member_from_tar_gz(&archive_path, "boot/vmlinuz-virt", destination)
}

fn first_field<'a>(fields: &'a BTreeMap<String, Vec<String>>, key: &str) -> Option<&'a str> {
    fields
        .get(key)
        .and_then(|values| values.first())
        .map(String::as_str)
}

fn join_fields(fields: &BTreeMap<String, Vec<String>>, key: &str) -> Vec<String> {
    fields
        .get(key)
        .into_iter()
        .flat_map(|values| values.iter())
        .flat_map(|value| value.split_whitespace())
        .map(ToOwned::to_owned)
        .collect()
}

fn strip_constraint(token: &str) -> String {
    let mut value = token.trim();
    for marker in ["!", "?", "<", ">", "=", "~"] {
        if let Some((left, _)) = value.split_once(marker) {
            value = left;
        }
    }
    value.trim().to_owned()
}

fn normalize_dep(providers: &BTreeMap<String, String>, token: &str) -> Option<String> {
    let dep = strip_constraint(token);
    if dep.is_empty() {
        return None;
    }
    if dep.starts_with("so:") || dep.starts_with("cmd:") {
        return providers.get(&dep).cloned();
    }
    Some(providers.get(&dep).cloned().unwrap_or(dep))
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
