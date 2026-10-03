# Unix release installer

Status: accepted distribution design, implemented in `install/install.sh`

## Goal

Provide one small POSIX shell installer for official AUV release binaries on
macOS and GNU/Linux. The installer complements Homebrew, proto, Nix, Cargo, and
manual downloads; it does not replace those routes or add another release
workflow.

## Contract

The installer:

- detects Apple Silicon and Intel macOS plus ARM64 and x86-64 GNU/Linux;
- downloads either the latest release or an exact `vX.Y.Z` release;
- downloads the matching `.sha256` asset and refuses to install a binary whose
  digest does not match;
- installs `auv` into `${AUV_INSTALL_DIR:-$HOME/.local/bin}` without `sudo`;
- accepts `--version`, `--install-dir`, and `--help`;
- accepts `AUV_VERSION`, `AUV_INSTALL_DIR`, and `AUV_RELEASES_URL` so the same
  path can be exercised against fixtures without changing production logic;
- prints a PATH hint when the install directory is not already on `PATH`.

The version may be written with or without the `v` prefix. The installer
normalizes exact versions to the release tag form and uses GitHub's
`releases/latest/download` redirect for `latest`, so it does not need the
GitHub API, `jq`, or a separate version-resolution script.

## Platform boundary

Release asset names come directly from `.github/workflows/release.yml`:

| Host | Release target |
| --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` |
| macOS x86-64 | `x86_64-apple-darwin` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |
| Linux x86-64 | `x86_64-unknown-linux-gnu` |

Linux musl is rejected explicitly because AUV does not publish musl release
artifacts. Windows remains owned by Scoop, proto, and direct ZIP downloads.

## Safety boundary

The script requires `curl`, `tar`, `mktemp`, and either `sha256sum` or
`shasum`. It downloads into a temporary directory, validates the release digest
before extraction, checks that the archive contains the expected executable,
and only then replaces the destination with `install -m 0755`.

The default destination is user-owned. An unwritable destination fails with a
clear error instead of escalating with `sudo`.

## macOS Helper boundary

The macOS release executable embeds the signed and notarized `AUV Helper.app`
archive produced by the release workflow. Installing that executable preserves
the existing `auv setup macos-helper install` flow; the shell installer does
not extract, modify, sign, or rebrand the helper itself.

## Verification

Before publication:

1. Run syntax and static shell checks.
2. Exercise all four target mappings with fixture archives.
3. Confirm checksum mismatch, musl, unsupported host, invalid version, missing
   executable, and unwritable destination failures.
4. Install the current official release on native Apple Silicon macOS and
   GNU/Linux x86-64, then run `auv --version`.
5. Run `cargo test`, `pnpm docs:update`, and `git diff --check`.

## Non-goals

- Adding Windows shell installation.
- Publishing musl builds.
- Adding helper scripts, a package manager, or a new updater workflow.
- Installing system-wide by default or invoking `sudo`.
- Changing release artifact contents or the Helper setup contract.

## Evidence

- [AUV release workflow](../../../../.github/workflows/release.yml)
- [alint single-script installer](https://github.com/alint-dev/alint/blob/main/install/install.sh)
- [GitHub latest-release asset links](https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases)
