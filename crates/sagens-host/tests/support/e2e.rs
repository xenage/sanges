use std::fs;
use std::path::{Path, PathBuf};

pub struct GuestAssets {
    pub kernel_image: PathBuf,
    pub rootfs_image: PathBuf,
    pub firmware: Option<PathBuf>,
}

pub fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn enabled() -> bool {
    matches!(env("SAGENS_RUN_E2E").as_deref(), Some("1" | "true" | "yes"))
}

pub fn state_dir() -> PathBuf {
    env("SAGENS_E2E_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let temp = tempfile::tempdir().expect("tempdir");
            temp.keep()
        })
}

pub fn default_host_binary() -> String {
    repo_root()
        .join("target/debug/sagens")
        .display()
        .to_string()
}

pub fn host_binary() -> PathBuf {
    env("SAGENS_HOST_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_host_binary()))
}

pub fn default_wheelhouse() -> Option<PathBuf> {
    first_existing_dir(&[repo_root().join(".e2e-wheelhouse")])
}

pub fn guest_assets() -> GuestAssets {
    let repo_root = repo_root();
    let guest_dir = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") | ("linux", "x86_64") => repo_root.join("artifacts/alpine-x86_64"),
        ("macos", "aarch64") | ("linux", "aarch64") => repo_root.join("artifacts/alpine-aarch64"),
        (os, arch) => panic!("unsupported e2e host platform {os}/{arch}"),
    };
    let kernel_image = env("SAGENS_KERNEL")
        .map(PathBuf::from)
        .unwrap_or_else(|| guest_dir.join("vmlinuz-virt"));
    let rootfs_image = env("SAGENS_ROOTFS")
        .map(PathBuf::from)
        .unwrap_or_else(|| guest_dir.join("rootfs.raw"));
    let firmware = env("SAGENS_FIRMWARE").map(PathBuf::from).or_else(|| {
        (std::env::consts::OS == "macos")
            .then(|| repo_root.join("third_party/upstream/libkrun/edk2/KRUN_EFI.silent.fd"))
    });
    assert!(
        kernel_image.is_file(),
        "missing e2e guest kernel: {}",
        kernel_image.display()
    );
    assert!(
        rootfs_image.is_file(),
        "missing e2e guest rootfs: {}",
        rootfs_image.display()
    );
    if let Some(path) = &firmware {
        assert!(
            path.is_file(),
            "missing e2e guest firmware: {}",
            path.display()
        );
    }
    GuestAssets {
        kernel_image,
        rootfs_image,
        firmware,
    }
}

pub fn ensure_host_binary_ready(host_binary: &Path) {
    #[cfg(target_os = "macos")]
    {
        let repo_root = repo_root();
        let entitlements = repo_root.join("macos/sagens.entitlements");
        let status = std::process::Command::new("codesign")
            .args([
                "--force",
                "--sign",
                "-",
                "--entitlements",
                entitlements.to_str().expect("utf-8 entitlements path"),
                "--timestamp=none",
                host_binary.to_str().expect("utf-8 host binary path"),
            ])
            .status()
            .expect("codesign host binary");
        assert!(status.success(), "codesign failed with {status}");
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = host_binary;
    }
}

#[cfg(target_os = "macos")]
pub fn configure_libkrun_test_helper() {
    let host_binary = host_binary();
    ensure_host_binary_ready(&host_binary);
    unsafe {
        std::env::set_var("SAGENS_HOST_BINARY", &host_binary);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn configure_libkrun_test_helper() {}

pub async fn upload_directory_box(
    client: &sagens_host::BoxApiClient,
    box_id: uuid::Uuid,
    host_dir: &Path,
    guest_dir: &Path,
) {
    client
        .make_dir(box_id, guest_dir.display().to_string(), true)
        .await
        .expect("mkdir");
    upload_dir_entries(client, box_id, host_dir, guest_dir).await;
}

async fn upload_dir_entries(
    client: &sagens_host::BoxApiClient,
    box_id: uuid::Uuid,
    host_dir: &Path,
    guest_dir: &Path,
) {
    let mut entries = fs::read_dir(host_dir)
        .expect("read host dir")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect host dir");
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let host_path = entry.path();
        let guest_path = guest_dir.join(entry.file_name());
        if host_path.is_dir() {
            client
                .make_dir(box_id, guest_path.display().to_string(), true)
                .await
                .expect("mkdir");
            Box::pin(upload_dir_entries(client, box_id, &host_path, &guest_path)).await;
            continue;
        }
        client
            .write_file(
                box_id,
                guest_path.display().to_string(),
                fs::read(&host_path).expect("read host file"),
                true,
            )
            .await
            .expect("write");
    }
}

fn first_existing_dir(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.is_dir()).cloned()
}
