use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source_path = manifest_dir
        .join("../../third_party/upstream/libkrun/src/libkrun/src/lib.rs")
        .canonicalize()
        .unwrap();
    println!("cargo:rerun-if-changed={}", source_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    let source = fs::read_to_string(&source_path).unwrap();
    let patched = apply_static_krunfw_patch(&source);
    let out_path = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("lib.rs");
    fs::write(out_path, patched).unwrap();
}

fn apply_static_krunfw_patch(source: &str) -> String {
    let mut patched = source.to_owned();

    patched = replace_exact(patched, "use std::sync::LazyLock;\n", "");

    patched = replace_exact(
        patched,
        "#[cfg(all(target_os = \"linux\", not(feature = \"tee\")))]\nconst KRUNFW_NAME: &str = \"libkrunfw.so.5\";\n#[cfg(all(target_os = \"linux\", feature = \"amd-sev\"))]\nconst KRUNFW_NAME: &str = \"libkrunfw-sev.so.5\";\n#[cfg(all(target_os = \"linux\", feature = \"tdx\"))]\nconst KRUNFW_NAME: &str = \"libkrunfw-tdx.so.5\";\n#[cfg(target_os = \"macos\")]\nconst KRUNFW_NAME: &str = \"libkrunfw.5.dylib\";\n",
        "const KRUNFW_NAME: &str = \"statically linked libkrunfw\";\n",
    );

    patched = replace_range(
        patched,
        "static KRUNFW:",
        "#[derive(Clone)]\n",
        "#[cfg(all(target_os = \"linux\", any(target_arch = \"x86_64\", target_arch = \"aarch64\"), not(feature = \"tee\")))]\nunsafe extern \"C\" {\n    fn krunfw_get_kernel(\n        kernel_guest_addr: *mut u64,\n        kernel_entry_addr: *mut u64,\n        kernel_size: *mut size_t,\n    ) -> *mut c_char;\n}\n\npub struct KrunfwBindings {\n    get_kernel: unsafe extern \"C\" fn(*mut u64, *mut u64, *mut size_t) -> *mut c_char,\n    #[cfg(feature = \"tee\")]\n    get_initrd: unsafe extern \"C\" fn(*mut size_t) -> *mut c_char,\n    #[cfg(feature = \"tee\")]\n    get_qboot: unsafe extern \"C\" fn(*mut size_t) -> *mut c_char,\n}\n\n#[cfg(all(target_os = \"linux\", any(target_arch = \"x86_64\", target_arch = \"aarch64\"), not(feature = \"tee\")))]\nimpl KrunfwBindings {\n    pub fn new() -> Option<Self> {\n        Some(Self {\n            get_kernel: krunfw_get_kernel,\n            #[cfg(feature = \"tee\")]\n            get_initrd: krunfw_get_initrd,\n            #[cfg(feature = \"tee\")]\n            get_qboot: krunfw_get_qboot,\n        })\n    }\n}\n\n#[cfg(not(all(target_os = \"linux\", any(target_arch = \"x86_64\", target_arch = \"aarch64\"), not(feature = \"tee\"))))]\nimpl KrunfwBindings {\n    pub fn new() -> Option<Self> {\n        None\n    }\n}\n\n",
    );

    patched = replace_range(
        patched,
        "unsafe fn load_krunfw_payload(\n",
        "    let mut kernel_guest_addr: u64 = 0;\n",
        "unsafe fn load_krunfw_payload(krunfw: &KrunfwBindings, vmr: &mut VmResources) {\n",
    );

    patched = replace_exact(patched, "\n    Ok(())\n}\n", "\n}\n");

    patched = replace_exact(
        patched,
        "        if let Some(ref krunfw) = ctx_cfg.krunfw {\n            if let Err(err) = unsafe { load_krunfw_payload(krunfw, &mut ctx_cfg.vmr) } {\n                eprintln!(\"Can't load libkrunfw symbols: {err}\");\n                return -libc::ENOENT;\n            }\n        } else {\n",
        "        if let Some(ref krunfw) = ctx_cfg.krunfw {\n            unsafe { load_krunfw_payload(krunfw, &mut ctx_cfg.vmr) };\n        } else {\n",
    );

    patched
}

fn replace_exact(mut source: String, from: &str, to: &str) -> String {
    if !source.contains(from) {
        panic!("failed to apply libkrun overlay patch for snippet:\n{from}");
    }
    source = source.replacen(from, to, 1);
    source
}

fn replace_range(source: String, start: &str, end: &str, replacement: &str) -> String {
    let start_idx = source
        .find(start)
        .unwrap_or_else(|| panic!("failed to find libkrun patch start:\n{start}"));
    let end_idx = source[start_idx..]
        .find(end)
        .map(|idx| start_idx + idx)
        .unwrap_or_else(|| panic!("failed to find libkrun patch end:\n{end}"));

    format!(
        "{}{}{}",
        &source[..start_idx],
        replacement,
        &source[end_idx..]
    )
}
