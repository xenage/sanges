# third_party

This directory is the project-local home for non-Rust runtime inputs used by `sagens`.

## Layout

- `upstream/`
  - source submodules for upstream projects used for provenance and rebuild workflows

## Upstream source

The current standalone build flow requires:

- `third_party/upstream/libkrun`
- `third_party/upstream/libkrunfw`
- `third_party/upstream/linux-loader` as a pinned local override so `libkrun` and `linux-loader`
  resolve against the same `vm-memory` ABI on Linux builders

`xtask` initializes the submodule-backed upstream checkouts on demand when Git metadata is
available.
The standalone host binary links vendored `libkrun` at build time and embeds guest assets
directly from repository-managed inputs.

## Guest artifacts

Guest images remain under `artifacts/`, for example:

- `artifacts/alpine-aarch64/rootfs.raw`
- `artifacts/alpine-aarch64/vmlinuz-virt`

Those are the project-local inputs used by the dev build unless explicitly overridden.
