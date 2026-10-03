#!/bin/sh

set -eu

error() {
  printf 'error: %s\n' "$*" >&2
}

info() {
  printf '%s\n' "$*" >&2
}

usage() {
  cat <<EOF
Install AUV from an official GitHub release.

Usage:
  install.sh [options]

Options:
  -v, --version <version>   Release version, for example v0.0.25 or latest
  -d, --install-dir <dir>   Install directory [default: \$HOME/.local/bin]
  -h, --help                Show this help

Environment:
  AUV_VERSION               Default release version [default: latest]
  AUV_INSTALL_DIR           Default install directory
  AUV_RELEASES_URL          Release base URL
EOF
}

require_value() {
  option=$1
  count=$2
  if [ "$count" -lt 2 ]; then
    error "Missing value for $option"
    exit 1
  fi
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    error "Required command not found: $1"
    exit 1
  fi
}

version=${AUV_VERSION:-latest}
releases_url=${AUV_RELEASES_URL:-https://github.com/moeru-ai/auv/releases}
releases_url=${releases_url%/}

if [ -n "${AUV_INSTALL_DIR:-}" ]; then
  install_dir=$AUV_INSTALL_DIR
elif [ -n "${HOME:-}" ]; then
  install_dir=$HOME/.local/bin
else
  error 'HOME is not set; pass --install-dir or set AUV_INSTALL_DIR.'
  exit 1
fi

while [ "$#" -gt 0 ]; do
  case "$1" in
    -v | --version)
      require_value "$1" "$#"
      version=$2
      shift 2
      ;;
    -d | --install-dir)
      require_value "$1" "$#"
      install_dir=$2
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      error "Unknown option: $1"
      usage >&2
      exit 1
      ;;
  esac
done

require_command curl
require_command install
require_command mktemp
require_command tar

if [ "$version" != latest ]; then
  if ! printf '%s\n' "$version" | grep -Eq '^v?[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$'; then
    error "Invalid release version: $version"
    exit 1
  fi
  case "$version" in
    v*) ;;
    *) version=v$version ;;
  esac
fi

case "$(uname -s)" in
  Darwin) platform=apple-darwin ;;
  Linux) platform=unknown-linux-gnu ;;
  *)
    error "Unsupported operating system: $(uname -s)"
    exit 1
    ;;
esac

case "$(uname -m)" in
  arm64 | aarch64) arch=aarch64 ;;
  x86_64 | amd64) arch=x86_64 ;;
  *)
    error "Unsupported architecture: $(uname -m)"
    exit 1
    ;;
esac

if [ "$platform" = unknown-linux-gnu ] && command -v ldd >/dev/null 2>&1; then
  ldd_version=$(ldd --version 2>&1 || :)
  if printf '%s\n' "$ldd_version" | grep -qi musl; then
    error 'Linux musl is not supported by the published AUV release artifacts.'
    exit 1
  fi
fi

target=$arch-$platform
archive=auv-$target.tar.gz
checksum_file=$archive.sha256

if [ "$version" = latest ]; then
  asset_url=$releases_url/latest/download/$archive
  checksum_url=$releases_url/latest/download/$checksum_file
else
  asset_url=$releases_url/download/$version/$archive
  checksum_url=$releases_url/download/$version/$checksum_file
fi

work_dir=$(mktemp -d "${TMPDIR:-/tmp}/auv-install.XXXXXX")
cleanup() {
  rm -rf "$work_dir"
}
trap cleanup 0 1 2 15

archive_path=$work_dir/$archive
checksum_path=$work_dir/$checksum_file
extract_dir=$work_dir/extract
mkdir -p "$extract_dir"

download() {
  url=$1
  destination=$2
  case "$url" in
    https://*) curl --proto '=https' --tlsv1.2 -fsSL "$url" -o "$destination" ;;
    *) curl -fsSL "$url" -o "$destination" ;;
  esac
}

info "Downloading $asset_url"
download "$asset_url" "$archive_path"
download "$checksum_url" "$checksum_path"

expected=$(awk 'NR == 1 { print tolower($1) }' "$checksum_path")
if ! printf '%s\n' "$expected" | grep -Eq '^[0-9a-f]{64}$'; then
  error "Invalid checksum file: $checksum_url"
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$archive_path" | awk '{ print tolower($1) }')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$archive_path" | awk '{ print tolower($1) }')
else
  error 'Required SHA-256 tool not found: install sha256sum or shasum.'
  exit 1
fi

if [ "$actual" != "$expected" ]; then
  error "Checksum mismatch for $archive"
  error "expected: $expected"
  error "actual:   $actual"
  exit 1
fi

tar -xzf "$archive_path" -C "$extract_dir" auv
if [ ! -f "$extract_dir/auv" ]; then
  error "Release archive did not contain the auv executable: $archive"
  exit 1
fi

mkdir -p "$install_dir"
if [ ! -d "$install_dir" ] || [ ! -w "$install_dir" ]; then
  error "Install directory is not writable: $install_dir"
  exit 1
fi

install -m 0755 "$extract_dir/auv" "$install_dir/auv"
info "Installed AUV $version to $install_dir/auv"

case ":${PATH:-}:" in
  *":$install_dir:"*) ;;
  *) info "Add $install_dir to PATH to run auv from any terminal." ;;
esac
