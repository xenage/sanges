#!/usr/bin/env bash

if [[ -n "${SAGENS_E2E_LOGGING_SOURCED:-}" ]]; then
  return 0
fi
SAGENS_E2E_LOGGING_SOURCED=1

if [[ -t 2 && -z "${NO_COLOR:-}" && "${TERM:-}" != "dumb" ]]; then
  E2E_COLOR_RESET=$'\033[0m'
  E2E_COLOR_STEP=$'\033[1;34m'
  E2E_COLOR_META=$'\033[2m'
else
  E2E_COLOR_RESET=""
  E2E_COLOR_STEP=""
  E2E_COLOR_META=""
fi

e2e_begin_group() {
  local label="$1"
  printf '\n' >&2
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    printf '::group::%s\n' "$label" >&2
  fi
  printf '%b==> %s%b\n' "$E2E_COLOR_STEP" "$label" "$E2E_COLOR_RESET" >&2
}

e2e_end_group() {
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    printf '::endgroup::\n' >&2
  fi
}

e2e_log_meta() {
  printf '%b    %s%b\n' "$E2E_COLOR_META" "$1" "$E2E_COLOR_RESET" >&2
}

e2e_log_output() {
  local output="$1"
  if [[ -z "$output" ]]; then
    e2e_log_meta "output: <empty>"
    return
  fi
  e2e_log_meta "output:"
  while IFS= read -r line; do
    printf '      %s\n' "$line" >&2
  done <<< "$output"
}

e2e_log_value() {
  local key="$1"
  local value="$2"
  e2e_log_meta "$key: $value"
}

e2e_run_capture() {
  local label="$1"
  local display="$2"
  local runner="$3"
  shift 3

  local output=""
  local status=0
  e2e_begin_group "$label"
  e2e_log_meta "command: $display"
  set +e
  output="$("$runner" "$@" 2>&1)"
  status=$?
  set -e
  e2e_log_output "$output"
  e2e_end_group
  if [[ "$status" -ne 0 ]]; then
    return "$status"
  fi
  printf '%s' "$output"
}

e2e_run_capture_with_stdin() {
  local label="$1"
  local display="$2"
  local runner="$3"
  local stdin_payload="$4"
  shift 4

  local output=""
  local status=0
  e2e_begin_group "$label"
  e2e_log_meta "command: $display"
  set +e
  output="$(printf '%s' "$stdin_payload" | "$runner" "$@" 2>&1)"
  status=$?
  set -e
  e2e_log_output "$output"
  e2e_end_group
  if [[ "$status" -ne 0 ]]; then
    return "$status"
  fi
  printf '%s' "$output"
}
