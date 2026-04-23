use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let upstream_root = manifest_dir
        .join("../../third_party/upstream/libkrun")
        .canonicalize()
        .unwrap();
    let upstream_src = upstream_root.join("src/devices/src");

    emit_rerun_if_changed(&upstream_src);
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/hvfgicv3.rs");

    let init_binary_path = std::env::var_os("KRUN_INIT_BINARY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let init_path = build_default_init(&upstream_root);
            unsafe { std::env::set_var("KRUN_INIT_BINARY_PATH", &init_path) };
            init_path
        });
    println!(
        "cargo:rustc-env=KRUN_INIT_BINARY_PATH={}",
        init_binary_path.display()
    );
    println!("cargo:rerun-if-env-changed=KRUN_INIT_BINARY_PATH");

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let copied_src = out_dir.join("devices-src");
    if copied_src.exists() {
        fs::remove_dir_all(&copied_src).unwrap();
    }
    copy_dir_all(&upstream_src, &copied_src).unwrap();
    fs::copy(
        manifest_dir.join("src/hvfgicv3.rs"),
        copied_src.join("legacy/hvfgicv3.rs"),
    )
    .unwrap();

    let root = generate_root_source(
        &fs::read_to_string(copied_src.join("lib.rs")).unwrap(),
        &copied_src,
    );
    fs::write(out_dir.join("lib.rs"), root).unwrap();
}

fn build_default_init(upstream_root: &Path) -> PathBuf {
    let init_src = upstream_root.join("init/init.c");
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let init_bin = out_dir.join("init");

    println!("cargo:rerun-if-env-changed=CC_LINUX");
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=TIMESYNC");
    println!("cargo:rerun-if-changed={}", init_src.display());
    println!(
        "cargo:rerun-if-changed={}",
        upstream_root.join("init/jsmn.h").display()
    );

    let mut init_cc_flags = vec!["-O2", "-static", "-Wall"];
    if std::env::var_os("TIMESYNC").as_deref() == Some(OsStr::new("1")) {
        init_cc_flags.push("-D__TIMESYNC__");
    }

    let cc_value = std::env::var("CC_LINUX")
        .or_else(|_| std::env::var("CC"))
        .unwrap_or_else(|_| "cc".to_string());
    let mut cc_parts = cc_value.split_ascii_whitespace();
    let cc = cc_parts.next().expect("CC_LINUX/CC must not be empty");
    let status = Command::new(cc)
        .args(cc_parts)
        .args(&init_cc_flags)
        .arg("-o")
        .arg(&init_bin)
        .arg(&init_src)
        .status()
        .unwrap_or_else(|error| panic!("failed to execute {cc}: {error}"));

    if !status.success() {
        panic!("failed to compile init/init.c: {status}");
    }
    init_bin
}

fn generate_root_source(source: &str, copied_src: &Path) -> String {
    let mut patched = source.replacen("//! ", "// ", 1);
    patched = replace_exact(
        patched,
        "mod bus;\n",
        &format!("#[path = {:?}]\nmod bus;\n", copied_src.join("bus.rs")),
    );
    patched = replace_exact(
        patched,
        "#[cfg(any(target_arch = \"aarch64\", target_arch = \"riscv64\"))]\npub mod fdt;\n",
        &format!(
            "#[cfg(any(target_arch = \"aarch64\", target_arch = \"riscv64\"))]\n#[path = {:?}]\npub mod fdt;\n",
            copied_src.join("fdt/mod.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "pub mod legacy;\n",
        &format!(
            "#[path = {:?}]\npub mod legacy;\n",
            copied_src.join("legacy/mod.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "pub mod virtio;\n",
        &format!(
            "#[path = {:?}]\npub mod virtio;\n",
            copied_src.join("virtio/mod.rs")
        ),
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
        panic!("failed to patch devices source for snippet:\n{from}");
    }
    source.replacen(from, to, 1)
}
