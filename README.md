# `auv`

[![License](https://badgen.net/github/license/moeru-ai/auv)](LICENSE.md)

## What It Is

AUV turns application UI workflows into inspectable, replayable operations.
Current fact sources live in:

- `src/runtime.rs`
- `crates/auv-cli-invoke/`
- `crates/auv-driver/`
- `crates/auv-driver-macos/`
- `docs/ai/references/`

The former JSON `skill` and checked-in `recipes/` execution lane has been
removed. App workflows should be modeled as Rust commands or typed driver
orchestration instead of new recipe manifests.

Current validated native-app samples are narrow:

- Apple Music Windows command surface:
  `docs/ai/references/2026-06-26-apple-music-windows-command-reference.md`
- QQ音乐 playback slices
- Notes AX text sample
- TextEdit AX text sample

Stable verification contracts:

- `debug.verifyNowPlayingTitle` for QQ音乐 playback
- `debug.verifyAxText` for native text-bearing apps

Useful CLI entrypoints:

- `cargo run --quiet -- invoke --help`
- `cargo run --quiet -- scan window-region --target <application-id> --region 0.0,0.0,1.0,1.0 --max-pages 3`

`scan window-region` is the first scroll-scan workflow. It is OCR-first,
region-scoped, conservative about duplicate text, and records why scanning
stopped instead of unconditionally claiming a complete collection.

## Protocol Buffers

The initial protobuf surface lives under `proto/`. Rust consumers use the
generated types exposed by `crates/auv-api-proto`.

- Edit schemas under `proto/auv/api/v1/`.
- Build generated Rust with Cargo:

  ```shell
  cargo check -p auv-api-proto
  ```

- Lint schemas with Buf:

  ```shell
  nix --extra-experimental-features nix-command --extra-experimental-features flakes develop --command buf lint proto
  ```

- Generate through the Buf template when checking SDK output:

  ```shell
  nix --extra-experimental-features nix-command --extra-experimental-features flakes develop --command buf generate proto --template proto/buf.gen.yaml
  ```

Cargo builds use vendored `protoc` through `crates/auv-api-proto/build.rs`.
The Nix dev shell provides `buf`, `protobuf`, `protoc-gen-prost`, and
`protoc-gen-tonic` for explicit schema work.

## License

[Apache License 2.0](LICENSE.md)
