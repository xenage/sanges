use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    #[cfg(target_os = "linux")]
    println!(
        "cargo:rustc-cdylib-link-arg=-Wl,-soname,libkrun.so.{}",
        env::var("CARGO_PKG_VERSION_MAJOR").unwrap()
    );
    #[cfg(target_os = "macos")]
    println!(
        "cargo:rustc-cdylib-link-arg=-Wl,-install_name,libkrun.{}.dylib,-compatibility_version,{}.0.0,-current_version,{}.{}.0",
        env::var("CARGO_PKG_VERSION_MAJOR").unwrap(),
        env::var("CARGO_PKG_VERSION_MAJOR").unwrap(),
        env::var("CARGO_PKG_VERSION_MAJOR").unwrap(),
        env::var("CARGO_PKG_VERSION_MINOR").unwrap()
    );
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=framework=Hypervisor");

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

    patched = replace_exact(
        patched,
        "use std::sync::LazyLock;\n",
        "#[cfg(not(all(target_os = \"linux\", target_arch = \"x86_64\", not(feature = \"tee\"))))]\nuse std::sync::LazyLock;\n",
    );

    patched = replace_exact(
        patched,
        "#[cfg(all(target_os = \"linux\", not(feature = \"tee\")))]\nconst KRUNFW_NAME: &str = \"libkrunfw.so.5\";\n",
        "#[cfg(all(target_os = \"linux\", target_arch = \"x86_64\", not(feature = \"tee\")))]\nconst KRUNFW_NAME: &str = \"statically linked libkrunfw\";\n#[cfg(all(target_os = \"linux\", not(feature = \"tee\"), not(target_arch = \"x86_64\")))]\nconst KRUNFW_NAME: &str = \"libkrunfw.so.5\";\n",
    );

    patched = replace_exact(
        patched,
        "static KRUNFW: LazyLock<Option<libloading::Library>> =\n    LazyLock::new(|| unsafe { libloading::Library::new(KRUNFW_NAME).ok() });\n\npub struct KrunfwBindings {\n    get_kernel: libloading::Symbol<\n        'static,\n        unsafe extern \"C\" fn(*mut u64, *mut u64, *mut size_t) -> *mut c_char,\n    >,\n    #[cfg(feature = \"tee\")]\n    get_initrd: libloading::Symbol<'static, unsafe extern \"C\" fn(*mut size_t) -> *mut c_char>,\n    #[cfg(feature = \"tee\")]\n    get_qboot: libloading::Symbol<'static, unsafe extern \"C\" fn(*mut size_t) -> *mut c_char>,\n}\n\nimpl KrunfwBindings {\n    fn load_bindings() -> Result<KrunfwBindings, libloading::Error> {\n        let krunfw = match KRUNFW.as_ref() {\n            Some(krunfw) => krunfw,\n            None => return Err(libloading::Error::DlOpenUnknown),\n        };\n        Ok(unsafe {\n            KrunfwBindings {\n                get_kernel: krunfw.get(b\"krunfw_get_kernel\")?,\n                #[cfg(feature = \"tee\")]\n                get_initrd: krunfw.get(b\"krunfw_get_initrd\")?,\n                #[cfg(feature = \"tee\")]\n                get_qboot: krunfw.get(b\"krunfw_get_qboot\")?,\n            }\n        })\n    }\n\n    pub fn new() -> Option<Self> {\n        Self::load_bindings().ok()\n    }\n}\n",
        "#[cfg(not(all(target_os = \"linux\", target_arch = \"x86_64\", not(feature = \"tee\"))))]\nstatic KRUNFW: LazyLock<Option<libloading::Library>> =\n    LazyLock::new(|| unsafe { libloading::Library::new(KRUNFW_NAME).ok() });\n\n#[cfg(all(target_os = \"linux\", target_arch = \"x86_64\", not(feature = \"tee\")))]\nunsafe extern \"C\" {\n    fn krunfw_get_kernel(\n        kernel_guest_addr: *mut u64,\n        kernel_entry_addr: *mut u64,\n        kernel_size: *mut size_t,\n    ) -> *mut c_char;\n}\n\n#[cfg(all(target_os = \"linux\", target_arch = \"x86_64\", not(feature = \"tee\")))]\npub struct KrunfwBindings {\n    get_kernel: unsafe extern \"C\" fn(*mut u64, *mut u64, *mut size_t) -> *mut c_char,\n    #[cfg(feature = \"tee\")]\n    get_initrd: unsafe extern \"C\" fn(*mut size_t) -> *mut c_char,\n    #[cfg(feature = \"tee\")]\n    get_qboot: unsafe extern \"C\" fn(*mut size_t) -> *mut c_char,\n}\n\n#[cfg(not(all(target_os = \"linux\", target_arch = \"x86_64\", not(feature = \"tee\"))))]\npub struct KrunfwBindings {\n    get_kernel: libloading::Symbol<\n        'static,\n        unsafe extern \"C\" fn(*mut u64, *mut u64, *mut size_t) -> *mut c_char,\n    >,\n    #[cfg(feature = \"tee\")]\n    get_initrd: libloading::Symbol<'static, unsafe extern \"C\" fn(*mut size_t) -> *mut c_char>,\n    #[cfg(feature = \"tee\")]\n    get_qboot: libloading::Symbol<'static, unsafe extern \"C\" fn(*mut size_t) -> *mut c_char>,\n}\n\n#[cfg(all(target_os = \"linux\", target_arch = \"x86_64\", not(feature = \"tee\")))]\nimpl KrunfwBindings {\n    pub fn new() -> Option<Self> {\n        Some(Self {\n            get_kernel: krunfw_get_kernel,\n            #[cfg(feature = \"tee\")]\n            get_initrd: krunfw_get_initrd,\n            #[cfg(feature = \"tee\")]\n            get_qboot: krunfw_get_qboot,\n        })\n    }\n}\n\n#[cfg(not(all(target_os = \"linux\", target_arch = \"x86_64\", not(feature = \"tee\"))))]\nimpl KrunfwBindings {\n    fn load_bindings() -> Result<KrunfwBindings, libloading::Error> {\n        let krunfw = match KRUNFW.as_ref() {\n            Some(krunfw) => krunfw,\n            None => return Err(libloading::Error::DlOpenUnknown),\n        };\n        Ok(unsafe {\n            KrunfwBindings {\n                get_kernel: krunfw.get(b\"krunfw_get_kernel\")?,\n                #[cfg(feature = \"tee\")]\n                get_initrd: krunfw.get(b\"krunfw_get_initrd\")?,\n                #[cfg(feature = \"tee\")]\n                get_qboot: krunfw.get(b\"krunfw_get_qboot\")?,\n            }\n        })\n    }\n\n    pub fn new() -> Option<Self> {\n        Self::load_bindings().ok()\n    }\n}\n",
    );

    patched = replace_exact(
        patched,
        "unsafe fn load_krunfw_payload(\n    krunfw: &KrunfwBindings,\n    vmr: &mut VmResources,\n) -> Result<(), libloading::Error> {\n",
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
