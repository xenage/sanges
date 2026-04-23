Code Quality:

Each file MUST NOT exceed 400 lines
Highly modular, production-grade design
Minimal dependencies
Explicit interfaces and strong error handling
No global mutable state
Strict typing
Minimum try/except
Completly remove deprecated code / unused code, any feature must remove old deprecated code 
Fully remove legacy code
Do not create fallbacks and remove old legacy fallbacks

Runtime Direction:

libkrun-first and libkrun-only
single host binary is the product surface for daemon, microVM runner, and CLI
link vendored libkrun at build time; do not ship or depend on runtime libkrun bundles, sidecar dylibs, or qemu compatibility paths
no docker in build or test flows
no required runtime asset downloads, sidecar daemons, or external bundle assembly for end users
guest-agent-driven execution over vsock
no compatibility layers for removed backends
local and CI macOS signing should flow through the same xtask-driven env contract (`.env` locally, GitHub secrets in Actions)
