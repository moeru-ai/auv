#!/bin/sh
# Install AUV from an official GitHub release.
#
# Environment:
#   AUV_VERSION      Release tag, for example v0.0.26 [default: latest]
#   AUV_INSTALL_DIR  Install directory [default: $HOME/.local/bin]

set -eu

error() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

version=${AUV_VERSION:-latest}
install_dir=${AUV_INSTALL_DIR:-$HOME/.local/bin}

case "$(uname -s)" in
  Darwin) platform=apple-darwin ;;
  Linux) platform=unknown-linux-gnu ;;
  *) error "Unsupported operating system: $(uname -s)" ;;
esac

case "$(uname -m)" in
  arm64 | aarch64) arch=aarch64 ;;
  x86_64 | amd64) arch=x86_64 ;;
  *) error "Unsupported architecture: $(uname -m)" ;;
esac

archive=auv-$arch-$platform.tar.gz
case "$version" in
  latest) base_url=https://github.com/moeru-ai/auv/releases/latest/download ;;
  v*) base_url=https://github.com/moeru-ai/auv/releases/download/$version ;;
  *) base_url=https://github.com/moeru-ai/auv/releases/download/v$version ;;
esac

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT
cd "$work_dir"

printf 'Downloading %s\n' "$base_url/$archive" >&2
curl -fsSL "$base_url/$archive" -o "$archive"
curl -fsSL "$base_url/$archive.sha256" -o "$archive.sha256"

expected=$(awk '{ print $1; exit }' "$archive.sha256")
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$archive" | awk '{ print $1 }')
else
  actual=$(shasum -a 256 "$archive" | awk '{ print $1 }')
fi
[ "$actual" = "$expected" ] || error "Checksum mismatch for $archive"

tar -xzf "$archive" auv
./auv --version >/dev/null || error 'The downloaded auv cannot run on this host.'

mkdir -p "$install_dir"
install -m 0755 auv "$install_dir/auv"
printf 'Installed auv to %s\n' "$install_dir/auv" >&2

case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) printf 'Add %s to PATH to run auv from any terminal.\n' "$install_dir" >&2 ;;
esac
