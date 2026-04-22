#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" == "Linux" ]]; then
  exec cc "$@"
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
arch="$(uname -m)"

case "$arch" in
  arm64 | aarch64)
    zig_target="aarch64-linux-musl"
    clang_target="aarch64-linux-gnu"
    ;;
  x86_64 | amd64)
    zig_target="x86_64-linux-musl"
    clang_target="x86_64-linux-gnu"
    ;;
  *)
    echo "unsupported host architecture for CC_LINUX shim: $arch" >&2
    exit 1
    ;;
esac

if command -v zig >/dev/null 2>&1; then
  exec zig cc -target "$zig_target" "$@"
fi

if command -v xcrun >/dev/null 2>&1; then
  clang="$(xcrun --find clang)"
else
  clang="$(command -v clang)"
fi

if [[ -z "${clang:-}" || ! -x "$clang" ]]; then
  echo "clang is required to build Linux init.krun on macOS" >&2
  exit 1
fi

rustc_path="$(rustup which rustc)"
toolchain_root="$(cd "$(dirname "$rustc_path")/.." && pwd)"
lld_path="$(find "$toolchain_root/lib/rustlib" -path '*/bin/gcc-ld/ld.lld' -print -quit)"
if [[ -z "${lld_path:-}" || ! -x "$lld_path" ]]; then
  echo "ld.lld is required to build Linux init.krun on macOS" >&2
  exit 1
fi

export PATH="$(dirname "$lld_path"):$PATH"
sysroot="$repo_root/third_party/upstream/libkrun/linux-sysroot"
gcc_lib_dir="$sysroot/usr/lib/gcc/$clang_target/12"

exec "$clang" \
  -target "$clang_target" \
  -fuse-ld=lld \
  --sysroot "$sysroot" \
  -B"$gcc_lib_dir" \
  -L"$gcc_lib_dir" \
  -Wno-c23-extensions \
  "$@"
