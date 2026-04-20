# runtime

This directory holds materialized local runtime bundles used by `cargo run --bin xtask -- ...`.
These bundles are generated local state rather than repo-owned source files.

Expected platform directories:

- `macos-aarch64`
- `macos-x86_64`
- `linux-aarch64`
- `linux-x86_64`

The intended path is prebuilt-first:

- `xtask` prefers an already materialized bundle under `third_party/runtime/<platform>/`
- when a bundle is missing, `xtask` rebuilds that slice from `third_party/upstream/libkrun`
- on macOS, that fallback may use `zig cc` for the embedded Linux init binary so a missing secondary slice can still be rebuilt locally
