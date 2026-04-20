# Third-Party Licenses

This document summarizes the major third-party components used by `sagens`.
It is informational only and does not replace the upstream license texts.

## First-party code

- `sagens` first-party source is licensed under Apache-2.0.
- See [LICENSE](LICENSE) for the full text.

## Vendored upstream components

### `third_party/upstream/krunkit`

- Upstream project: `krunkit`
- Repository path: `third_party/upstream/krunkit`
- License: Apache-2.0
- Source of truth: upstream license file in that directory

### `third_party/upstream/libkrun`

- Upstream project: `libkrun`
- Repository path: `third_party/upstream/libkrun`
- License: Apache-2.0
- Source of truth: upstream license file in that directory

### `third_party/upstream/libkrunfw`

- Upstream project: `libkrunfw`
- Repository path: `third_party/upstream/libkrunfw`
- License composition as documented by upstream:
  - Linux kernel: GPL-2.0-only
  - patch files under `patches/`: GPL-2.0-only
  - library code, including generated code: LGPL-2.1-only
- Source of truth: upstream README plus `LICENSE-GPL-2.0-only` and `LICENSE-LGPL-2.1-only`

Upstream `libkrunfw` documents that binary distributions of that library must be
accompanied by the corresponding source for the bundled kernel and library
code. This repository keeps the upstream source tree under
`third_party/upstream/libkrunfw` for provenance and rebuild workflows.

## Generated runtime and guest artifacts

`sagens` packaging can embed generated runtime and guest assets into release
artifacts. Those payloads may include materials derived from:

- `libkrun`
- `libkrunfw`
- Alpine guest image inputs fetched during the guest build path

The generated bundle directories under `third_party/runtime/<platform>/` are
local build outputs. They are not the provenance source of truth; use the
tracked upstream source trees under `third_party/upstream/` plus the build
pipeline documentation in [docs/BUILD.md](docs/BUILD.md).

## Release artifact composition

Standalone `sagens` binaries may include:

- first-party `sagens` host code
- embedded `libkrun` runtime components
- firmware or support libraries materialized from the runtime bundle
- generated guest kernel or root filesystem assets required by the runtime path

If you redistribute release artifacts, review the upstream licenses above and
carry forward any notices or corresponding-source obligations that apply to the
bundled payloads.
