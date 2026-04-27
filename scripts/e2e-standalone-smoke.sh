#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 /absolute/path/to/sagens" >&2
  exit 2
fi

BIN="$1"
if [[ ! -x "$BIN" ]]; then
  echo "standalone binary is missing or not executable: $BIN" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ENTITLEMENTS="$REPO_ROOT/macos/sagens.entitlements"
source "$SCRIPT_DIR/e2e-logging.sh"

ROOT_TMP="$(mktemp -d "${TMPDIR:-/tmp}/sagens-standalone-smoke.XXXXXX")"
STATE_DIR="$ROOT_TMP/state"
CONFIG_DIR="$ROOT_TMP/config"
RUN_BIN="$ROOT_TMP/sagens"
CONFIG_JSON="$CONFIG_DIR/config.json"
SECOND_CONFIG_JSON="$CONFIG_DIR/second.json"

mkdir -p "$STATE_DIR" "$CONFIG_DIR"
cp "$BIN" "$RUN_BIN"
chmod +x "$RUN_BIN"

if [[ "$(uname -s)" == "Darwin" ]]; then
  if [[ ! -f "$ENTITLEMENTS" ]]; then
    echo "missing macOS entitlements file: $ENTITLEMENTS" >&2
    exit 1
  fi
  codesign --force --sign - --entitlements "$ENTITLEMENTS" --timestamp=none "$RUN_BIN" >/dev/null
fi

cleanup() {
  local status="${1:-0}"
  if [[ "$status" -ne 0 ]]; then
    e2e_log_meta "standalone smoke state dir: $STATE_DIR"
  fi
  if [[ "$status" -ne 0 && -f "$STATE_DIR/daemon.log" ]]; then
    echo "daemon log:" >&2
    cat "$STATE_DIR/daemon.log" >&2 || true
  fi
  if [[ -f "$STATE_DIR/daemon.pid" ]]; then
    local daemon_pid
    daemon_pid="$(tr -d '[:space:]' < "$STATE_DIR/daemon.pid" || true)"
    if [[ -n "$daemon_pid" ]] && kill -0 "$daemon_pid" 2>/dev/null; then
      kill "$daemon_pid" 2>/dev/null || true
      wait "$daemon_pid" 2>/dev/null || true
    fi
  fi
  rm -rf "$ROOT_TMP"
}
trap 'cleanup "$?"' EXIT

PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
ENDPOINT="ws://127.0.0.1:${PORT}"
e2e_log_value "standalone binary" "$RUN_BIN"
e2e_log_value "state dir" "$STATE_DIR"
e2e_log_value "endpoint" "$ENDPOINT"

run_sagens() {
  env \
    -u SAGENS_KERNEL \
    -u SAGENS_ROOTFS \
    -u SAGENS_FIRMWARE \
    -u SAGENS_GUEST_AGENT_PATH \
    SAGENS_STATE_DIR="$STATE_DIR" \
    SAGENS_CONFIG="$CONFIG_JSON" \
    SAGENS_ENDPOINT="$ENDPOINT" \
    "$RUN_BIN" "$@"
}

run_sagens_with_config() {
  local config_path="$1"
  shift
  env \
    -u SAGENS_KERNEL \
    -u SAGENS_ROOTFS \
    -u SAGENS_FIRMWARE \
    -u SAGENS_GUEST_AGENT_PATH \
    SAGENS_STATE_DIR="$STATE_DIR" \
    SAGENS_CONFIG="$config_path" \
    SAGENS_ENDPOINT="$ENDPOINT" \
    "$RUN_BIN" "$@"
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo "expected output to contain: $needle" >&2
    echo "actual output:" >&2
    printf '%s\n' "$haystack" >&2
    exit 1
  fi
}

assert_macos_embedded_kernel_not_pe() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    return
  fi
  local kernel="$STATE_DIR/embedded-bundle/sagens/vmlinuz-virt"
  python3 - "$kernel" <<'PY'
from pathlib import Path
import sys

kernel = Path(sys.argv[1])
if not kernel.is_file():
    raise SystemExit(f"missing embedded kernel: {kernel}")
if kernel.read_bytes()[0x38:0x3c] != b"ARMd":
    raise SystemExit(f"macOS embedded kernel must be a raw ARM64 Image: {kernel}")
PY
}

extract_first_uuid() {
  python3 -c '
import re
import sys

text = sys.stdin.read()
match = re.search(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b", text)
if match:
    print(match.group(0))
'
}

extract_table_value() {
  local label="$1"
  python3 -c '
import re
import sys

label = sys.argv[1]
text = re.sub(r"\x1b\[[0-9;]*m", "", sys.stdin.read())
for raw_line in text.splitlines():
    parts = [part.strip() for part in re.split(r"[│|]", raw_line) if part.strip()]
    if len(parts) >= 2 and parts[0] == label:
        print(parts[1])
        break
' "$label"
}

START_OUT="$(e2e_run_capture "Start daemon" "sagens start" run_sagens start)"
assert_contains "$START_OUT" "daemon "
assert_macos_embedded_kernel_not_pe

HELP_OUT="$(e2e_run_capture "Show CLI help" "sagens" run_sagens)"
assert_contains "$HELP_OUT" "sagens <command> [args]"

BOX_NEW_OUT="$(e2e_run_capture "Create BOX" "sagens box new" run_sagens box new)"
BOX_ID="$(printf '%s\n' "$BOX_NEW_OUT" | extract_first_uuid)"
if [[ ! "$BOX_ID" =~ ^[0-9a-fA-F-]{36}$ ]]; then
  echo "invalid box id: $BOX_ID" >&2
  exit 1
fi
e2e_log_value "BOX_ID" "$BOX_ID"

LIST_OUT="$(e2e_run_capture "List BOXes" "sagens box list" run_sagens box list)"
assert_contains "$LIST_OUT" "$BOX_ID"
assert_contains "$LIST_OUT" "CREATED"

ADMIN_ADD_OUT="$(e2e_run_capture "Create admin credential" "sagens admin add" run_sagens admin add)"
assert_contains "$ADMIN_ADD_OUT" "Admin UUID"
assert_contains "$ADMIN_ADD_OUT" "Admin token"
assert_contains "$ADMIN_ADD_OUT" "Endpoint"
ADMIN_UUID="$(printf '%s\n' "$ADMIN_ADD_OUT" | extract_table_value "Admin UUID")"
ADMIN_TOKEN="$(printf '%s\n' "$ADMIN_ADD_OUT" | extract_table_value "Admin token")"
ADMIN_ENDPOINT="$(printf '%s\n' "$ADMIN_ADD_OUT" | extract_table_value "Endpoint")"
if [[ -z "$ADMIN_UUID" || -z "$ADMIN_TOKEN" || -z "$ADMIN_ENDPOINT" ]]; then
  echo "failed to parse admin add output" >&2
  printf '%s\n' "$ADMIN_ADD_OUT" >&2
  exit 1
fi
e2e_log_value "ADMIN_UUID" "$ADMIN_UUID"
e2e_log_value "ADMIN_ENDPOINT" "$ADMIN_ENDPOINT"

cat > "$SECOND_CONFIG_JSON" <<EOF
{
  "version": 1,
  "admin_uuid": "$ADMIN_UUID",
  "admin_token": "$ADMIN_TOKEN",
  "endpoint": "$ADMIN_ENDPOINT"
}
EOF
chmod 600 "$SECOND_CONFIG_JSON"

SECOND_LIST_OUT="$(e2e_run_capture "List BOXes via second config" "sagens(second config) box list" run_sagens_with_config "$SECOND_CONFIG_JSON" box list)"
assert_contains "$SECOND_LIST_OUT" "$BOX_ID"

RM_OUT="$(e2e_run_capture "Remove BOX" "sagens box rm $BOX_ID" run_sagens box rm "$BOX_ID")"
assert_contains "$RM_OUT" "removed"

FINAL_LIST_OUT="$(e2e_run_capture "List BOXes after removal" "sagens box list" run_sagens box list)"
assert_contains "$FINAL_LIST_OUT" "No BOXes found."

QUIT_OUT="$(e2e_run_capture "Stop daemon" "sagens quit" run_sagens quit)"
assert_contains "$QUIT_OUT" "daemon stopped"

QUIT_AGAIN_OUT="$(e2e_run_capture "Stop daemon again" "sagens quit" run_sagens quit)"
assert_contains "$QUIT_AGAIN_OUT" "daemon already stopped"

echo "standalone smoke e2e passed"
