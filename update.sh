#!/usr/bin/env bash
#
# Updates an installed s1grep.
#
#   curl -fsSL https://raw.githubusercontent.com/apiservicesac/s1grep/main/update.sh | bash
#
# Pin a version with --version 0.3.0. A script rather than a subcommand: the file being replaced is the one that would
# be doing the replacing. It downloads, checks the download against SHA256SUMS, proves the new binary runs, and only
# then swaps it in. Models, indexes and settings are kept.

set -euo pipefail

REPOSITORY="apiservicesac/s1grep"
VERSION="${S1GREP_VERSION:-latest}"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; BOLD='\033[1m'; RESET='\033[0m'

info()    { echo -e "${CYAN}${BOLD}→${RESET} $*"; }
success() { echo -e "${GREEN}${BOLD}✓${RESET} $*"; }
warn()    { echo -e "${YELLOW}${BOLD}!${RESET} $*"; }
error()   { echo -e "\n${RED}${BOLD}✗${RESET} $*\n" >&2; exit 1; }

usage() {
    cat <<USAGE
Usage: update.sh [options]

  --version V     Install a specific release instead of the latest
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *)         echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
    esac
done

command -v curl >/dev/null || error "curl is required"
target="$(command -v s1grep || true)"
[ -n "${target}" ] && [ -x "${target}" ] || error "s1grep is not installed (or not in PATH). Use install.sh instead."
current="$("${target}" --version 2>/dev/null || echo "unknown")"
info "Installed: ${current} (${target})"

if [ "${VERSION}" = "latest" ]; then
    url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${REPOSITORY}/releases/latest")" \
        || error "Could not reach GitHub to find the latest release."
    VERSION="${url##*/}"
fi
VERSION="${VERSION#v}"

if [ "${current}" = "s1grep ${VERSION}" ]; then
    success "Already on ${current}."
    exit 0
fi

base="https://github.com/${REPOSITORY}/releases/download/${VERSION}"
file="s1grep-${VERSION}-x86_64-linux"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

info "Downloading ${VERSION}..."
curl -fsSL --retry 3 "${base}/${file}" -o "${work}/${file}" || error "Could not download ${base}/${file}"
curl -fsSL --retry 3 "${base}/SHA256SUMS" -o "${work}/SHA256SUMS" || error "Could not download SHA256SUMS"
(cd "${work}" && grep " ${file}\$" SHA256SUMS | sha256sum -c --quiet -) \
    || error "The download does not match SHA256SUMS: it arrived incomplete or changed. Try again."

# Prove it runs before it replaces anything that works.
staged="${work}/${file}"
chmod +x "${staged}"
fetched="$("${staged}" --version 2>/dev/null)" || error "That download is not a working s1grep. Try again."

# The background process still runs the old version; stop it so the next search starts the new one.
"${target}" stop >/dev/null 2>&1 || true
# `install` replaces in one move: writing over the binary in place can leave no s1grep at all if interrupted.
install -m 755 "${staged}" "${target}"
success "Updated ${current} → ${fetched}"

# Fetch any model a new version needs; files already present are kept.
"${target}" setup >/dev/null 2>&1 || warn "Could not check the models. Run: s1grep setup"
