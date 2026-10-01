#!/usr/bin/env bash
#
# s1grep installer.
#
#   curl -fsSL https://raw.githubusercontent.com/apiservicesac/s1grep/main/install.sh | bash
#
# Installs the latest release into ~/.local/bin, for the current user and without sudo, then downloads the models
# (about 2.4 GB, once). Every download is checked against the release's SHA256SUMS, and the binary is proven to run
# before it is put in place.

set -euo pipefail

# ══════════════════════════════════════════════════════════════════════════════
# Settings
# ══════════════════════════════════════════════════════════════════════════════
REPOSITORY="apiservicesac/s1grep"
INSTALL_DIR="${S1GREP_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${S1GREP_VERSION:-latest}"
MINIMUM_GLIBC="2.38"
MODELS=true
UNINSTALL=""
PURGE=""

# ══════════════════════════════════════════════════════════════════════════════
# Output
# ══════════════════════════════════════════════════════════════════════════════
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; RESET='\033[0m'

info()    { echo -e "${CYAN}${BOLD}→${RESET} $*"; }
success() { echo -e "${GREEN}${BOLD}✓${RESET} $*"; }
warn()    { echo -e "${YELLOW}${BOLD}!${RESET} $*"; }
error()   { echo -e "\n${RED}${BOLD}✗${RESET} $*\n" >&2; exit 1; }
step()    { echo -e "\n${BOLD}── $* ──${RESET}"; }

# Read from the terminal, not stdin: piped from curl, the script itself arrives on stdin. With no terminal the
# default answer stands.
ask_yes() {
    local prompt="$1" answer
    if ! : 2>/dev/null </dev/tty; then return 0; fi
    read -rp "$(echo -e "${CYAN}${prompt} [Y/n]: ${RESET}")" answer </dev/tty
    [[ ! "${answer}" =~ ^[nN] ]]
}

# ══════════════════════════════════════════════════════════════════════════════
# Arguments
# ══════════════════════════════════════════════════════════════════════════════
usage() {
    cat <<USAGE
Usage: install.sh [options]

  --version V     Install a specific release instead of the latest
  --dir DIR       Install into DIR (default: ~/.local/bin)
  --no-models     Do not download the models now (run \`s1grep setup\` later)
  --uninstall     Remove s1grep from this machine; indexes and models stay
  --purge         With --uninstall, also delete the models, indexes and settings
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)   VERSION="$2";     shift 2 ;;
        --dir)       INSTALL_DIR="$2"; shift 2 ;;
        --no-models) MODELS=false;     shift ;;
        --uninstall) UNINSTALL="1";    shift ;;
        --purge)     PURGE="1";        shift ;;
        --help|-h)   usage; exit 0 ;;
        *)           echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
    esac
done

TARGET="${INSTALL_DIR}/s1grep"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/s1grep"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/s1grep"

# ══════════════════════════════════════════════════════════════════════════════
# Uninstall
# ══════════════════════════════════════════════════════════════════════════════
uninstall() {
    step "Removing s1grep"
    [ -x "${TARGET}" ] && "${TARGET}" stop >/dev/null 2>&1 || true
    rm -f "${TARGET}"
    success "Removed ${TARGET}"
    if [ -n "${PURGE}" ]; then
        rm -rf "${CACHE_DIR}" "${CONFIG_DIR}"
        success "Removed the models, indexes and settings (${CACHE_DIR}, ${CONFIG_DIR})"
    else
        info "Models and indexes stay in ${CACHE_DIR}; --uninstall --purge deletes them."
    fi
    exit 0
}

# ══════════════════════════════════════════════════════════════════════════════
# 1 — Is this machine supported
# ══════════════════════════════════════════════════════════════════════════════
preflight() {
    step "Checking this machine"
    case "$(uname -s)" in
        Linux) ;;
        Darwin) error "macOS is not supported yet." ;;
        *) error "On Windows, run in PowerShell:
    irm https://raw.githubusercontent.com/${REPOSITORY}/main/install.ps1 | iex" ;;
    esac
    [ "$(uname -m)" = "x86_64" ] || error "Only x86-64 is supported for now (this machine is $(uname -m))."
    for tool in curl sha256sum; do
        command -v "${tool}" >/dev/null || error "${tool} is required"
    done
    local glibc
    glibc="$(ldd --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+$' || echo 0)"
    [ "$(printf '%s\n%s\n' "${MINIMUM_GLIBC}" "${glibc}" | sort -V | head -1)" = "${MINIMUM_GLIBC}" ] \
        || error "glibc ${MINIMUM_GLIBC} or newer is required (this machine has ${glibc}): Ubuntu 24.04, Debian 13 or newer."
    success "Linux x86-64, glibc ${glibc}"
}

# ══════════════════════════════════════════════════════════════════════════════
# 2 — Download, check and install
# ══════════════════════════════════════════════════════════════════════════════
resolve_version() {
    if [ "${VERSION}" = "latest" ]; then
        local url
        url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${REPOSITORY}/releases/latest")" \
            || error "Could not reach GitHub to find the latest release."
        VERSION="${url##*/}"
        [ -n "${VERSION}" ] && [ "${VERSION}" != "latest" ] || error "No published release found."
    fi
    VERSION="${VERSION#v}"
}

install_binary() {
    step "Installing s1grep ${VERSION}"
    local base="https://github.com/${REPOSITORY}/releases/download/${VERSION}"
    local file="s1grep-${VERSION}-x86_64-linux"
    local work
    work="$(mktemp -d)"
    trap 'rm -rf "${work}"' EXIT

    info "Downloading ${file}..."
    curl -fsSL --retry 3 "${base}/${file}" -o "${work}/${file}" || error "Could not download ${base}/${file}"
    curl -fsSL --retry 3 "${base}/SHA256SUMS" -o "${work}/SHA256SUMS" || error "Could not download SHA256SUMS"
    (cd "${work}" && grep " ${file}\$" SHA256SUMS | sha256sum -c --quiet -) \
        || error "The download does not match SHA256SUMS: it arrived incomplete or changed. Try again."

    # Prove it runs before it replaces anything.
    chmod +x "${work}/${file}"
    "${work}/${file}" --version >/dev/null 2>&1 || error "The downloaded binary does not run on this machine."

    mkdir -p "${INSTALL_DIR}"
    [ -x "${TARGET}" ] && "${TARGET}" stop >/dev/null 2>&1 || true
    install -m 755 "${work}/${file}" "${TARGET}"
    success "Installed $("${TARGET}" --version) in ${TARGET}"
}

check_path() {
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) return ;;
    esac
    warn "${INSTALL_DIR} is not in your PATH. Add this line to ~/.bashrc or ~/.zshrc, then open a new terminal:"
    echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
}

# ══════════════════════════════════════════════════════════════════════════════
# 3 — Models
# ══════════════════════════════════════════════════════════════════════════════
install_models() {
    step "Models"
    if ! ${MODELS}; then
        info "Skipped. Run \`s1grep setup\` before the first search."
        return
    fi
    if ask_yes "Download the models now (about 2.4 GB, once)?"; then
        "${TARGET}" setup
    else
        info "Run \`s1grep setup\` before the first search."
    fi
}

# ══════════════════════════════════════════════════════════════════════════════
# Main
# ══════════════════════════════════════════════════════════════════════════════
echo -e "\n  ${BOLD}s1grep${RESET} · find code by asking what it does\n"
[ -n "${UNINSTALL}" ] && uninstall
preflight
resolve_version
install_binary
check_path
install_models

step "Ready"
echo "  s1grep \"where do we retry a failed payment\" path/to/repo"
echo "  s1grep status            what is loaded and indexed"
echo "  s1grep skill --install   teach Claude Code to use it"
echo
