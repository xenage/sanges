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

ROOT_TMP="$(mktemp -d "${TMPDIR:-/tmp}/sagens-e2e.XXXXXX")"
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
trap cleanup EXIT

PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
ENDPOINT="ws://127.0.0.1:${PORT}"

run_sagens() {
  env \
    -u SAGENS_LIBKRUN_LIBRARY \
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
    -u SAGENS_LIBKRUN_LIBRARY \
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

assert_equals() {
  local actual="$1"
  local expected="$2"
  if [[ "$actual" != "$expected" ]]; then
    echo "expected: $expected" >&2
    echo "actual:   $actual" >&2
    exit 1
  fi
}

extract_json_field() {
  local file="$1"
  local key="$2"
  sed -nE "s/^[[:space:]]*\"${key}\":[[:space:]]*\"([^\"]+)\"[[:space:]]*,?[[:space:]]*$/\\1/p" "$file" | head -n1
}

START_OUT="$(run_sagens start)"
assert_contains "$START_OUT" "daemon "

HELP_OUT="$(run_sagens)"
assert_contains "$HELP_OUT" "usage: sagens"

LIST_OUT="$(run_sagens box list)"
assert_equals "$LIST_OUT" ""

BOX_ID="$(run_sagens box new | tr -d '[:space:]')"
if [[ ! "$BOX_ID" =~ ^[0-9a-fA-F-]{36}$ ]]; then
  echo "invalid box id: $BOX_ID" >&2
  exit 1
fi

PS_OUT="$(run_sagens box ps)"
assert_contains "$PS_OUT" "$BOX_ID"
assert_contains "$PS_OUT" "created"

START_BOX_OUT="$(run_sagens box start "$BOX_ID")"
assert_contains "$START_BOX_OUT" "running"

BASH_OUT="$(run_sagens box exec "$BOX_ID" bash "echo hello-from-bash")"
assert_contains "$BASH_OUT" "hello-from-bash"

SHELL_CMD_OUT="$(run_sagens box exec "$BOX_ID" bash 'printf "%s\n" "$BASH_VERSION"; pwd; printf "shell-ok\n"')"
assert_contains "$SHELL_CMD_OUT" "."
assert_contains "$SHELL_CMD_OUT" "/workspace"
assert_contains "$SHELL_CMD_OUT" "shell-ok"

PY_OUT="$(run_sagens box exec "$BOX_ID" python -c "import json, sys; print(json.dumps({'hello': 'from-python', 'major': sys.version_info[0]}))")"
assert_contains "$PY_OUT" '"hello": "from-python"'
assert_contains "$PY_OUT" '"major": 3'

BASH_I_OUT="$(
  cat <<'EOF' | run_sagens box exec "$BOX_ID" bash -i
printf "%s\n" "$BASH_VERSION"
pwd
printf "shell-i-ok\n"
exit
EOF
)"
assert_contains "$BASH_I_OUT" "/workspace"
assert_contains "$BASH_I_OUT" "shell-i-ok"
assert_contains "$BASH_I_OUT" "."

PY_I_OUT="$(
  cat <<'EOF' | run_sagens box exec "$BOX_ID" python -i
import json
import sys
print(json.dumps({"interactive": True, "major": sys.version_info[0]}))
raise SystemExit
EOF
)"
assert_contains "$PY_I_OUT" '"interactive": true'
assert_contains "$PY_I_OUT" '"major": 3'

printf 'hello-from-stdin' > "$ROOT_TMP/note.txt"
run_sagens box fs "$BOX_ID" upload "$ROOT_TMP/note.txt" /workspace/note.txt >/dev/null

run_sagens box fs "$BOX_ID" download /workspace/note.txt "$ROOT_TMP/note-downloaded.txt" >/dev/null
READ_OUT="$(cat "$ROOT_TMP/note-downloaded.txt")"
assert_equals "$READ_OUT" "hello-from-stdin"

LS_OUT="$(run_sagens box fs "$BOX_ID" ls /workspace)"
assert_contains "$LS_OUT" "note.txt"

DIFF_OUT="$(run_sagens box fs "$BOX_ID" diff)"
assert_contains "$DIFF_OUT" $'A\tnote.txt'

run_sagens admin add > "$ROOT_TMP/admin-add.json"
ADMIN_UUID="$(extract_json_field "$ROOT_TMP/admin-add.json" "admin_uuid")"
ADMIN_TOKEN="$(extract_json_field "$ROOT_TMP/admin-add.json" "admin_token")"
ADMIN_ENDPOINT="$(extract_json_field "$ROOT_TMP/admin-add.json" "endpoint")"
if [[ -z "$ADMIN_UUID" || -z "$ADMIN_TOKEN" || -z "$ADMIN_ENDPOINT" ]]; then
  echo "failed to parse admin add output" >&2
  cat "$ROOT_TMP/admin-add.json" >&2
  exit 1
fi

cat > "$SECOND_CONFIG_JSON" <<EOF
{
  "version": 1,
  "admin_uuid": "$ADMIN_UUID",
  "admin_token": "$ADMIN_TOKEN",
  "endpoint": "$ADMIN_ENDPOINT"
}
EOF
chmod 600 "$SECOND_CONFIG_JSON"

SECOND_LIST_OUT="$(run_sagens_with_config "$SECOND_CONFIG_JSON" box list)"
assert_contains "$SECOND_LIST_OUT" "$BOX_ID"

STOP_OUT="$(run_sagens box stop "$BOX_ID")"
assert_contains "$STOP_OUT" "stopped"

RM_OUT="$(run_sagens box rm "$BOX_ID")"
assert_contains "$RM_OUT" "removed"

FINAL_LIST_OUT="$(run_sagens box list)"
assert_equals "$FINAL_LIST_OUT" ""

REMOVE_ME_OUT="$(run_sagens_with_config "$SECOND_CONFIG_JSON" admin remove me)"
assert_contains "$REMOVE_ME_OUT" "admin removed"

QUIT_OUT="$(run_sagens quit)"
assert_contains "$QUIT_OUT" "daemon stopped"

QUIT_AGAIN_OUT="$(run_sagens quit)"
assert_contains "$QUIT_AGAIN_OUT" "daemon already stopped"

echo "standalone shell e2e passed"
