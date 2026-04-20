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

Runtime Direction:

libkrun-first and libkrun-only
single host binary is the product surface for daemon, microVM runner, and CLI
prefer prebuilt libkrun bundles for packaging; source build is fallback/refresh path
no docker in build or test flows
no required runtime asset downloads, sidecar daemons, or external bundle assembly for end users
guest-agent-driven execution over vsock
no compatibility layers for removed backends
local and CI macOS signing should flow through the same xtask-driven env contract (`.env` locally, GitHub secrets in Actions)
