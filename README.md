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

## License

[Apache License 2.0](LICENSE.md)
