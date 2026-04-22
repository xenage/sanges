use super::platform::{TargetPlatform, platform_from_parts};
use super::{
    ReleaseAsset, ReleaseMetadata, decode_sha256_hex, parse_sha256_manifest, select_release_assets,
};

#[test]
fn parses_supported_platforms() {
    assert_eq!(
        platform_from_parts("linux", "x86_64").expect("platform"),
        TargetPlatform::LinuxX86_64
    );
    assert_eq!(
        platform_from_parts("linux", "arm64").expect("platform"),
        TargetPlatform::LinuxAarch64
    );
    let error = platform_from_parts("macos", "x86_64").expect_err("unsupported intel mac");
    assert!(
        error
            .to_string()
            .contains("self-update is not supported on macOS x86_64"),
        "unexpected error: {error}"
    );
    assert_eq!(
        platform_from_parts("darwin", "arm64").expect("platform"),
        TargetPlatform::MacosAarch64
    );
}

#[test]
fn selects_platform_assets_from_release() {
    let release = ReleaseMetadata {
        tag_name: "v1.2.3".into(),
        assets: vec![
            ReleaseAsset {
                name: "sagens-v1.2.3-linux-x86_64".into(),
                browser_download_url: "https://example.invalid/bin".into(),
            },
            ReleaseAsset {
                name: "sagens-v1.2.3-linux-x86_64.sha256".into(),
                browser_download_url: "https://example.invalid/bin.sha256".into(),
            },
        ],
    };

    let (binary, checksum) =
        select_release_assets(&release, TargetPlatform::LinuxX86_64).expect("assets");

    assert_eq!(binary.name, "sagens-v1.2.3-linux-x86_64");
    assert_eq!(checksum.name, "sagens-v1.2.3-linux-x86_64.sha256");
}

#[test]
fn parses_sha256_manifest_line() {
    let digest = parse_sha256_manifest(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  sagens-v1.2.3-linux-x86_64\n",
        "sagens-v1.2.3-linux-x86_64",
    )
    .expect("digest");

    assert_eq!(digest, decode_sha256_hex(&"aa".repeat(32)).expect("hex"));
}

#[test]
fn accepts_binary_checksum_marker() {
    let digest = parse_sha256_manifest(
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb *sagens-v1.2.3-macos-aarch64\n",
        "sagens-v1.2.3-macos-aarch64",
    )
    .expect("digest");

    assert_eq!(digest, decode_sha256_hex(&"bb".repeat(32)).expect("hex"));
}
