use std::path::PathBuf;

use crate::config::GuestConfig;
use crate::workspace::validate_persisted_id;
use crate::{Result, SandboxError};

pub const BASE_IMAGE_NAME: &str = "base";

pub(super) const BASE_URL: &str = "https://dl-cdn.alpinelinux.org/alpine/v3.23";
pub(super) const ALPINE_VERSION: &str = "3.23.4";
pub(super) const ALPINE_RELEASE_KEY_URL: &str = "https://alpinelinux.org/keys/ncopa.asc";
pub(super) const EXPECTED_SIGNING_FINGERPRINT: &str = "0482D84022F52DF1C4E7CD43293ACD0907D9495A";
pub(super) const IMAGE_MANIFEST: &str = "image.json";
pub(super) const DEFAULT_IMAGE_MIB: u64 = 512;
pub(super) const CACHE_IMAGE_MIB: u64 = 64;
pub(super) const BASE_APK_PACKAGES: &[&str] = &[
    "bash",
    "python3",
    "py3-pip",
    "ca-certificates-bundle",
    "busybox-binsh",
    "linux-virt",
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VmImageManifest {
    pub version: u32,
    pub name: String,
    pub arch: String,
    pub alpine_version: String,
    pub created_at_ms: u64,
    #[serde(default)]
    pub apk: Vec<String>,
    #[serde(default)]
    pub pip: Vec<String>,
    #[serde(default)]
    pub npm: Vec<String>,
    pub kernel_image: String,
    pub rootfs_image: String,
    #[serde(default)]
    pub cache_image: Option<String>,
    #[serde(default)]
    pub package_count: usize,
}

#[derive(Debug, Clone)]
pub struct ImageBuildSpec {
    pub state_dir: PathBuf,
    pub name: String,
    pub apk: Vec<String>,
    pub pip: Vec<String>,
    pub npm: Vec<String>,
    pub min_image_mib: u64,
    pub force_refresh: bool,
    pub base_guest: GuestConfig,
}

#[derive(Debug, Clone)]
pub struct ResolvedImageGuest {
    pub guest: GuestConfig,
    pub cache_image: Option<PathBuf>,
}

pub fn default_guest_arch() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") | ("linux", "aarch64") => Ok("aarch64"),
        ("macos", "x86_64") | ("linux", "x86_64") => Ok("x86_64"),
        (os, arch) => Err(SandboxError::UnsupportedHost(format!(
            "unsupported host platform for guest image defaults: {os}/{arch}"
        ))),
    }
}

pub fn validate_image_name(name: &str) -> Result<()> {
    validate_persisted_id(name, "image name")
}
