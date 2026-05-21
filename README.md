# `auv`

[![License](https://badgen.net/github/license/moeru-ai/auv)](LICENSE.md)

## What It Is

AUV turns application UI workflows into inspectable, replayable recipes and
bundle-shaped skill artifacts. Current fact sources live in:

- `src/runtime.rs`
- `src/catalog.rs`
- `src/skill.rs`
- `src/bundle.rs`
- `src/driver/macos/`
- `recipes/`
- `bundles/`
- `docs/ai/references/`

Current validated native-app samples are narrow:

- QQ音乐 playback slices
- Notes AX text sample
- TextEdit AX text sample

Stable verification contracts:

- `debug.verifyNowPlayingTitle` for QQ音乐 playback
- `debug.verifyAxText` for native text-bearing apps

Useful CLI entrypoints:

- `cargo run --quiet -- list-commands`
- `cargo run --quiet -- skill cases list`
- `cargo run --quiet -- skill bundle list`
- `cargo run --quiet -- skill bundle coverage native.app.skill-tree.v0`
- `cargo run --quiet -- scan window-region --target <bundle-id> --region 0.0,0.0,1.0,1.0 --max-pages 3`

`scan window-region` is the first scroll-scan workflow. It is OCR-first,
region-scoped, conservative about duplicate text, and records why scanning
stopped instead of unconditionally claiming a complete collection.

## License

[Apache License 2.0](LICENSE.md)
