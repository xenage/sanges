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

ROOT_TMP="$(mktemp -d "${TMPDIR:-/tmp}/sagens-checkpoint-e2e.XXXXXX")"
STATE_DIR="$ROOT_TMP/state"
CONFIG_DIR="$ROOT_TMP/config"
RUN_BIN="$ROOT_TMP/sagens"
CONFIG_JSON="$CONFIG_DIR/config.json"

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

assert_not_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "$haystack" == *"$needle"* ]]; then
    echo "expected output not to contain: $needle" >&2
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

extract_first_uuid() {
  python3 - <<'PY'
import re
import sys

text = sys.stdin.read()
match = re.search(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b", text)
if match:
    print(match.group(0))
PY
}

wait_for_checkpoint_order() {
  python3 - <<'PY'
import time
time.sleep(0.02)
PY
}

assert_uuid() {
  local value="$1"
  if [[ ! "$value" =~ ^[0-9a-fA-F-]{36}$ ]]; then
    echo "invalid uuid: $value" >&2
    exit 1
  fi
}

START_OUT="$(run_sagens start)"
assert_contains "$START_OUT" "daemon "

BOX_ID="$(run_sagens box new | extract_first_uuid)"
assert_uuid "$BOX_ID"
run_sagens box start "$BOX_ID" >/dev/null

printf 'seed' > "$ROOT_TMP/note.txt"
run_sagens box fs "$BOX_ID" upload "$ROOT_TMP/note.txt" /workspace/note.txt >/dev/null
run_sagens box fs "$BOX_ID" download /workspace/note.txt "$ROOT_TMP/note-downloaded.txt" >/dev/null
SEED_READ="$(cat "$ROOT_TMP/note-downloaded.txt")"
assert_equals "$SEED_READ" "seed"

CHECKPOINT_A="$(run_sagens box checkpoint create "$BOX_ID" --name seed --meta stage=seed --meta suite=checkpoint | extract_first_uuid)"
assert_uuid "$CHECKPOINT_A"

CHECKPOINT_LIST="$(run_sagens box checkpoint list "$BOX_ID")"
assert_contains "$CHECKPOINT_LIST" "$CHECKPOINT_A"
assert_contains "$CHECKPOINT_LIST" $'seed\t'
assert_contains "$CHECKPOINT_LIST" '"stage":"seed"'
assert_contains "$CHECKPOINT_LIST" '"suite":"checkpoint"'

wait_for_checkpoint_order
printf 'branch-b' > "$ROOT_TMP/note.txt"
run_sagens box fs "$BOX_ID" upload "$ROOT_TMP/note.txt" /workspace/note.txt >/dev/null
CHECKPOINT_B="$(run_sagens box checkpoint create "$BOX_ID" --name branch-b --meta stage=branch-b | extract_first_uuid)"
assert_uuid "$CHECKPOINT_B"
CHECKPOINT_LIST_BEFORE_ROLLBACK="$(run_sagens box checkpoint list "$BOX_ID")"
assert_contains "$CHECKPOINT_LIST_BEFORE_ROLLBACK" "$CHECKPOINT_A"
assert_contains "$CHECKPOINT_LIST_BEFORE_ROLLBACK" "$CHECKPOINT_B"

printf 'live-before-rollback' > "$ROOT_TMP/note.txt"
run_sagens box fs "$BOX_ID" upload "$ROOT_TMP/note.txt" /workspace/note.txt >/dev/null
run_sagens box fs "$BOX_ID" download /workspace/note.txt "$ROOT_TMP/note-downloaded.txt" >/dev/null
LIVE_BEFORE_ROLLBACK="$(cat "$ROOT_TMP/note-downloaded.txt")"
assert_equals "$LIVE_BEFORE_ROLLBACK" "live-before-rollback"

RESTORE_ROLLBACK="$(run_sagens box checkpoint restore "$BOX_ID" "$CHECKPOINT_A" --mode rollback)"
assert_contains "$RESTORE_ROLLBACK" "$CHECKPOINT_A"
assert_contains "$RESTORE_ROLLBACK" "restored"
run_sagens box fs "$BOX_ID" download /workspace/note.txt "$ROOT_TMP/note-downloaded.txt" >/dev/null
RESTORED_READ="$(cat "$ROOT_TMP/note-downloaded.txt")"
assert_equals "$RESTORED_READ" "seed"
CHECKPOINT_LIST_AFTER_ROLLBACK="$(run_sagens box checkpoint list "$BOX_ID")"
assert_contains "$CHECKPOINT_LIST_AFTER_ROLLBACK" "$CHECKPOINT_A"
assert_not_contains "$CHECKPOINT_LIST_AFTER_ROLLBACK" "$CHECKPOINT_B"
assert_not_contains "$CHECKPOINT_LIST_AFTER_ROLLBACK" "branch-b"

wait_for_checkpoint_order
printf 'replace-base' > "$ROOT_TMP/note.txt"
run_sagens box fs "$BOX_ID" upload "$ROOT_TMP/note.txt" /workspace/note.txt >/dev/null
CHECKPOINT_C="$(run_sagens box checkpoint create "$BOX_ID" --name replace-base --meta stage=replace-base | extract_first_uuid)"
assert_uuid "$CHECKPOINT_C"

wait_for_checkpoint_order
printf 'replace-newer' > "$ROOT_TMP/note.txt"
run_sagens box fs "$BOX_ID" upload "$ROOT_TMP/note.txt" /workspace/note.txt >/dev/null
CHECKPOINT_D="$(run_sagens box checkpoint create "$BOX_ID" --name replace-newer --meta stage=replace-newer | extract_first_uuid)"
assert_uuid "$CHECKPOINT_D"
CHECKPOINT_LIST_BEFORE_REPLACE="$(run_sagens box checkpoint list "$BOX_ID")"
assert_contains "$CHECKPOINT_LIST_BEFORE_REPLACE" "$CHECKPOINT_C"
assert_contains "$CHECKPOINT_LIST_BEFORE_REPLACE" "$CHECKPOINT_D"

printf 'live-before-replace' > "$ROOT_TMP/note.txt"
run_sagens box fs "$BOX_ID" upload "$ROOT_TMP/note.txt" /workspace/note.txt >/dev/null
REPLACE_OUT="$(run_sagens box checkpoint restore "$BOX_ID" "$CHECKPOINT_C" --mode replace)"
assert_contains "$REPLACE_OUT" "$CHECKPOINT_C"
assert_contains "$REPLACE_OUT" "restored"
run_sagens box fs "$BOX_ID" download /workspace/note.txt "$ROOT_TMP/note-downloaded.txt" >/dev/null
REPLACED_READ="$(cat "$ROOT_TMP/note-downloaded.txt")"
assert_equals "$REPLACED_READ" "replace-base"
CHECKPOINT_LIST_AFTER_REPLACE="$(run_sagens box checkpoint list "$BOX_ID")"
assert_contains "$CHECKPOINT_LIST_AFTER_REPLACE" "$CHECKPOINT_C"
assert_contains "$CHECKPOINT_LIST_AFTER_REPLACE" "$CHECKPOINT_D"
assert_contains "$CHECKPOINT_LIST_AFTER_REPLACE" "replace-newer"

printf 'source-live' > "$ROOT_TMP/note.txt"
run_sagens box fs "$BOX_ID" upload "$ROOT_TMP/note.txt" /workspace/note.txt >/dev/null
run_sagens box fs "$BOX_ID" download /workspace/note.txt "$ROOT_TMP/note-downloaded.txt" >/dev/null
SOURCE_BEFORE_FORK="$(cat "$ROOT_TMP/note-downloaded.txt")"
assert_equals "$SOURCE_BEFORE_FORK" "source-live"
FORK_BOX_ID="$(run_sagens box checkpoint fork "$BOX_ID" "$CHECKPOINT_A" --name forked-seed | extract_first_uuid)"
assert_uuid "$FORK_BOX_ID"
run_sagens box fs "$BOX_ID" download /workspace/note.txt "$ROOT_TMP/note-downloaded.txt" >/dev/null
SOURCE_AFTER_FORK="$(cat "$ROOT_TMP/note-downloaded.txt")"
assert_equals "$SOURCE_AFTER_FORK" "source-live"
run_sagens box start "$FORK_BOX_ID" >/dev/null
run_sagens box fs "$FORK_BOX_ID" download /workspace/note.txt "$ROOT_TMP/fork-note-downloaded.txt" >/dev/null
FORK_READ="$(cat "$ROOT_TMP/fork-note-downloaded.txt")"
assert_equals "$FORK_READ" "seed"

DELETE_OUT="$(run_sagens box checkpoint delete "$BOX_ID" "$CHECKPOINT_A")"
assert_contains "$DELETE_OUT" "$CHECKPOINT_A"
assert_contains "$DELETE_OUT" "deleted"
DELETE_OUT="$(run_sagens box checkpoint delete "$BOX_ID" "$CHECKPOINT_C")"
assert_contains "$DELETE_OUT" "$CHECKPOINT_C"
assert_contains "$DELETE_OUT" "deleted"
DELETE_OUT="$(run_sagens box checkpoint delete "$BOX_ID" "$CHECKPOINT_D")"
assert_contains "$DELETE_OUT" "$CHECKPOINT_D"
assert_contains "$DELETE_OUT" "deleted"

CHECKPOINT_LIST_AFTER_DELETE="$(run_sagens box checkpoint list "$BOX_ID")"
assert_contains "$CHECKPOINT_LIST_AFTER_DELETE" "No checkpoints found."

run_sagens box stop "$FORK_BOX_ID" >/dev/null
run_sagens box rm "$FORK_BOX_ID" >/dev/null
run_sagens box stop "$BOX_ID" >/dev/null
run_sagens box rm "$BOX_ID" >/dev/null

QUIT_OUT="$(run_sagens quit)"
assert_contains "$QUIT_OUT" "daemon stopped"

echo "standalone checkpoint e2e passed"
