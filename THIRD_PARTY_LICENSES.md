# Third-Party Licenses

This document summarizes the major third-party components used by `sagens`.
It is informational only and does not replace the upstream license texts.

## First-party code

- `sagens` first-party source is licensed under Apache-2.0.
- See [LICENSE](LICENSE) for the full text.

## Vendored upstream components

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

### `third_party/upstream/linux-loader`

- Upstream project: `linux-loader`
- Repository path: `third_party/upstream/linux-loader`
- License: Apache-2.0 AND BSD-3-Clause
- Source of truth: upstream crate license files in that directory

## Generated guest artifacts

`sagens` packaging can embed generated guest assets into standalone release
artifacts. Those payloads may include materials derived from:

- statically linked `libkrun`
- `libkrunfw`
- Alpine guest image inputs fetched during the guest build path

The tracked upstream source trees under `third_party/upstream/` are the
provenance source of truth for these embedded payloads.

## Release artifact composition

Standalone `sagens` binaries may include:

- first-party `sagens` host code
- statically linked `libkrun`
- embedded guest kernel or root filesystem assets required by the runtime path
- macOS firmware embedded from the vendored `libkrun` checkout when needed

If you redistribute release artifacts, review the upstream licenses above and
carry forward any notices or corresponding-source obligations that apply to the
bundled payloads.
