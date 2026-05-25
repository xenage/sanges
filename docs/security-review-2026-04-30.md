# Security Review: BOX Isolation

Date: 2026-04-30

## Goal

This review answers a strict question:

"Can code running in one BOX harm the host device?"

Short answer:

- On Linux `secure` mode, the project is on the right path and already has meaningful isolation.
- On macOS or `compat` mode, the answer is no: this must not be treated as host-safe isolation.
- Even on Linux `secure` mode, the current implementation still allows host harm through resource exhaustion paths, and the microVM stack remains part of the trusted computing base.

## Threat model

We care about four classes of harm:

1. Escaping the BOX and touching host files, sockets, processes, or other BOXes.
2. Consuming enough host CPU, memory, PIDs, disk, or I/O to degrade or break the device.
3. Reaching host or external networks through hidden channels.
4. Exploiting the VMM or host kernel layer behind the BOX runtime.

## What the project already gets right

### 1. Secure mode is fail-closed

- Packaged hosts do not silently fall back from `secure` to `compat`.
- `compat` is explicitly marked insecure and requires opt-in.
- macOS is explicitly documented as dev/test only.

Evidence:

- `README.md`
- `src/sagens/config.rs`

### 2. The host-side runner is sandboxed before libkrun starts

In Linux `secure` mode the helper process applies:

- `PR_SET_NO_NEW_PRIVS`
- `PR_SET_DUMPABLE = 0`
- `PR_SET_MDWE` when supported
- user, mount, network, IPC, UTS, cgroup, and PID namespaces
- `chroot`
- private `/tmp`
- read-only `/proc`
- Landlock
- seccomp

Evidence:

- `src/backend/libkrun/runner/secure_runner/bootstrap.rs`
- `src/backend/libkrun/runner/secure_runner/policies.rs`
- `src/backend/libkrun/runner/secure_runner/harness.rs`

### 3. Secure mode intentionally avoids the dangerous libkrun host-sharing paths

The project explicitly rejects the `init.krun` + `virtiofs` root path in secure mode and disables libkrun's implicit vsock/TSI path before adding only the explicit RPC bridge.

Evidence:

- `src/backend/libkrun/config.rs`
- `src/backend/libkrun/loader.rs`

### 4. BOX-scoped auth is real, not fake

BOX credentials are restricted to one BOX. Lifecycle and daemon-wide control remain admin-only.

Evidence:

- `src/box_api/server/dispatch/access.rs`
- `src/box_api/server/checkpoints.rs`
- `crates/sagens-host/tests/box_api_auth.rs`

### 5. Basic host resource controls already exist

Secure mode attaches the runner to a delegated cgroup and enforces:

- `memory.max`
- `cpu.max`
- `pids.max`

Evidence:

- `src/host_hardening.rs`

## Findings

### P0: BOX-scoped clients can exhaust host disk through unlimited checkpoints

This is the most concrete current violation of the "cannot harm the device" goal.

Why:

- BOX-scoped auth is enough to call checkpoint create and delete.
- Creating a checkpoint copies the whole `workspace.raw` image into checkpoint storage.
- There is no quota on checkpoint count, total checkpoint bytes, or checkpoint creation rate.

Impact:

- A malicious or buggy agent with BOX-scoped access can repeatedly create checkpoints until the host disk fills.
- This bypasses the workspace disk size as a practical host protection because the checkpoint store grows outside the guest filesystem quota.

Evidence:

- `src/box_api/server/checkpoints.rs`: BOX auth is enough for create/list/restore/delete.
- `src/workspace/checkpoints.rs`: checkpoint creation is always accepted.
- `src/workspace/lineage.rs`: checkpoint creation copies the whole workspace image.

What to change:

1. Add hard quotas for checkpoint count per BOX.
2. Add hard quotas for total checkpoint bytes per BOX.
3. Add rate limiting for checkpoint creation.
4. Consider making checkpoint creation admin-only unless explicitly enabled for BOX-scoped workers.

### P0: Exec capture APIs accumulate unbounded stdout/stderr in memory

The default captured-exec path is currently unsafe against memory DoS.

Why:

- `CommandStream::collect()` appends output forever.
- The Rust client capture path appends output forever.
- The Python client appends output forever.
- The Node client appends output forever.

Impact:

- A command such as `yes`, `cat /dev/zero`, or an accidental log storm can grow host memory until the calling process becomes unstable or is killed.
- This is especially risky because the public SDK APIs default to returning a fully buffered `CompletedExecution`.

Evidence:

- `src/protocol.rs`
- `src/box_api/client/exec.rs`
- `python/sagens/_client.py`
- `node/src/client.ts`

What to change:

1. Introduce a hard max captured-output size.
2. Stop or kill the underlying exec when the cap is reached.
3. Return explicit truncation metadata.
4. Keep streaming shells as the unbounded-output interface and make buffered exec intentionally bounded.

### P1: Host hardening cgroups do not limit I/O or checkpoint-driven storage growth

Current cgroup hardening only writes `memory.max`, `cpu.max`, and `pids.max`.

Impact:

- A BOX can still create intense disk pressure through workspace churn.
- Checkpoint creation happens outside any explicit checkpoint-storage quota.
- Even if CPU and memory are bounded, the host can still be made unpleasant or unstable through storage pressure and I/O saturation.

Evidence:

- `src/host_hardening.rs`

What to change:

1. If the delegated parent exposes the `io` controller, configure `io.max` or equivalent host policy.
2. Add explicit checkpoint-store quotas outside cgroup accounting.
3. Consider `memory.swap.max = 0` or another clear swap policy if host swap thrash is a concern.

### P1: The daemon and VMM are still part of the trusted computing base

This is not a bug in the repo, but it is an architectural limit that must be stated plainly.

The upstream libkrun security model says the guest and the VMM should be treated as one security context, and host resources accessible to the VMM may be reachable to the guest through it. That means:

- the secure runner sandbox matters a lot,
- the daemon process matters a lot,
- `/dev/kvm`, libkrun, and the host kernel remain high-value attack surfaces.

Impact:

- "Impossible to harm the device" is too strong if the BOX runtime shares the same physical host as the user's real workstation.
- A VM escape in KVM/libkrun or a host-side daemon bug remains a host-compromise path.

What to change:

1. Treat Linux `secure` mode as necessary but not sufficient for workstation-grade safety.
2. Run the daemon on a dedicated worker host or inside a dedicated outer VM, not on the user's main laptop.
3. Run the daemon under a dedicated non-login service account with the minimum access needed for state directories and `/dev/kvm`.

### P1: macOS and compat mode must stay outside the security promise

The repo is already honest about this. The important conclusion is operational:

- if the goal is "the box must not be able to harm the device",
- then macOS and `compat` mode are out of scope.

Evidence:

- `README.md`
- `src/sagens/config.rs`

## Recommended definition of "host safe"

The project should only claim host-safe isolation when all of the following are true:

1. Host OS is Linux.
2. Isolation mode is `secure`.
3. Secure preflight passes.
4. Guest networking is disabled.
5. Checkpoint quota and captured-output quota are enforced.
6. The daemon runs under a dedicated service account.
7. The daemon runs on a dedicated worker host or inside a dedicated outer VM, not on the user's primary workstation.

Without item 7, the project can reasonably claim "strong isolation with residual VMM/kernel risk", but not "cannot harm the device".

## Recommended backlog

### Immediate

1. Add checkpoint count and byte quotas.
2. Add a hard captured-output limit with truncation metadata.
3. Add tests for both quotas.
4. Document that only Linux `secure` mode is within the security boundary.

### Next

1. Add I/O controller support when available.
2. Add swap policy for the secure runner cgroup.
3. Add rate limiting for checkpoint creation.
4. Add per-BOX accounting and observability for checkpoint bytes and exec output bytes.

### Operational

1. Run `sagens` only on hardened Linux workers.
2. Keep host kernel, libkrun, and bundled guest assets patched.
3. Keep long-lived secrets off the same worker wherever possible.
4. Treat the daemon and secure runner as privileged components in threat modeling and incident response.

## External references

- libkrun security model:
  - https://github.com/containers/libkrun
- Linux Landlock docs:
  - https://kernel.org/doc/html/v6.0/security/landlock.html
  - https://cdn.kernel.org/doc/html/latest/userspace-api/landlock.html
- Linux cgroup v2 docs:
  - https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html

## Bottom line

The current Linux `secure` design is directionally good and significantly better than a shared shell or plain container on a developer laptop. But today it does not yet satisfy the stricter promise that a BOX cannot harm the device.

To get close to that bar, the project needs:

- Linux-only secure deployment,
- explicit disk and output quotas,
- stronger host resource controls,
- and deployment on dedicated worker infrastructure instead of the user's main machine.
