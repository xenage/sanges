#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST_DIR="$ROOT/dist"
STATE_DIR="$ROOT/.sagens-state"

SCRIPT_START=$SECONDS
TOTAL_STEPS=7
CURRENT_STEP=0
CURRENT_STEP_NAME=""
CURRENT_STEP_START=0

TEMP_ROOT="${BUILD_LOCAL_TMPDIR:-}"
TEMP_ROOT_OWNED=0
STATE_DIR_PREEXISTED=0

PLATFORM=""
EXPECTED_BINARY=""
PACKAGE_VERSION="local"
PACKAGE_ARGS=()
FORCE_SIGN=0

if [[ -t 1 && -z "${NO_COLOR:-}" && "${TERM:-}" != "dumb" ]]; then
  COLOR_RESET=$'\033[0m'
  COLOR_STEP=$'\033[1;34m'
  COLOR_META=$'\033[2m'
  COLOR_OK=$'\033[1;32m'
  COLOR_WARN=$'\033[1;33m'
  COLOR_FAIL=$'\033[1;31m'
else
  COLOR_RESET=""
  COLOR_STEP=""
  COLOR_META=""
  COLOR_OK=""
  COLOR_WARN=""
  COLOR_FAIL=""
fi

host_platform() {
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64)
      printf 'macos-aarch64\n'
      ;;
    Darwin:x86_64)
      printf 'macos-x86_64\n'
      ;;
    Linux:aarch64 | Linux:arm64)
      printf 'linux-aarch64\n'
      ;;
    Linux:x86_64 | Linux:amd64)
      printf 'linux-x86_64\n'
      ;;
    *)
      printf 'unsupported host platform: %s/%s\n' "$(uname -s)" "$(uname -m)" >&2
      return 1
      ;;
  esac
}

format_duration() {
  local total_seconds="$1"
  local hours=$((total_seconds / 3600))
  local minutes=$(((total_seconds % 3600) / 60))
  local seconds=$((total_seconds % 60))

  if ((hours > 0)); then
    printf '%02dh:%02dm:%02ds' "$hours" "$minutes" "$seconds"
    return
  fi
  if ((minutes > 0)); then
    printf '%02dm:%02ds' "$minutes" "$seconds"
    return
  fi
  printf '%02ds' "$seconds"
}

print_line() {
  local stream="$1"
  local color="$2"
  local message="$3"

  if [[ "$stream" == "stderr" ]]; then
    printf '%b%s%b\n' "$color" "$message" "$COLOR_RESET" >&2
    return
  fi
  printf '%b%s%b\n' "$color" "$message" "$COLOR_RESET"
}

log_meta() {
  print_line stdout "$COLOR_META" "      $1"
}

log_warn() {
  print_line stderr "$COLOR_WARN" "warning: $1"
}

log_error() {
  print_line stderr "$COLOR_FAIL" "error: $1"
}

format_command() {
  local parts=()
  local arg

  for arg in "$@"; do
    parts+=("$(printf '%q' "$arg")")
  done

  if [[ ${#parts[@]} -eq 0 ]]; then
    return
  fi

  local joined
  joined="$(printf '%s ' "${parts[@]}")"
  printf '%s' "${joined% }"
}

begin_step() {
  CURRENT_STEP=$((CURRENT_STEP + 1))
  CURRENT_STEP_NAME="$1"
  CURRENT_STEP_START=$SECONDS

  local elapsed_total
  local steps_left
  elapsed_total="$(format_duration "$((SECONDS - SCRIPT_START))")"
  steps_left=$((TOTAL_STEPS - CURRENT_STEP))

  printf '\n'
  print_line stdout "$COLOR_STEP" "[$CURRENT_STEP/$TOTAL_STEPS] $CURRENT_STEP_NAME ($elapsed_total elapsed, $steps_left left)"
}

finish_step() {
  local step_elapsed
  step_elapsed="$(format_duration "$((SECONDS - CURRENT_STEP_START))")"
  print_line stdout "$COLOR_OK" "      done in $step_elapsed"
}

fail_step() {
  local exit_code="$1"
  local step_elapsed
  local total_elapsed
  step_elapsed="$(format_duration "$((SECONDS - CURRENT_STEP_START))")"
  total_elapsed="$(format_duration "$((SECONDS - SCRIPT_START))")"
  log_error "step failed: $CURRENT_STEP_NAME (exit $exit_code, step $step_elapsed, total $total_elapsed)"
}

run_step() {
  local name="$1"
  shift

  begin_step "$name"
  local exit_code=0
  "$@" || exit_code=$?
  if [[ "$exit_code" -eq 0 ]]; then
    finish_step
    return 0
  fi

  fail_step "$exit_code"
  return "$exit_code"
}

cleanup() {
  local exit_code="$1"

  if [[ "$TEMP_ROOT_OWNED" -eq 1 && -d "$TEMP_ROOT" ]]; then
    rm -rf "$TEMP_ROOT"
  fi

  if [[ "$STATE_DIR_PREEXISTED" -eq 0 && -d "$STATE_DIR" ]]; then
    rmdir "$STATE_DIR" 2>/dev/null || true
  fi

  trap - EXIT
  exit "$exit_code"
}

trap 'cleanup "$?"' EXIT

print_banner() {
  print_line stdout "$COLOR_STEP" "==> Local package build"
  log_meta "root: $ROOT"
  log_meta "dist: $DIST_DIR"
}

detect_platform() {
  PLATFORM="$(host_platform)"

  log_meta "host platform: $PLATFORM"
}

prepare_build_workspace() {
  if [[ -z "$TEMP_ROOT" ]]; then
    TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/sagens-local-build.XXXXXX")"
    TEMP_ROOT_OWNED=1
    log_meta "temp root: $TEMP_ROOT (managed)"
  else
    mkdir -p "$TEMP_ROOT"
    log_meta "temp root: $TEMP_ROOT (user supplied)"
  fi

  if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
    export CARGO_TARGET_DIR="$TEMP_ROOT/target"
    log_meta "cargo target dir: $CARGO_TARGET_DIR"
  else
    log_meta "cargo target dir: $CARGO_TARGET_DIR (preconfigured)"
  fi

  if [[ -z "${SAGENS_GUEST_OUTPUT_DIR:-}" ]]; then
    export SAGENS_GUEST_OUTPUT_DIR="$TEMP_ROOT/guest-output"
    log_meta "guest output dir: $SAGENS_GUEST_OUTPUT_DIR"
  else
    log_meta "guest output dir: $SAGENS_GUEST_OUTPUT_DIR (preconfigured)"
  fi

  if [[ -z "${SAGENS_GUEST_WORK_DIR:-}" ]]; then
    export SAGENS_GUEST_WORK_DIR="$TEMP_ROOT/guest-work"
    log_meta "guest work dir: $SAGENS_GUEST_WORK_DIR"
  else
    log_meta "guest work dir: $SAGENS_GUEST_WORK_DIR (preconfigured)"
  fi

  if [[ -e "$STATE_DIR" ]]; then
    STATE_DIR_PREEXISTED=1
    log_meta "state dir already exists: $STATE_DIR"
  else
    log_meta "state dir will be cleaned if created: $STATE_DIR"
  fi
}

sync_submodules() {
  if [[ ! -e "$ROOT/.git" ]]; then
    log_meta "skipping submodule sync outside git checkout"
    return 0
  fi

  log_meta "refreshing required upstream submodules"
  if ! git -C "$ROOT" submodule update --init --recursive \
    third_party/upstream/libkrun; then
    log_warn "failed to initialize upstream submodules; continuing with existing checkout"
  fi
}

prepare_dist_dir() {
  mkdir -p "$DIST_DIR"
  find "$DIST_DIR" -maxdepth 1 -type f \
    \( \
      -name "sagens-*-$PLATFORM" -o \
      -name "sagens-*-$PLATFORM.sha256" -o \
      -name "sagens-*-$PLATFORM.pkg" -o \
      -name "sagens-*-$PLATFORM.pkg.sha256" -o \
      -name "sagens-*-$PLATFORM.zip" -o \
      -name "sagens-*-$PLATFORM.zip.sha256" \
    \) \
    -delete
  log_meta "cleared stale local artifacts in $DIST_DIR"
}

resolve_package_args() {
  PACKAGE_ARGS=()

  local has_version=0
  local expect_version_value=0
  local arg
  for arg in "$@"; do
    if [[ "$expect_version_value" -eq 1 ]]; then
      PACKAGE_VERSION="$arg"
      PACKAGE_ARGS+=("$arg")
      expect_version_value=0
      continue
    fi
    case "$arg" in
      --sign)
        FORCE_SIGN=1
        ;;
      --version)
        has_version=1
        expect_version_value=1
        PACKAGE_ARGS+=("$arg")
        ;;
      *)
        PACKAGE_ARGS+=("$arg")
        ;;
    esac
  done

  if [[ "$expect_version_value" -eq 1 ]]; then
    log_error "--version requires a value"
    return 1
  fi

  if [[ "$has_version" -eq 0 ]]; then
    PACKAGE_VERSION="local"
    if [[ ${#PACKAGE_ARGS[@]} -gt 0 ]]; then
      PACKAGE_ARGS=(--version "$PACKAGE_VERSION" "${PACKAGE_ARGS[@]}")
    else
      PACKAGE_ARGS=(--version "$PACKAGE_VERSION")
    fi
  fi

  if [[ "$FORCE_SIGN" -eq 1 ]]; then
    export SAGENS_FORCE_SIGN=1
    log_meta "release signing requested via --sign"
  fi

  log_meta "artifact version: $PACKAGE_VERSION"
  log_meta "xtask package args: $(format_command "${PACKAGE_ARGS[@]}")"
}

package_local_build() {
  log_meta "command: $(format_command cargo run --bin xtask --manifest-path "$ROOT/Cargo.toml" -- package "${PACKAGE_ARGS[@]}")"
  cargo run --bin xtask --manifest-path "$ROOT/Cargo.toml" -- package "${PACKAGE_ARGS[@]}"
}

finalize_dist_artifacts() {
  EXPECTED_BINARY="$DIST_DIR/sagens-$PACKAGE_VERSION-$PLATFORM"

  if [[ -f "$EXPECTED_BINARY" ]]; then
    log_meta "primary artifact: $EXPECTED_BINARY"
  else
    log_warn "expected artifact not found: $EXPECTED_BINARY"
  fi

  if [[ -f "$EXPECTED_BINARY.sha256" ]]; then
    log_meta "checksum: $EXPECTED_BINARY.sha256"
  fi
}

print_summary() {
  local total_elapsed
  total_elapsed="$(format_duration "$((SECONDS - SCRIPT_START))")"

  printf '\n'
  print_line stdout "$COLOR_OK" "Build complete in $total_elapsed"
  if [[ -n "$EXPECTED_BINARY" ]]; then
    log_meta "binary: $EXPECTED_BINARY"
  fi
  if [[ -f "$EXPECTED_BINARY.sha256" ]]; then
    log_meta "checksum: $EXPECTED_BINARY.sha256"
  fi
}

print_banner
run_step "Detect host platform" detect_platform
run_step "Prepare build workspace" prepare_build_workspace
run_step "Sync upstream submodules" sync_submodules
run_step "Clean dist directory" prepare_dist_dir
run_step "Resolve package arguments" resolve_package_args "$@"
run_step "Package local build" package_local_build
run_step "Finalize dist artifacts" finalize_dist_artifacts
print_summary
