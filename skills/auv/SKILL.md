---
name: auv
description: >-
  Automate desktop applications with AUV. Use for screenshots, OCR, mouse and
  keyboard input, reusable GUI workflows, or operations on a paired AUV Device.
---

# AUV

Prefer existing application operations for repeated workflows.
Build reusable operations with the typed Rust APIs or [TypeScript SDK](https://github.com/moeru-ai/auv/blob/main/js/packages/sdk/README.md).
Keep actions and application verification together in the operation.

## Start here

Discover commands from the installed version:

```sh
auv invoke --help                      # capabilities and platform limits
auv invoke <command-id> --help         # arguments, targets, and examples
auv plugin list                        # installed application commands
```

Use `--json` for structured invoke results. For permissions or environment errors, run `auv doctor` on the affected host.

For remote execution, discover names with `auv devices list --json` and put `--device <NAME>` before `invoke`.

## Coordinates and results

- Clicks use logical coordinates. Project screenshot pixels through the capture bounds, scale, and any image resize.
- `completed` means command completion. Inspect delivery and fallback results, then verify application success separately.

## Runs

Invoke records Run evidence automatically. Retain returned `run_id` values and artifact locations.
For related calls in one daemon Run, read `auv run --help` and pass the returned ID through root `--run <ID>`.
