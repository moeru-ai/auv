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
staging_dir=
backup_dir=
link_tmp=
cleanup() {
  rm -rf "$work_dir"
  [ -z "$staging_dir" ] || rm -rf "$staging_dir"
  [ -z "$backup_dir" ] || rm -rf "$backup_dir"
  [ -z "$link_tmp" ] || rm -f "$link_tmp"
}
trap cleanup EXIT
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

mkdir -p "$install_dir"
if [ "$platform" = unknown-linux-gnu ]; then
  staging_dir=$install_dir/.auv.new.$$
  backup_dir=$install_dir/.auv.old.$$
  link_tmp=$install_dir/.auv-link.$$
  [ ! -e "$staging_dir" ] || error "Temporary install path already exists: $staging_dir"
  [ ! -e "$backup_dir" ] || error "Temporary backup path already exists: $backup_dir"
  if [ -d "$install_dir/auv" ] && [ ! -L "$install_dir/auv" ]; then
    error "Cannot replace directory: $install_dir/auv"
  fi

  mkdir "$staging_dir"
  tar -xzf "$archive" -C "$staging_dir"
  "$staging_dir/auv" --version >/dev/null || error 'The downloaded auv cannot run on this host.'
  ln -s .auv/auv "$link_tmp"

  if [ -e "$install_dir/.auv" ] || [ -L "$install_dir/.auv" ]; then
    mv "$install_dir/.auv" "$backup_dir"
  fi
  if ! mv "$staging_dir" "$install_dir/.auv" || ! mv -f "$link_tmp" "$install_dir/auv"; then
    rm -rf "$install_dir/.auv"
    if [ -e "$backup_dir" ] || [ -L "$backup_dir" ]; then
      mv "$backup_dir" "$install_dir/.auv"
      backup_dir=
    fi
    error 'Failed to replace the installed Linux bundle.'
  fi
  staging_dir=
  link_tmp=
  rm -rf "$backup_dir"
  backup_dir=
else
  tar -xzf "$archive" auv
  ./auv --version >/dev/null || error 'The downloaded auv cannot run on this host.'
  install -m 0755 auv "$install_dir/auv"
fi
printf 'Installed auv to %s\n' "$install_dir/auv" >&2

case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) printf 'Add %s to PATH to run auv from any terminal.\n' "$install_dir" >&2 ;;
esac
