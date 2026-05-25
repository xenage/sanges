use std::collections::BTreeMap;

use super::packages::{normalize_dep, strip_constraint, wanted_apk_packages};
use super::{BASE_IMAGE_NAME, validate_image_name};

#[test]
fn validates_image_names() {
    assert!(validate_image_name("chromium").is_ok());
    assert!(validate_image_name("py_312").is_ok());
    assert!(validate_image_name("../nope").is_err());
}

#[test]
fn base_image_name_is_stable() {
    assert_eq!(BASE_IMAGE_NAME, "base");
}

#[test]
fn wanted_apk_packages_include_base_and_extra() {
    let wanted = wanted_apk_packages(&["chromium".into(), "python3".into()], false);
    assert!(wanted.iter().any(|package| package == "chromium"));
    assert!(wanted.iter().any(|package| package == "python3"));
    assert!(wanted.iter().any(|package| package == "linux-virt"));
}

#[test]
fn wanted_apk_packages_include_npm_when_cache_requested() {
    let wanted = wanted_apk_packages(&[], true);
    assert!(wanted.iter().any(|package| package == "npm"));
}

#[test]
fn normalizes_virtual_providers() {
    let mut providers = BTreeMap::new();
    providers.insert("cmd:chromium".into(), "chromium".into());
    providers.insert("so:libx.so.1".into(), "libx".into());
    assert_eq!(
        normalize_dep(&providers, "cmd:chromium"),
        Some("chromium".into())
    );
    assert_eq!(
        normalize_dep(&providers, "so:libx.so.1>=1"),
        Some("libx".into())
    );
    assert_eq!(normalize_dep(&providers, "bash"), Some("bash".into()));
    assert_eq!(strip_constraint("pkg>=1"), "pkg");
}
