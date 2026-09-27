# crates.io publication

## Publication set

The workspace contains 29 crates with `publish = true`. The other 12 workspace
crates use `publish = false`.

Each internal path dependency has a registry version. Cargo removes the path
when it creates an archive. The registry version remains in the archive.

The `bumpp` hook updates `workspace.package.version` and each internal path
dependency version in the 29 publishable crates. It stops if a dependency
version is missing or differs from the previous workspace version. A `0.0.x`
version requirement accepts only that patch version.

## Source requirements

Clone the repository with recursive submodules. The `auv-media-macos` archive
includes the pinned `mediaremote-adapter` submodule.

The `auv-api-proto/proto` symlink points to the root `proto/` directory. Cargo
includes the required schemas as regular files in the crate archive.

On macOS, the build needs CMake and the Xcode Swift compiler. The Nix
development shell now provides CMake and exposes the system Swift compiler.

## Validation and publication

The Rust CI matrix tests the workspace on Linux, macOS, and Windows. A `v*`
tag push in `moeru-ai/auv` starts the Release packages workflow for both npm
and Cargo. The Cargo job checks out the tag and runs
`cargo publish --workspace --locked`. Cargo selects the publishable workspace
crates and publishes them in dependency order. The job needs a
`CARGO_REGISTRY_TOKEN` repository secret with publish access. To backfill a tag
that was pushed before this trigger existed, manually run the Release packages
workflow from `main` with `tag_name` set to the existing tag; the npm job is
skipped on manual runs. The Release workflow's prerelease and artifact-upload
events do not publish crates.

The complete 29-crate package set passed on macOS arm64 on 2026-09-24. The run used
Cargo 1.96.0, CMake 4.1.2, and Apple Swift 6.2.1.
On 2026-09-26, `cargo publish --workspace --locked --dry-run` packaged and
compiled all 29 publishable crates without uploading them.

## Registry names

The core SDK package is named `auv-core`; this avoids the already registered
`auv` name. The crate remains at `crates/auv`, and workspace dependents retain
the `auv` Rust import through Cargo dependency aliases. Check ownership and
availability of every package name again immediately before publication. A
failure after lower-level uploads leaves a partial release on crates.io. Check
which versions landed before retrying the publication job.
