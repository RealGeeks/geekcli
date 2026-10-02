#!/bin/sh
# Install the geekcli CLI from its GitHub release archives.
#
# Verifies the sha256 that the Release workflow publishes beside each archive.
set -eu

usage() {
  cat <<'USAGE'
Install the geekcli CLI from its GitHub release archives.

  sh install.sh [--version vX.Y.Z] [--dir DIR]

Piped from curl, the flags belong to sh, not to this script, so pass them
after `-s --` or use the environment variables:

  curl -fsSL $RAW/install.sh | sh
  curl -fsSL $RAW/install.sh | sh -s -- --version v0.3.0
  curl -fsSL $RAW/install.sh | GEEKCLI_VERSION=v0.3.0 sh

`curl ... | sh --version v0.3.0` does NOT work: sh takes the flag itself.

Options (flag or environment):
  --version vX.Y.Z   GEEKCLI_VERSION       a tag; default: the latest release
  --dir DIR          GEEKCLI_INSTALL_DIR   default: /usr/local/bin if writable,
                                           else ~/.local/bin
                     GEEKCLI_REPO          default: realgeeks/geekcli
                     GH_TOKEN / GITHUB_TOKEN  optional; avoids GitHub's
                                           anonymous API rate limit
USAGE
}

REPO="${GEEKCLI_REPO:-realgeeks/geekcli}"
VERSION="${GEEKCLI_VERSION:-}"
INSTALL_DIR="${GEEKCLI_INSTALL_DIR:-}"
TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}"

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || { echo "install.sh: --version needs a tag" >&2; exit 2; }
               VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#--version=}"; shift ;;
    --dir) [ $# -ge 2 ] || { echo "install.sh: --dir needs a directory" >&2; exit 2; }
           INSTALL_DIR="$2"; shift 2 ;;
    --dir=*) INSTALL_DIR="${1#--dir=}"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "install.sh: unknown option $1" >&2; usage >&2; exit 2 ;;
  esac
done

say() { printf '%s\n' "$*" >&2; }
die() { say "install.sh: $*"; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }
need curl
need tar

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Darwin) os_part="apple-darwin" ;;
  Linux) os_part="unknown-linux-gnu" ;;
  MINGW*|MSYS*|CYGWIN*) die "on Windows run install.ps1 instead: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" ;;
  *) die "unsupported OS: $os" ;;
esac
case "$arch" in
  x86_64|amd64) arch_part="x86_64" ;;
  arm64|aarch64) arch_part="aarch64" ;;
  *) die "unsupported architecture: $arch" ;;
esac
target="$arch_part-$os_part"

auth_header=""
if [ -n "$TOKEN" ]; then
  auth_header="Authorization: Bearer $TOKEN"
fi

api() {
  # $1: API URL. Prints the body.
  if [ -n "$auth_header" ]; then
    curl -fsSL -H "$auth_header" -H "Accept: application/vnd.github+json" "$1"
  else
    curl -fsSL -H "Accept: application/vnd.github+json" "$1"
  fi
}

if [ -z "$VERSION" ]; then
  release_url="https://api.github.com/repos/$REPO/releases/latest"
else
  case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac
  release_url="https://api.github.com/repos/$REPO/releases/tags/$VERSION"
fi

release_json=$(api "$release_url") || die "cannot read the release ($release_url). If GitHub is rate limiting you, set GH_TOKEN."
tag=$(printf '%s' "$release_json" | sed -n 's/^ *"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
[ -n "$tag" ] || die "no tag_name in the release response"

name="geekcli-$tag-$target"
archive="$name.tar.gz"

# With a token, fetch through the API (which also works for a private fork);
# without one, the public download URL.
asset_api_url() {
  # GitHub lists each asset as url, id, node_id, name on consecutive lines.
  printf '%s\n' "$release_json" | grep -B 5 "\"name\": *\"$1\"" | sed -n 's/^ *"url": *"\(https:\/\/api\.github\.com\/repos\/[^"]*\/assets\/[0-9]*\)".*/\1/p' | tail -1
}
fetch_asset() {
  # $1: asset name, $2: destination
  if [ -n "$auth_header" ]; then
    url=$(asset_api_url "$1")
    [ -n "$url" ] || die "release $tag has no asset $1"
    curl -fsSL -H "$auth_header" -H "Accept: application/octet-stream" -o "$2" "$url"
  else
    curl -fsSL -o "$2" "https://github.com/$REPO/releases/download/$tag/$1"
  fi
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "Downloading geekcli $tag for $target..."
fetch_asset "$archive" "$tmp/$archive"
fetch_asset "$archive.sha256" "$tmp/$archive.sha256"

expected=$(cut -d' ' -f1 "$tmp/$archive.sha256")
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$archive" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)
fi
[ "$expected" = "$actual" ] || die "checksum mismatch for $archive (expected $expected, got $actual)"

tar -xzf "$tmp/$archive" -C "$tmp"
[ -f "$tmp/$name/geekcli" ] || die "$archive did not contain $name/geekcli"

if [ -z "$INSTALL_DIR" ]; then
  if [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
    INSTALL_DIR=/usr/local/bin
  else
    INSTALL_DIR="$HOME/.local/bin"
  fi
fi
mkdir -p "$INSTALL_DIR"
install -m 755 "$tmp/$name/geekcli" "$INSTALL_DIR/geekcli"

say "Installed $INSTALL_DIR/geekcli ($("$INSTALL_DIR/geekcli" --version))"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) say "Add it to your PATH:  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac
say "Next: geekcli auth login --site www.yoursite.com   (then: geekcli guide)"
