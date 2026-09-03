#!/bin/sh
# DockerSmith installer.
#
# Download and review this script before running it when possible.
#
# Environment overrides:
#   DOCKERSMITH_INSTALL_DIR   where to install the binary (default: ~/.local/bin)
#   DOCKERSMITH_VERSION       release tag to install, e.g. v1.2.0 (default: latest)

set -eu

REPO="Kardzhilov/DockerSmith"
BIN="dockersmith"

# ── Pretty output ───────────────────────────────────────────────────────────
if [ -t 1 ]; then
    BOLD="$(printf '\033[1m')"; DIM="$(printf '\033[2m')"
    GREEN="$(printf '\033[32m')"; RED="$(printf '\033[31m')"
    YELLOW="$(printf '\033[33m')"; NC="$(printf '\033[0m')"
else
    BOLD=""; DIM=""; GREEN=""; RED=""; YELLOW=""; NC=""
fi
info() { printf '%s\n' "$*"; }
ok()   { printf '%s✓%s %s\n' "$GREEN" "$NC" "$*"; }
warn() { printf '%s!%s %s\n' "$YELLOW" "$NC" "$*"; }
err()  { printf '%s✗ %s%s\n' "$RED" "$*" "$NC" >&2; exit 1; }

# ── Detect platform ─────────────────────────────────────────────────────────
os="$(uname -s)"
arch="$(uname -m)"
case "$os:$arch" in
    Linux:x86_64 | Linux:amd64) target="x86_64-unknown-linux-gnu" ;;
    Linux:aarch64 | Linux:arm64) target="aarch64-unknown-linux-gnu" ;;
    Linux:armv7l | Linux:armv7) target="armv7-unknown-linux-musleabihf" ;;
    Darwin:x86_64 | Darwin:amd64) target="x86_64-apple-darwin" ;;
    Darwin:arm64 | Darwin:aarch64) target="aarch64-apple-darwin" ;;
    *) err "Unsupported platform: $os $arch" ;;
esac
if [ "$os" = "Linux" ] && [ "$target" != "armv7-unknown-linux-musleabihf" ] \
    && { [ -f /etc/alpine-release ] || ldd --version 2>&1 | grep -qi musl; }; then
    target="$(printf '%s' "$target" | sed 's/-gnu$/-musl/')"
fi

# ── Pick a downloader ───────────────────────────────────────────────────────
if command -v curl >/dev/null 2>&1; then
    DL="curl"
elif command -v wget >/dev/null 2>&1; then
    DL="wget"
else
    err "Neither curl nor wget is available; please install one and retry."
fi

fetch() { # fetch <url> <output-file>
    if [ "$DL" = "curl" ]; then
        curl -fSL --progress-bar "$1" -o "$2"
    else
        wget -q --show-progress -O "$2" "$1"
    fi
}

# ── Choose a destination ───────────────────────────────────────────────────
install_dir="${DOCKERSMITH_INSTALL_DIR:-$HOME/.local/bin}"
need_sudo=0
if ! mkdir -p "$install_dir" 2>/dev/null || [ ! -w "$install_dir" ]; then
    command -v sudo >/dev/null 2>&1 \
        || err "Install directory is not writable: $install_dir. Install sudo or choose DOCKERSMITH_INSTALL_DIR."
    info "${DIM}Requesting elevated access to ${install_dir}${NC}"
    sudo -v || err "Could not obtain elevated access for $install_dir."
    sudo mkdir -p "$install_dir" || err "Could not create install directory: $install_dir"
    need_sudo=1
fi

privileged() {
    if [ "$need_sudo" -eq 1 ]; then
        sudo "$@"
    else
        "$@"
    fi
}

# ── Resolve the release tag ─────────────────────────────────────────────────
if [ -n "${DOCKERSMITH_VERSION:-}" ]; then
    tag="$DOCKERSMITH_VERSION"
elif [ "$DL" = "curl" ]; then
    # Follow the "latest" redirect and read the resolved tag from the final URL.
    if ! effective_url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/${REPO}/releases/latest")"; then
        err "Could not query the latest release."
    fi
    tag="$(printf '%s' "$effective_url" | sed 's#.*/##')"
else
    if ! release_json="$(wget -qO- "https://api.github.com/repos/${REPO}/releases/latest")"; then
        err "Could not query the latest release."
    fi
    tag="$(printf '%s' "$release_json" | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"[^"]+"' \
        | head -1 | sed -E 's/.*"([^"]+)"/\1/')"
fi
[ -n "$tag" ] || err "Could not determine the latest release version."
printf '%s\n' "$tag" | grep -Eq '^v?[0-9]+([.][0-9]+){0,2}([-+][0-9A-Za-z.-]+)?$' \
    || err "Release tag has an unexpected format: $tag"

# ── Resolve download URL ────────────────────────────────────────────────────
# Assets are named  dockersmith-<target>-<tag>  (version at the end).
asset="${BIN}-${target}-${tag}"
url="https://github.com/${REPO}/releases/download/${tag}/${asset}"

info "${BOLD}Installing DockerSmith ${tag}${NC} ${DIM}(${target})${NC}"

# ── Download ────────────────────────────────────────────────────────────────
umask 077
tmp="$(mktemp "${TMPDIR:-/tmp}/.${BIN}.download.XXXXXX")"
sum="$(mktemp "${TMPDIR:-/tmp}/.${BIN}.checksum.XXXXXX")"
staged=""
cleanup() {
    rm -f "$tmp" "$sum"
    if [ -n "$staged" ]; then
        privileged rm -f "$staged" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM
fetch "$url" "$tmp" || err "Download failed. No prebuilt binary at ${url}"

[ -s "$tmp" ] || err "Downloaded file is empty."

# ── Verify checksum ─────────────────────────────────────────────────────────
# The release publishes <asset>.sha256 next to each binary.
fetch "${url}.sha256" "$sum" 2>/dev/null \
    || err "No published checksum found for ${asset}; refusing to install."
[ -s "$sum" ] || err "Published checksum is empty; refusing to install."
expected="$(awk 'NR == 1 { print $1 }' "$sum")"
printf '%s\n' "$expected" | grep -Eq '^[0-9A-Fa-f]{64}$' \
    || err "Published checksum is not a valid SHA-256 digest."
expected="$(printf '%s' "$expected" | tr '[:upper:]' '[:lower:]')"
published_name="$(awk 'NR == 1 && NF >= 2 { sub(/^\*/, "", $2); print $2 }' "$sum")"
if [ -n "$published_name" ] && [ "$published_name" != "$asset" ]; then
    err "Checksum names ${published_name}, expected ${asset}."
fi
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$tmp" | awk '{print $1}')"
else
    err "No SHA-256 tool found (need sha256sum or shasum); refusing to install."
fi
actual="$(printf '%s' "$actual" | tr '[:upper:]' '[:lower:]')"
[ "$expected" = "$actual" ] || err "Checksum mismatch — refusing to install."
ok "Checksum verified"

# ── Install ─────────────────────────────────────────────────────────────────
staged="$(privileged mktemp "$install_dir/.${BIN}.install.XXXXXX")" \
    || err "Could not create a staging file in $install_dir."
privileged install -m 0755 "$tmp" "$staged" \
    || err "Could not stage DockerSmith in $install_dir."
privileged mv -f "$staged" "$install_dir/$BIN" \
    || err "Could not replace $install_dir/$BIN."
staged=""
rm -f "$tmp"
rm -f "$sum"
trap - EXIT INT TERM
ok "Installed to ${BOLD}${install_dir}/${BIN}${NC}"

# ── Ensure it's on PATH ─────────────────────────────────────────────────────
add_path_line() {
    # $1 = rc file, $2 = line to append (idempotent on the install dir).
    rc="$1"; line="$2"
    [ -f "$rc" ] || : > "$rc"
    if ! grep -qsF "$install_dir" "$rc"; then
        printf '\n# Added by the DockerSmith installer\n%s\n' "$line" >> "$rc"
        ok "Added ${install_dir} to PATH in ${DIM}${rc}${NC}"
        RC_UPDATED="$rc"
    fi
}

RC_UPDATED=""
case ":$PATH:" in
    *":$install_dir:"*)
        ok "${install_dir} is already on your PATH"
        ;;
    *)
        shell_name="$(basename "${SHELL:-sh}")"
        case "$shell_name" in
            bash) add_path_line "$HOME/.bashrc" "export PATH=\"$install_dir:\$PATH\"" ;;
            zsh)  add_path_line "${ZDOTDIR:-$HOME}/.zshrc" "export PATH=\"$install_dir:\$PATH\"" ;;
            fish)
                fish_rc="$HOME/.config/fish/config.fish"
                mkdir -p "$(dirname "$fish_rc")"
                add_path_line "$fish_rc" "fish_add_path -- \"$install_dir\""
                ;;
            *) add_path_line "$HOME/.profile" "export PATH=\"$install_dir:\$PATH\"" ;;
        esac
        ;;
esac

# ── Verify + next steps ─────────────────────────────────────────────────────
info ""
if "$install_dir/$BIN" --version >/dev/null 2>&1; then
    ok "$("$install_dir/$BIN" --version)"
else
    warn "Installed, but the binary did not run — your glibc may be older than the build host's."
fi

info ""
info "${BOLD}Done!${NC}"
if [ -n "$RC_UPDATED" ]; then
    info "Restart your shell or run: ${BOLD}source $RC_UPDATED${NC}"
fi
info "Then launch it with: ${BOLD}${BIN}${NC}"
