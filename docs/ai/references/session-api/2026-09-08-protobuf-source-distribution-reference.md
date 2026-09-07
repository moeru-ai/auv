# Protobuf source distribution

## Contract

`cargo install --git https://github.com/moeru-ai/auv auv-cli --bin auv`
must find third-party Protobuf sources in the Git checkout. Cargo builds use
`proto/vendor/` directly and the compiler supplied by `protoc-bin-vendored`.
Buf and yq are maintainer tools for dependency updates, not install prerequisites.

`proto/buf.yaml` and `proto/buf.lock` remain authoritative for schema dependencies.
The checked-in `proto/vendor/` is the unmodified export of each commit-qualified
BSR module in the lockfile, including its available license and documentation
files ([`buf export --all`](https://buf.build/docs/reference/cli/buf/export/)). The current Google APIs module contains 47 schemas;
AUV directly imports `google/api/annotations.proto`, which imports `http.proto`.
Keeping the existing whole-module export avoids a second list of dependency
versions or a custom import resolver. Upstream copyright headers are preserved.

Buf continues to exclude `vendor` from the first-party module and resolves its
own dependencies through the lockfile. Cargo compilation requires no BSR access;
Cargo itself still needs its normal Rust dependency downloads.

## Updating dependencies

With Buf 1.71.0 and Mike Farah yq available, update dependencies deliberately:

```sh
cd proto
buf dep update
cd ..
./scripts/generate-proto-vendor
```

On Windows, run `./scripts/generate-proto-vendor.ps1` instead. Regeneration
without `buf dep update` reproduces the currently locked export. Do not edit
vendored files manually. Review and commit the lockfile and vendor changes
together, then run the normal schema and Rust checks.

Rust checks, JS CLI builds, and release builds consume the checked-in sources
without regenerating them. A separate CI job runs both export scripts and
rejects modified, deleted, or untracked vendor files. This checks reproducibility
without repairing an incomplete checkout before Cargo gets to compile it.

## Regression and evidence

At commit `372a07be`, `proto/vendor/` was ignored. CI generated it before builds,
masking the missing sources in `cargo install --git`. Compiling `health.proto`
from `git archive HEAD proto` reproduced `google/api/annotations.proto: File not
found`; exporting the locked dependency made the same compilation pass.

Validated on macOS arm64 on 2026-09-08 using a temporary Git repository containing
the proposed changes (snapshot `98f14234cfec50c04c2ef58ac4ae936b64b5c53c`):

- All first-party schemas compiled from `git archive` of that snapshot. Removing
  its vendored `annotations.proto` reproduced the original error.
- Regenerating with Buf 1.71.0 produced no modified, deleted, or untracked files.
- `cargo install --git file://<snapshot-repository> --rev <snapshot-commit>
  auv-cli --bin auv --locked --root <temporary-install> --target-dir <build-cache>`
  completed in the default release profile. Cargo fetched its own Git checkout;
  no vendor-generation step ran in that checkout. The existing failed-install
  build cache was reused; this was not a cold-cache benchmark.
- The installed `auv --help` and `auv invoke --help` both exited successfully.
- `cargo test -p auv-api-proto --locked`: 3 tests passed.
- `cargo fmt --check`, shell syntax, workflow YAML parsing, and whitespace checks
  passed (rustfmt reported existing configuration warnings).

The installation destination was temporary and did not replace the developer's
installed `auv`. Linux and Windows execution remains for CI, including the
PowerShell regeneration script; those platforms were not run locally.

## Boundary

This fix covers Git source distribution. NOTICE: crates.io packaging of schemas
outside individual crate directories is deferred until an owner-approved
registry-publication slice; this Git-install check does not establish that
`cargo package` produces self-contained crate archives.
