# sagens

`sagens` is a local-first microVM sandbox for agent workloads.

It is intentionally not container-first. Each BOX runs inside a fresh `libkrun`
microVM with:

- a guest agent over `vsock`
- a read-only Alpine root filesystem
- a persistent block-backed workspace
- a single host binary for CLI, daemon, and internal runner duties

## What Is Stable

The supported product surface is:

- the `sagens` CLI
- the `box_api` WebSocket protocol

The Rust library is internal-first. It exists to support the shipped binary,
tests, and embedding flows, and should not be treated as a stable semver API.

## Core Model

- A `BOX` is the durable workspace identity exposed to the user.
- Runtime sessions are ephemeral microVMs created on demand.
- Filesystem, exec, and shell actions can boot a BOX automatically.
- Checkpoints are product-level recovery points for a BOX workspace:
  - `checkpoint_create`
  - `checkpoint_list`
  - `checkpoint_restore`
  - `checkpoint_fork`
  - `checkpoint_delete`
- `checkpoint_fork` copies the selected snapshot into a new BOX. The forked BOX
  starts a fresh checkpoint lineage.

## Supported Platforms

- macOS `aarch64`
- macOS `x86_64`
- Linux `aarch64`
- Linux `x86_64`

GitHub Releases published from version tags are the supported distribution
channel for end users in the first public OSS iteration.

## Quickstart

Build locally:

```bash
./build-local.sh
./dist/sagens-local-<platform> start
./dist/sagens-local-<platform> box new
```

Run the full local quality and standalone-binary verification path:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo doc --no-deps
cargo test
bash scripts/e2e-standalone.sh ./dist/sagens-local-<platform>
bash scripts/e2e-checkpoint.sh ./dist/sagens-local-<platform>
```

Build a standalone local artifact:

```bash
./build-local.sh
```

`build-local.sh` detects the current host and produces one local binary for
that exact platform under `dist/sagens-local-<platform>`.

## Security Model

`sagens` currently targets a trusted local-host model:

- loopback-only host API by default
- microVM isolation via `libkrun`
- CPU and memory limits enforced through `libkrun`
- process count enforced in-guest as a best effort
- network disable is intentionally minimal and best effort

It does not currently claim hostile multi-tenant isolation or remote service
hardening guarantees.

## Known Limits

- cold start is acceptable today; warm-pool and snapshot-boot improvements are
  roadmap items, not product guarantees
- only one exec may run at a time inside a single BOX
- checkpoint storage is product-facing today and storage-backend-agnostic; ZFS
  mapping is future work rather than a current requirement
- GitHub Releases are supported; crates.io publishing is intentionally out of
  scope for the first OSS release

## Documentation

- [Docs Index](docs/DOCS.md)
- [Usage](docs/USAGE.md)
- [WebSocket Protocol](docs/PROTOCOL.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Build And Packaging](docs/BUILD.md)
- [CI And Releases](docs/CI.md)
- [Project Status](STATUS.md)
- [Roadmap](ROADMAP.md)
- [Contributing](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)
- [Code Of Conduct](CODE_OF_CONDUCT.md)
- [Notice](NOTICE)
- [Third-Party Licenses](THIRD_PARTY_LICENSES.md)

## License

`sagens` is licensed under the [Apache License 2.0](LICENSE).
Bundled runtime and release artifacts may include third-party components under
additional terms; see [NOTICE](NOTICE) and
[Third-Party Licenses](THIRD_PARTY_LICENSES.md).
