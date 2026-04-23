use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let upstream_src = manifest_dir
        .join("../../third_party/upstream/libkrun/src/hvf/src")
        .canonicalize()
        .unwrap();

    emit_rerun_if_changed(&upstream_src);
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/weak_hypervisor.c");
    println!("cargo:rustc-link-lib=framework=Hypervisor");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file(manifest_dir.join("src/weak_hypervisor.c"))
            .compile("sagens_hvf_weak");
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let copied_src = out_dir.join("hvf-src");
    if copied_src.exists() {
        fs::remove_dir_all(&copied_src).unwrap();
    }
    copy_dir_all(&upstream_src, &copied_src).unwrap();

    let patched = patch_hvf_source(
        &fs::read_to_string(copied_src.join("lib.rs")).unwrap(),
        &copied_src.join("bindings.rs"),
    );
    fs::write(out_dir.join("lib.rs"), patched).unwrap();
}

fn patch_hvf_source(source: &str, bindings_path: &Path) -> String {
    let mut patched = source.to_owned();

    patched = replace_exact(
        patched,
        "pub mod bindings;\n",
        &format!("#[path = {:?}]\npub mod bindings;\n", bindings_path),
    );
    patched = replace_exact(
        patched,
        "use std::sync::{Arc, LazyLock};\n",
        "use std::sync::Arc;\n",
    );
    patched = replace_line_containing(
        patched,
        "    FindSymbol(",
        "    FindSymbol(&'static str),\n",
    );
    patched = replace_exact(
        patched,
        "extern \"C\" {\n    pub fn mach_absolute_time() -> u64;\n}\n",
        "extern \"C\" {\n    pub fn mach_absolute_time() -> u64;\n    fn sagens_hvf_vm_config_get_el2_supported(el2_supported: *mut bool) -> hv_return_t;\n    fn sagens_hvf_vm_config_set_el2_enabled(config: hv_vm_config_t, el2_enabled: bool) -> hv_return_t;\n}\n\nconst HVF_SYMBOL_MISSING: hv_return_t = -1;\n",
    );
    patched = replace_range(
        patched,
        "pub fn check_nested_virt() -> Result<bool, Error> {\n",
        "impl HvfVm {\n",
        "pub fn check_nested_virt() -> Result<bool, Error> {\n    let mut el2_supported = false;\n    let ret = unsafe { sagens_hvf_vm_config_get_el2_supported(&mut el2_supported) };\n    if ret == HVF_SYMBOL_MISSING {\n        info!(\"cannot find hv_vm_config_get_el2_supported symbol\");\n        return Ok(false);\n    }\n    if ret != HV_SUCCESS {\n        error!(\"hv_vm_config_get_el2_supported failed: {ret:?}\");\n        return Err(Error::NestedCheck);\n    }\n\n    Ok(el2_supported)\n}\n\npub struct HvfVm {}\n\n",
    );
    patched = replace_range(
        patched,
        "        if nested_enabled {\n",
        "        let ret = unsafe { hv_vm_create(config) };\n",
        "        if nested_enabled {\n            let ret = unsafe { sagens_hvf_vm_config_set_el2_enabled(config, true) };\n            if ret == HVF_SYMBOL_MISSING {\n                return Err(Error::FindSymbol(\"hv_vm_config_set_el2_enabled\"));\n            }\n            if ret != HV_SUCCESS {\n                return Err(Error::EnableEL2);\n            }\n        }\n\n",
    );

    patched
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn emit_rerun_if_changed(path: &Path) {
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            emit_rerun_if_changed(&entry.unwrap().path());
        }
        return;
    }
    println!("cargo:rerun-if-changed={}", path.display());
}

fn replace_exact(source: String, from: &str, to: &str) -> String {
    if !source.contains(from) {
        panic!("failed to patch hvf source for snippet:\n{from}");
    }
    source.replacen(from, to, 1)
}

fn replace_line_containing(source: String, needle: &str, replacement: &str) -> String {
    let needle_offset = source
        .find(needle)
        .unwrap_or_else(|| panic!("failed to find hvf line containing:\n{needle}"));
    let line_start = source[..needle_offset].rfind('\n').map_or(0, |idx| idx + 1);
    let line_end = source[needle_offset..]
        .find('\n')
        .map_or(source.len(), |idx| needle_offset + idx + 1);

    format!(
        "{}{}{}",
        &source[..line_start],
        replacement,
        &source[line_end..]
    )
}

fn replace_range(source: String, start: &str, end: &str, replacement: &str) -> String {
    let start_idx = source
        .find(start)
        .unwrap_or_else(|| panic!("failed to find hvf patch start:\n{start}"));
    let end_idx = source[start_idx..]
        .find(end)
        .map(|idx| start_idx + idx)
        .unwrap_or_else(|| panic!("failed to find hvf patch end:\n{end}"));

    format!(
        "{}{}{}",
        &source[..start_idx],
        replacement,
        &source[end_idx..]
    )
}
