# third_party

This directory is the project-local home for non-Rust runtime inputs used by `sagens`.

## Layout

- `upstream/`
  - source submodules for upstream projects used for provenance and rebuild workflows
- `runtime/`
  - materialized local runtime bundles produced by `cargo run --bin xtask -- ...`
  - generated local build state, not repo-owned source of truth

## Upstream submodules

The repository defines these submodules in `.gitmodules`:

- `third_party/upstream/libkrun`
- `third_party/upstream/libkrunfw`
- `third_party/upstream/krunkit`

`xtask` initializes them on demand when Git metadata is available, and the packaging path rebuilds the host runtime from `third_party/upstream/libkrun`.

## Runtime bundle

The default dev path is intentionally local-first:

- build the host binary from this repository
- embed the guest/runtime artifacts from this repository
- avoid manual `.env` files and ad-hoc path exports

`xtask` writes a project-local runtime bundle under `third_party/runtime/<platform>/`.
Treat that directory as generated local state: its provenance source remains the
tracked submodules under `third_party/upstream/`.

The intended flow is:

- build `libkrun` from `third_party/upstream/libkrun`
- stage the resulting runtime library into `third_party/runtime/<platform>/lib`
- stage any runtime support libraries required by that platform into the same bundle
- stage firmware into `third_party/runtime/<platform>/share/krunkit/` on macOS
- embed those paths into the standalone `sagens` binary through a temporary manifest consumed by `build.rs`

## Guest artifacts

Guest images remain under `artifacts/`, for example:

- `artifacts/alpine-aarch64/rootfs.raw`
- `artifacts/alpine-aarch64/vmlinuz-virt`
- `artifacts/alpine-aarch64/vmlinuz-virt.pe.gz`

Those are the project-local inputs used by the dev build unless explicitly overridden.
