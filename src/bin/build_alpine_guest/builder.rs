use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, ensure};
use reqwest::blocking::Client;
use sequoia_openpgp as openpgp;
use sequoia_openpgp::cert::CertParser;
use sequoia_openpgp::parse::Parse;

use super::args::{Args, absolute_path, rootfs_tar_name};
use super::image::build_ext4_image;
use super::packages::{
    download_packages, extract_kernel, load_indexes, rebuild_rootfs, resolve_packages,
    validate_guest_agent_binary,
};
use super::verify::{extract_member_from_tar_gz, verify_detached_signature, verify_sha256};

pub(super) struct Builder {
    client: Client,
    signing_cert: openpgp::Cert,
}

impl Builder {
    pub(super) fn new() -> anyhow::Result<Self> {
        let client = Client::builder()
            .user_agent("agent-box/build-alpine-guest")
            .timeout(Duration::from_secs(300))
            .build()
            .context("building HTTP client")?;
        let signing_key = client
            .get(crate::ALPINE_RELEASE_KEY_URL)
            .send()
            .context("requesting Alpine release signing key")?
            .error_for_status()
            .context("downloading Alpine release signing key")?
            .text()
            .context("reading Alpine release signing key")?;
        let signing_cert = CertParser::from_bytes(signing_key.as_bytes())
            .context("parsing Alpine release signing keyring")?
            .find_map(|item| match item {
                Ok(cert)
                    if cert.fingerprint().to_string() == crate::EXPECTED_SIGNING_FINGERPRINT =>
                {
                    Some(Ok(cert))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .transpose()?
            .context("expected Alpine release signing cert not found in vendored keyring")?;
        ensure!(
            signing_cert.fingerprint().to_string() == crate::EXPECTED_SIGNING_FINGERPRINT,
            "vendored Alpine release key fingerprint mismatch"
        );
        Ok(Self {
            client,
            signing_cert,
        })
    }

    pub(super) fn run(&self, args: Args) -> anyhow::Result<()> {
        let work_dir = absolute_path(&args.work_dir)?;
        let output_dir = absolute_path(&args.output_dir)?;
        let guest_agent = absolute_path(&args.guest_agent)?;
        validate_guest_agent_binary(&guest_agent, &args.arch)?;
        let downloads = work_dir.join("downloads");
        let indexes = work_dir.join("index");
        let apk_dir = work_dir.join("apk");
        let rootfs_dir = work_dir.join("rootfs-dir");
        fs::create_dir_all(&downloads)?;
        fs::create_dir_all(&indexes)?;
        fs::create_dir_all(&apk_dir)?;
        fs::create_dir_all(&output_dir)?;

        let tar_name = rootfs_tar_name(&args.arch);
        let rootfs_tar = downloads.join(&tar_name);
        self.fetch_rootfs(&args.arch, &rootfs_tar, &tar_name)?;
        self.fetch_indexes(&indexes, &args.arch)?;
        let (packages, providers) = load_indexes(&indexes)?;
        let resolved = resolve_packages(&packages, &providers, crate::WANTED_PACKAGES)?;
        download_packages(&self.client, &apk_dir, &args.arch, &resolved)?;
        rebuild_rootfs(&rootfs_tar, &apk_dir, &resolved, &rootfs_dir, &guest_agent)?;

        let kernel_package = resolved
            .iter()
            .find(|package| package.name == "linux-virt")
            .context("linux-virt package missing from resolved package set")?;
        let kernel_path = output_dir.join("vmlinuz-virt");
        extract_kernel(&apk_dir, kernel_package, &kernel_path)?;
        build_ext4_image(
            &rootfs_dir,
            &output_dir.join("rootfs.raw"),
            args.min_image_mib,
        )
    }

    fn fetch_rootfs(&self, arch: &str, destination: &Path, tar_name: &str) -> anyhow::Result<()> {
        let url = format!("{}/releases/{arch}/{tar_name}", crate::BASE_URL);
        let checksum_path = destination.with_file_name(format!("{tar_name}.sha256"));
        let signature_path = destination.with_file_name(format!("{tar_name}.asc"));
        self.fetch_to_file(&url, destination)?;
        self.fetch_to_file(&format!("{url}.sha256"), &checksum_path)?;
        self.fetch_to_file(&format!("{url}.asc"), &signature_path)?;
        verify_sha256(destination, &checksum_path)?;
        verify_detached_signature(destination, &signature_path, &self.signing_cert)
    }

    fn fetch_indexes(&self, index_dir: &Path, arch: &str) -> anyhow::Result<()> {
        for repo in ["main", "community"] {
            let url = format!("{}/{repo}/{arch}/APKINDEX.tar.gz", crate::BASE_URL);
            let archive_path = index_dir.join(format!("APKINDEX-{repo}.tar.gz"));
            self.fetch_to_file(&url, &archive_path)?;
            let index_path = index_dir.join(format!("APKINDEX-{repo}"));
            extract_member_from_tar_gz(&archive_path, "APKINDEX", &index_path)?;
        }
        Ok(())
    }

    fn fetch_to_file(&self, url: &str, destination: &Path) -> anyhow::Result<()> {
        destination.parent().map(fs::create_dir_all).transpose()?;
        let response = self
            .client
            .get(url)
            .send()
            .with_context(|| format!("requesting {url}"))?
            .error_for_status()
            .with_context(|| format!("downloading {url}"))?;
        let bytes = response.bytes().with_context(|| format!("reading {url}"))?;
        fs::write(destination, &bytes)
            .with_context(|| format!("writing {}", destination.display()))?;
        Ok(())
    }
}
