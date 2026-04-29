# Support matrix

## Why this matters

If you are evaluating `sagens`, one of the first questions is whether the host
you want to run on is part of the supported path, and what runtime stack sits
under each BOX on that host.

This page is the explicit support contract for the current libkrun-only
backend.

## Host and SDK matrix

| Host OS | CPU | CLI / host binary | Python SDK | Node SDK | Notes |
| --- | --- | --- | --- | --- | --- |
| macOS | arm64 (Apple Silicon) | Supported | Supported on Python `3.11+` | Supported on Node `20+` | Uses the macOS `arm64` platform package / wheel path |
| Linux | x86_64 | Supported | Supported on Python `3.11+` | Supported on Node `20+` | Full microVM runtime requires `/dev/kvm` |
| Linux | arm64 / aarch64 | Supported | Supported on Python `3.11+` | Supported on Node `20+` | Full microVM runtime requires `/dev/kvm` |

Not supported by the current backend:

- Windows
- macOS `x86_64`

## Runtime version policy

### Python

- The package metadata requires `Python >=3.11`.
- The Rust extension is built with `pyo3` `abi3-py311`, so the published wheel
  targets the stable Python ABI starting at Python `3.11`.
- The package classifiers explicitly list Python `3.11`, `3.12`, and `3.13`.

### Node

- The Node package declares `"engines": { "node": ">=20" }`.
- CI currently exercises the packaged Node flow on Node `22`.

## microVM runtime by host

### macOS arm64

Under the hood, `sagens` uses:

- vendored `libkrun` as the in-process microVM runtime library
- Apple's Hypervisor Framework (`HVF`) through `libkrun`
- the bundled `KRUN_EFI.silent.fd` firmware from
  `third_party/upstream/libkrun/edk2`
- the repo-managed AArch64 guest kernel and rootfs artifacts

Practical notes:

- This is the only supported macOS host path.
- The current libkrun-only backend does not support macOS `x86_64`.

### Linux x86_64

Under the hood, `sagens` uses:

- vendored `libkrun` as the microVM runtime library
- the Linux `KVM` backend exposed through `/dev/kvm`
- a guest kernel materialized from the prebuilt `libkrunfw-x86_64` bundle
- the pinned local `linux-loader` override in `third_party/upstream/linux-loader`
  so the Linux build uses the same `vm-memory` ABI as `libkrun`

Practical notes:

- This is the main full-e2e path exercised in CI.
- The Linux runtime path needs `/dev/kvm` for real microVM execution.
- No separate firmware layer is used on this path.

### Linux arm64 / aarch64

Under the hood, `sagens` uses:

- vendored `libkrun` as the microVM runtime library
- the Linux `KVM` backend exposed through `/dev/kvm`
- a guest kernel materialized from the prebuilt `libkrunfw-aarch64` bundle
- the pinned local `linux-loader` override in `third_party/upstream/linux-loader`
  so the Linux build uses the same `vm-memory` ABI as `libkrun`

Practical notes:

- The Linux runtime path needs `/dev/kvm` for real microVM execution.
- No separate firmware layer is used on this path.

## Packaging notes

- The standalone host binary links vendored `libkrun` at build time; the
  packaged CLI, Python wheel, and Node platform packages do not expect a
  system-installed `libkrun`.
- The Node package publishes only these platform packages:
  `@xenage/sanges-darwin-arm64`, `@xenage/sanges-linux-x64`, and
  `@xenage/sanges-linux-arm64`.
- The Python release workflow builds wheels for the same three host targets.
