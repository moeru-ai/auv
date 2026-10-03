# Nix distribution design

Date: 2026-10-04

## Status

Approved implementation slice. This note defines the source-built Nix package
and its user-facing limits.

## Problem

The repository has a flake, but it does not expose an installable package.
`packages` is nested inside each development shell instead of being a top-level
flake output. The package expression also reads the version from
`package.version`, while the root manifest stores it at
`workspace.package.version`.

As a result, `nix build .#` fails because
`packages.<system>.default` does not exist.

## Decision

Repair the existing flake as a source package. Do not add another installer or
wrap the GitHub release archives.

The flake will expose `packages.<system>.default` for:

- `aarch64-darwin`
- `x86_64-darwin`
- `aarch64-linux`
- `x86_64-linux`

The default package builds the `auv` binary from the `auv-cli` Cargo package,
reads the version from `[workspace.package]`, and sets `auv` as the main
program.

The macOS build requires the `mediaremote-adapter` Git submodule. Set
`inputs.self.submodules = true` so remote Git flake users receive the same
source tree without adding a query parameter. This attribute requires Nix 2.27
or newer.

Keep development-only tools in the development shell. The installable package
will declare only the build tools and linked libraries required by the selected
platform:

- all platforms: CMake;
- macOS: the existing system `swiftc` pattern, a system `codesign` wrapper,
  native-only framework compilation, Apple clang for the final Swift-aware
  Darwin link, and `libiconv`;
- Linux: `pkg-config`, the Nixpkgs bindgen hook, PipeWire, Wayland,
  libxkbcommon, Tesseract, and Leptonica.

Build only `--package auv-cli --bin auv`. The repository's full test suite is
run separately because it includes host and daemon integration behavior that is
not suitable for a Nix sandbox package check.

## User flow

With Nix 2.27 or newer and flakes enabled:

```sh
nix profile install github:moeru-ai/auv#default
auv --version
```

This is an optional source-install route. It does not replace Homebrew, Scoop,
proto, Cargo, or direct release downloads.

## macOS Helper boundary

The Nix package must not claim to include the official signed
`AUV Helper.app`. The release workflow embeds the helper only after Developer
ID signing and Apple notarization. A public source build does not have those
credentials and leaves the embedded helper archive empty.

On macOS, users who need `auv setup macos-helper install` should use an
official Homebrew, proto, or direct-release installation. Projects embedding
AUV may instead supply their own signed and rebranded helper through the
existing integration path.

## Verification

Before publication:

1. Confirm `nix flake show --all-systems` exposes all four package outputs.
2. Build and run `auv --version` on Apple Silicon macOS.
3. Build and run `auv --version` on GNU Linux x86-64.
4. Evaluate the Intel macOS and ARM64 Linux outputs without upgrading that
   evaluation into a native-build claim.
5. Confirm the macOS package reports that no helper is embedded.
6. Run the repository test suite, README generation, and `git diff --check`.

Native validation on 2026-10-04 built and ran `auv 0.0.25` on Apple Silicon
macOS with Nix 2.34.1 and on GNU Linux x86-64 with Nix 2.32.4. The other two
package outputs evaluated successfully. The macOS result also reported
`helper_embedded: false`.

## Non-goals

- Packaging Windows or musl Linux through Nix.
- Embedding unsigned or locally ad-hoc-signed helper binaries.
- Adding a Nix-specific release workflow.
- Publishing to nixpkgs in this slice.

## Evidence

- [Nix flake package outputs](https://nix.dev/concepts/flakes.html)
- [Nix 2.27 submodule support](https://nix.dev/manual/nix/2.35/release-notes/rl-2.27)
- [Nix profile install](https://nix.dev/manual/nix/2.18/command-ref/new-cli/nix3-profile-install.html)
- [Nixpkgs dependency categories](https://nixos.org/manual/nixpkgs/unstable/#ssec-stdenv-dependencies)
- [Rust bindgen support in Nix](https://wiki.nixos.org/wiki/Rust#Installing_with_bindgen_support)
- [AUV release workflow](../../../../.github/workflows/release.yml)
