#!/usr/bin/env bash
set -euo pipefail

APP_NAME="sanges"
APP_LABEL="Sagens"
REPO_SLUG="${SAGENS_REPO:-xenage/sanges}"
REQUESTED_VERSION="${SAGENS_VERSION:-latest}"
INSTALL_DIR="${SAGENS_INSTALL_DIR:-}"
ASSUME_YES="${SAGENS_ASSUME_YES:-0}"
FORCE_INSTALL="${SAGENS_FORCE_INSTALL:-0}"
MODIFY_BASH_PATH="${SAGENS_MODIFY_BASH_PATH:-1}"

if [[ -t 1 && -z "${NO_COLOR:-}" && "${TERM:-}" != "dumb" ]]; then
  COLOR_RESET=$'\033[0m'
  COLOR_BANNER=$'\033[1;36m'
  COLOR_STEP=$'\033[1;34m'
  COLOR_META=$'\033[2m'
  COLOR_OK=$'\033[1;32m'
  COLOR_WARN=$'\033[1;33m'
  COLOR_FAIL=$'\033[1;31m'
else
  COLOR_RESET=""
  COLOR_BANNER=""
  COLOR_STEP=""
  COLOR_META=""
  COLOR_OK=""
  COLOR_WARN=""
  COLOR_FAIL=""
fi

FETCHER=""
HASHER=""
PLATFORM=""
VERSION=""
ASSET_NAME=""
ASSET_URL=""
CHECKSUM_URL=""
WORK_DIR=""
INSTALL_PATH=""
PATH_UPDATED=0

line() {
  local stream="$1"
  local color="$2"
  local message="$3"

  if [[ "$stream" == "stderr" ]]; then
    printf '%b%s%b\n' "$color" "$message" "$COLOR_RESET" >&2
    return
  fi

  printf '%b%s%b\n' "$color" "$message" "$COLOR_RESET"
}

step() {
  line stdout "$COLOR_STEP" ""
  line stdout "$COLOR_STEP" "$1"
}

note() {
  line stdout "$COLOR_META" "      $1"
}

ok() {
  line stdout "$COLOR_OK" "      $1"
}

die() {
  line stderr "$COLOR_FAIL" "error: $1"
  exit 1
}

usage() {
  cat <<'EOF'
Usage: ./install.sh [options]

Options:
  --version <tag>      Install a specific release tag instead of the latest one.
  --dir <path>         Install into a specific directory.
  --repo <owner/name>  Download from a different GitHub repository.
  --force              Replace an existing target without prompting.
  --yes                Run non-interactively.
  --no-modify-path     Do not update bash startup files if PATH is missing.
  -h, --help           Show this help.

Environment overrides:
  SAGENS_VERSION
  SAGENS_INSTALL_DIR
  SAGENS_REPO
  SAGENS_FORCE_INSTALL=1
  SAGENS_ASSUME_YES=1
  SAGENS_MODIFY_BASH_PATH=0
EOF
}

print_banner() {
  line stdout "$COLOR_BANNER" "============================================================"
  line stdout "$COLOR_BANNER" "  ____"
  line stdout "$COLOR_BANNER" " / ___|  __ _  __ _  ___ _ __  ___"
  line stdout "$COLOR_BANNER" " \\___ \\ / _\` |/ _\` |/ _ \\ '_ \\/ __|"
  line stdout "$COLOR_BANNER" "  ___) | (_| | (_| |  __/ | | \\__ \\"
  line stdout "$COLOR_BANNER" " |____/ \\__,_|\\__, |\\___|_| |_|___/"
  line stdout "$COLOR_BANNER" "              |___/"
  line stdout "$COLOR_BANNER" "============================================================"
  line stdout "$COLOR_META" "Install the latest ${APP_LABEL} release for this machine."
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

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
      return 1
      ;;
  esac
}

download_text() {
  local url="$1"

  if [[ "$FETCHER" == "curl" ]]; then
    curl -fsSL --retry 3 --connect-timeout 20 --proto "=https" --tlsv1.2 \
      -H "Accept: application/vnd.github+json" \
      -A "sanges-install.sh" \
      "$url"
    return
  fi

  wget -qO- --https-only --tries=3 --timeout=20 \
    --header="Accept: application/vnd.github+json" \
    --user-agent="sanges-install.sh" \
    "$url"
}

download_file() {
  local url="$1"
  local dest="$2"

  if [[ "$FETCHER" == "curl" ]]; then
    if [[ -t 1 ]]; then
      curl -fL --retry 3 --connect-timeout 20 --proto "=https" --tlsv1.2 \
        -A "sanges-install.sh" \
        -# \
        -o "$dest" \
        "$url"
    else
      curl -fL --retry 3 --connect-timeout 20 --proto "=https" --tlsv1.2 \
        -A "sanges-install.sh" \
        -o "$dest" \
        "$url"
    fi
    return
  fi

  if [[ -t 1 ]]; then
    wget --https-only --tries=3 --timeout=20 \
      --user-agent="sanges-install.sh" \
      --progress=bar:force:noscroll \
      -O "$dest" \
      "$url"
  else
    wget --https-only --tries=3 --timeout=20 \
      --user-agent="sanges-install.sh" \
      -O "$dest" \
      "$url"
  fi
}

dir_on_path() {
  local needle="$1"
  local old_ifs="$IFS"
  local entry

  IFS=':'
  for entry in $PATH; do
    if [[ "$entry" == "$needle" ]]; then
      IFS="$old_ifs"
      return 0
    fi
  done
  IFS="$old_ifs"

  return 1
}

dir_writable_or_creatable() {
  local dir="$1"
  local parent

  if [[ -d "$dir" ]]; then
    [[ -w "$dir" ]]
    return
  fi

  parent="$(dirname "$dir")"
  while [[ "$parent" != "/" && ! -d "$parent" ]]; do
    parent="$(dirname "$parent")"
  done

  [[ -d "$parent" && -w "$parent" ]]
}

path_line() {
  local dir="$1"

  if [[ "$dir" == "$HOME"* ]]; then
    printf 'export PATH="%s:$PATH"\n' "${dir/#$HOME/\$HOME}"
    return
  fi

  printf 'export PATH="%s:$PATH"\n' "$dir"
}

append_if_missing() {
  local file="$1"
  local wanted="$2"

  if [[ -f "$file" ]] && grep -Fqx "$wanted" "$file"; then
    return
  fi

  mkdir -p "$(dirname "$file")"
  if [[ -f "$file" && -s "$file" ]]; then
    printf '\n%s\n' "$wanted" >> "$file"
  else
    printf '%s\n' "$wanted" >> "$file"
  fi
}

maybe_update_bash_path() {
  local export_line="$1"

  if [[ "$MODIFY_BASH_PATH" != "1" ]]; then
    return
  fi

  append_if_missing "$HOME/.bashrc" "$export_line"
  append_if_missing "$HOME/.bash_profile" "$export_line"
  PATH_UPDATED=1
}

confirm_replace() {
  local target="$1"

  if [[ ! -e "$target" || "$FORCE_INSTALL" == "1" || "$ASSUME_YES" == "1" ]]; then
    return 0
  fi

  if [[ ! -t 0 ]]; then
    return 1
  fi

  printf 'Replace existing %s at %s? [y/N] ' "$APP_NAME" "$target" >&2
  read -r reply
  case "$reply" in
    y | Y | yes | YES)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      [[ $# -ge 2 ]] || die "--version requires a value"
      REQUESTED_VERSION="$2"
      shift 2
      ;;
    --dir)
      [[ $# -ge 2 ]] || die "--dir requires a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --repo)
      [[ $# -ge 2 ]] || die "--repo requires a value"
      REPO_SLUG="$2"
      shift 2
      ;;
    --force)
      FORCE_INSTALL=1
      shift
      ;;
    --yes)
      ASSUME_YES=1
      shift
      ;;
    --no-modify-path)
      MODIFY_BASH_PATH=0
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      die "unknown option: $1"
      ;;
  esac
done

print_banner

step "[1/8] Checking required tools"
if have_cmd curl; then
  FETCHER="curl"
elif have_cmd wget; then
  FETCHER="wget"
else
  die "curl or wget is required"
fi

if have_cmd sha256sum; then
  HASHER="sha256sum"
elif have_cmd shasum; then
  HASHER="shasum"
else
  die "sha256sum or shasum is required"
fi

have_cmd mktemp || die "mktemp is required"
have_cmd uname || die "uname is required"
have_cmd chmod || die "chmod is required"
note "downloader: $FETCHER"
note "checksum tool: $HASHER"
ok "tooling looks good"

step "[2/8] Detecting host platform"
PLATFORM="$(host_platform)" || die "unsupported host platform: $(uname -s)/$(uname -m)"
note "platform: $PLATFORM"
ok "platform supported"

step "[3/8] Resolving release version"
if [[ "$REQUESTED_VERSION" == "latest" ]]; then
  RELEASE_JSON="$(download_text "https://api.github.com/repos/${REPO_SLUG}/releases/latest")" || die "failed to resolve the latest release"
  VERSION="$(printf '%s' "$RELEASE_JSON" | tr -d '\n' | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
  [[ -n "$VERSION" ]] || die "failed to parse tag_name from GitHub release metadata"
else
  VERSION="$REQUESTED_VERSION"
fi

ASSET_NAME="${APP_NAME}-${VERSION}-${PLATFORM}"
ASSET_URL="https://github.com/${REPO_SLUG}/releases/download/${VERSION}/${ASSET_NAME}"
CHECKSUM_URL="${ASSET_URL}.sha256"
note "repo: $REPO_SLUG"
note "version: $VERSION"
note "asset: $ASSET_NAME"
ok "release resolved"

step "[4/8] Selecting install directory"
if [[ -z "$INSTALL_DIR" ]]; then
  candidates=()
  case "$PLATFORM" in
    macos-aarch64)
      candidates=(/opt/homebrew/bin /usr/local/bin "$HOME/.local/bin" "$HOME/bin")
      ;;
    macos-x86_64)
      candidates=(/usr/local/bin /opt/homebrew/bin "$HOME/.local/bin" "$HOME/bin")
      ;;
    linux-*)
      candidates=(/usr/local/bin "$HOME/.local/bin" "$HOME/bin")
      ;;
  esac

  for candidate in "${candidates[@]}"; do
    if dir_writable_or_creatable "$candidate" && dir_on_path "$candidate"; then
      INSTALL_DIR="$candidate"
      break
    fi
  done

  if [[ -z "$INSTALL_DIR" ]]; then
    for candidate in "${candidates[@]}"; do
      if dir_writable_or_creatable "$candidate"; then
        INSTALL_DIR="$candidate"
        break
      fi
    done
  fi
fi

[[ -n "$INSTALL_DIR" ]] || die "could not find a writable install directory; use --dir"
INSTALL_PATH="${INSTALL_DIR}/${APP_NAME}"
note "install dir: $INSTALL_DIR"
if dir_on_path "$INSTALL_DIR"; then
  note "PATH: already available"
else
  note "PATH: will add for future bash sessions"
fi
ok "install location ready"

step "[5/8] Downloading release asset"
TEMP_ROOT="${TMPDIR:-/tmp}"
TEMP_ROOT="${TEMP_ROOT%/}"
WORK_DIR="$(mktemp -d "${TEMP_ROOT}/sanges-install.XXXXXX")"
DOWNLOAD_PATH="${WORK_DIR}/${APP_NAME}"
CHECKSUM_PATH="${WORK_DIR}/${APP_NAME}.sha256"
download_file "$ASSET_URL" "$DOWNLOAD_PATH" || die "failed to download ${ASSET_NAME}"
download_file "$CHECKSUM_URL" "$CHECKSUM_PATH" || die "failed to download ${ASSET_NAME}.sha256"
chmod 755 "$DOWNLOAD_PATH"
note "downloaded to: $DOWNLOAD_PATH"
ok "download complete"

step "[6/8] Verifying release"
EXPECTED_SUM="$(awk 'NR == 1 { print $1 }' "$CHECKSUM_PATH")"
[[ -n "$EXPECTED_SUM" ]] || die "checksum file is empty or invalid"
if [[ "$HASHER" == "sha256sum" ]]; then
  ACTUAL_SUM="$(sha256sum "$DOWNLOAD_PATH" | awk '{print $1}')"
else
  ACTUAL_SUM="$(shasum -a 256 "$DOWNLOAD_PATH" | awk '{print $1}')"
fi
[[ "$EXPECTED_SUM" == "$ACTUAL_SUM" ]] || die "checksum mismatch for ${ASSET_NAME}"

HELP_OUTPUT="$("$DOWNLOAD_PATH" --help 2>&1)" || {
  printf '%s\n' "$HELP_OUTPUT" >&2
  die "downloaded binary failed its smoke test"
}
if [[ "$HELP_OUTPUT" != *"usage: sanges"* && "$HELP_OUTPUT" != *"sagens <command> [args]"* ]]; then
  printf '%s\n' "$HELP_OUTPUT" >&2
  die "downloaded binary did not print the expected help output"
fi
note "sha256: $ACTUAL_SUM"
ok "checksum and smoke test passed"

step "[7/8] Installing ${APP_LABEL}"
mkdir -p "$INSTALL_DIR"
if [[ -e "$INSTALL_PATH" ]] && ! confirm_replace "$INSTALL_PATH"; then
  die "installation cancelled"
fi
TMP_INSTALL_PATH="${INSTALL_PATH}.tmp.$$"
cp "$DOWNLOAD_PATH" "$TMP_INSTALL_PATH"
chmod 755 "$TMP_INSTALL_PATH"
mv -f "$TMP_INSTALL_PATH" "$INSTALL_PATH"
if ! dir_on_path "$INSTALL_DIR"; then
  maybe_update_bash_path "$(path_line "$INSTALL_DIR")"
fi
note "installed binary: $INSTALL_PATH"
if [[ "$PATH_UPDATED" == "1" ]]; then
  note "updated: $HOME/.bashrc"
  note "updated: $HOME/.bash_profile"
fi
ok "installation complete"

step "[8/8] Final verification"
"$INSTALL_PATH" --help >/dev/null 2>&1 || die "installed binary failed final verification"
ok "${APP_LABEL} ${VERSION} is ready"

line stdout "$COLOR_STEP" ""
line stdout "$COLOR_STEP" "Installed ${APP_LABEL} ${VERSION}"
note "binary: $INSTALL_PATH"
note "command: ${APP_NAME}"

if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
  rm -rf "$WORK_DIR"
fi

if dir_on_path "$INSTALL_DIR"; then
  line stdout "$COLOR_OK" "Run it now: ${APP_NAME}"
elif [[ "$PATH_UPDATED" == "1" ]]; then
  line stdout "$COLOR_OK" "Open a new bash session or run: source ~/.bashrc"
  line stdout "$COLOR_OK" "Then start it with: ${APP_NAME}"
else
  line stdout "$COLOR_WARN" "Add ${INSTALL_DIR} to PATH, then run: ${APP_NAME}"
fi
