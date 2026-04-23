use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let upstream_root = manifest_dir
        .join("../../third_party/upstream/libkrun")
        .canonicalize()
        .unwrap();
    let upstream_src = upstream_root.join("src/vmm/src");

    emit_rerun_if_changed(&upstream_src);
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64") {
        let edk2_binary_path = std::env::var("KRUN_EDK2_BINARY_PATH").unwrap_or_else(|_| {
            upstream_root
                .join("edk2/KRUN_EFI.silent.fd")
                .display()
                .to_string()
        });
        println!("cargo:rustc-env=KRUN_EDK2_BINARY_PATH={edk2_binary_path}");
        println!("cargo:rerun-if-env-changed=KRUN_EDK2_BINARY_PATH");
    }

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let copied_src = out_dir.join("vmm-src");
    if copied_src.exists() {
        fs::remove_dir_all(&copied_src).unwrap();
    }
    copy_dir_all(&upstream_src, &copied_src).unwrap();

    let root = generate_root_source(
        &fs::read_to_string(copied_src.join("lib.rs")).unwrap(),
        &copied_src,
    );
    fs::write(out_dir.join("lib.rs"), root).unwrap();
}

fn generate_root_source(source: &str, copied_src: &Path) -> String {
    let mut patched = replace_exact(
        source.to_owned(),
        "//! Virtual Machine Monitor that leverages the Linux Kernel-based Virtual Machine (KVM),\n//! and other virtualization features to run a single lightweight micro-virtual\n//! machine (microVM).\n",
        "// Virtual Machine Monitor that leverages the Linux Kernel-based Virtual Machine (KVM),\n// and other virtualization features to run a single lightweight micro-virtual\n// machine (microVM).\n",
    );
    patched = patched.replace("//!", "///");
    patched = replace_exact(
        patched,
        "pub mod builder;\n",
        &format!(
            "#[path = {:?}]\npub mod builder;\n",
            copied_src.join("builder.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "pub(crate) mod device_manager;\n",
        &format!(
            "#[path = {:?}]\npub(crate) mod device_manager;\n",
            copied_src.join("device_manager/mod.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "pub mod resources;\n",
        &format!(
            "#[path = {:?}]\npub mod resources;\n",
            copied_src.join("resources.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "#[cfg(target_os = \"linux\")]\npub mod signal_handler;\n",
        &format!(
            "#[cfg(target_os = \"linux\")]\n#[path = {:?}]\npub mod signal_handler;\n",
            copied_src.join("signal_handler.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "pub mod vmm_config;\n",
        &format!(
            "#[path = {:?}]\npub mod vmm_config;\n",
            copied_src.join("vmm_config/mod.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "#[cfg(target_os = \"linux\")]\nmod linux;\n",
        &format!(
            "#[cfg(target_os = \"linux\")]\n#[path = {:?}]\nmod linux;\n",
            copied_src.join("linux/mod.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "#[cfg(target_os = \"macos\")]\nmod macos;\n",
        &format!(
            "#[cfg(target_os = \"macos\")]\n#[path = {:?}]\nmod macos;\n",
            copied_src.join("macos/mod.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "mod terminal;\n",
        &format!(
            "#[path = {:?}]\nmod terminal;\n",
            copied_src.join("terminal.rs")
        ),
    );
    patched = replace_exact(
        patched,
        "pub mod worker;\n",
        &format!(
            "#[path = {:?}]\npub mod worker;\n",
            copied_src.join("worker.rs")
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
        panic!("failed to patch vmm source for snippet:\n{from}");
    }
    source.replacen(from, to, 1)
}
