# Support matrix

## Why this matters

If you are evaluating `sagens`, one of the first questions is whether the host
you want to run on is part of the supported path, and what runtime stack sits
under each BOX on that host.

This page is the explicit support contract for the current secure host runtime.

## Host and SDK matrix

| Host OS | CPU | CLI / host binary | Python SDK | Node SDK | Notes |
| --- | --- | --- | --- | --- | --- |
| Linux | x86_64 | Supported | Supported on Python `3.11+` | Supported on Node `20+` | Secure mode requires `/dev/kvm`, delegated cgroup access, and Linux Landlock |
| Linux | arm64 / aarch64 | Supported | Supported on Python `3.11+` | Supported on Node `20+` | Secure mode requires `/dev/kvm`, delegated cgroup access, and Linux Landlock |
| macOS | arm64 (Apple Silicon) | Dev/test only | Dev/test only | Dev/test only | Requires explicit insecure compat opt-in; secure host isolation is not shipped |

Not supported by the current secure backend:

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

- This host path is not part of the shipped secure runtime.
- Running here requires explicit insecure compatibility opt-in.
- The current backend does not support macOS `x86_64`.

### Linux x86_64

Under the hood, `sagens` uses:

- vendored `libkrun` as the microVM runtime library
- the Linux `KVM` backend exposed through `/dev/kvm`
- a direct-boot Alpine guest kernel extracted from the pinned `linux-virt` package
- the pinned local `linux-loader` override in `third_party/upstream/linux-loader`
  so the Linux build uses the same `vm-memory` ABI as `libkrun`
- a secure runner sandbox with Linux namespaces, chroot, Landlock, and seccomp

Practical notes:

- This is the main full-e2e path exercised in CI.
- The Linux runtime path needs `/dev/kvm` for real microVM execution.
- `secure` mode requires delegated cgroup access through `SAGENS_CGROUP_PARENT`.
- `secure` startup runs a built-in runner harness and fails closed if the helper sandbox cannot establish namespaces, chroot, Landlock, and seccomp on the current host.
- No separate firmware layer is used on this path.

### Linux arm64 / aarch64

Under the hood, `sagens` uses:

- vendored `libkrun` as the microVM runtime library
- the Linux `KVM` backend exposed through `/dev/kvm`
- a direct-boot Alpine guest kernel extracted from the pinned `linux-virt` package
- the pinned local `linux-loader` override in `third_party/upstream/linux-loader`
  so the Linux build uses the same `vm-memory` ABI as `libkrun`
- a secure runner sandbox with Linux namespaces, chroot, Landlock, and seccomp

Practical notes:

- The Linux runtime path needs `/dev/kvm` for real microVM execution.
- `secure` mode requires delegated cgroup access through `SAGENS_CGROUP_PARENT`.
- `secure` startup runs a built-in runner harness and fails closed if the helper sandbox cannot establish namespaces, chroot, Landlock, and seccomp on the current host.
- No separate firmware layer is used on this path.

## Packaging notes

- The standalone host binary links vendored `libkrun` at build time; the
  packaged CLI, Python wheel, and Node platform packages do not expect a
  system-installed `libkrun`.
- The Node package publishes only these platform packages:
  `@xenage/sanges-darwin-arm64`, `@xenage/sanges-linux-x64`, and
  `@xenage/sanges-linux-arm64`.
- The Python release workflow builds wheels for the same three host targets.
