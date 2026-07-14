# Contributing to AUV

Thanks for helping with AUV. This repository is Rust-first; use the shared
runtime, driver, and run-recording model instead of adding one-off command
paths.

## Prerequisites

- Git
- Rust stable with `cargo` and `rustfmt`
- Nix with flakes, optional but recommended for repository tooling

Install Rust with `rustup`:

```shell
rustup toolchain install stable
rustup component add rustfmt clippy
```

## For Nix users

On macOS or Linux, the dev shell provides Rust tools and protobuf tools,
including `buf`, `protobuf`, `protoc-gen-prost`, and `protoc-gen-tonic`.

```shell
nix --extra-experimental-features nix-command --extra-experimental-features flakes develop
```

## If you have already contributed before

If you already have a local checkout, update it before starting new work:

```shell
git fetch --all
git checkout main
git pull --rebase
```

If you have a working branch, rebase it on the updated `main`:

```shell
git checkout <your-branch-name>
git rebase main
```

## Fork and clone

Fork [moeru-ai/auv](https://github.com/moeru-ai/auv), then clone your fork:

```shell
git clone https://github.com/<your-github-username>/auv.git
cd auv
```

If this is your first contribution, add the upstream repository:

```shell
git remote add upstream https://github.com/moeru-ai/auv.git
```

## Create your working branch

```shell
git checkout -b <your-branch-name>
```

## Protobuf and Buf

Schemas live in `proto/`. Rust generated types are compiled by
`crates/auv-api-proto/build.rs`, which uses vendored `protoc` so normal Cargo
builds do not require a global `protoc`.

Useful commands:

```shell
cargo check -p auv-api-proto
cargo test -p auv-api-proto
nix --extra-experimental-features nix-command --extra-experimental-features flakes develop --command buf lint proto
nix --extra-experimental-features nix-command --extra-experimental-features flakes develop --command buf generate proto --template proto/buf.gen.yaml
```

The `.prototools` file records the Buf plugin source for tools that understand
proto tool plugin manifests. The Nix dev shell remains the expected setup for
schema work in this repository.

## Before commit

Before committing Rust changes, run:

```shell
cargo fmt --check
cargo check
cargo test
git diff --check
```

For the CLI smoke check:

```shell
cargo run --quiet -- invoke --help
bash scripts/ci/public-claims-check.sh
bash scripts/ci/install-smoke.sh
```

For docs-only changes, `git diff --check` is enough unless the docs change a
documented command or workflow.

## Commit

```shell
git add <changed-files>
git commit -m "<your-commit-message>"
```

Use concise Conventional Commit-style subjects when possible, for example:

```shell
git commit -m "docs: add proto contribution setup"
```

## Push

```shell
git push -u origin <your-branch-name>
```

## Create a pull request

Open a pull request from your branch to `moeru-ai/auv`. Include:

- a short description of the change
- relevant issue, design, or PR links
- validation commands you ran
