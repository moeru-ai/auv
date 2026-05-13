# AUV AIRI Desktop Reuse Notes

Date: 2026-05-13

Status: working implementation note

## Purpose

This note exists separately from `2026-05-12-auv-setup.md` so the design
brainstorming document can stay focused on product and architecture direction.

This file records the narrower implementation decision for current work:

- what AUV may reuse from AIRI desktop work
- what AUV must not import from AIRI
- what the current capability-validation slice actually implements

## Reuse Boundary

AUV may reuse selected implementation ideas or low-level primitives from AIRI
desktop work, but only at the driver layer.

Good donor candidates include:

- screenshot capture primitives
- platform input primitives such as Quartz, Swift, or AppleScript-backed calls
- CDP bridge patterns
- capability declaration and verification habits
- artifact capture habits around driver operations

These are not acceptable donor layers for AUV core:

- MCP tool descriptors
- server-side action executors
- approval queues
- chat, workflow, or policy orchestration shells
- AIRI-specific result shaping or provider-facing abstractions

The practical rule is:

> Borrow drivers and evidence capture, not the AIRI server wrapper.

This matters because AUV must keep its own runtime invocation, implicit run
creation, artifact retention, inspection, and replay semantics. If the project
imports AIRI's outer server shell too early, it will collapse back into another
computer-use service instead of an application command runtime.

## Current Slice Snapshot

The current repository prototype focuses on shared execution plus a narrow
macOS capability-validation slice, without importing AIRI's outer server shell.

Implemented in the current default build:

- library-first runtime core in `src/lib.rs`
- provisional file-backed local `.auv/` run store
- provisional file-backed local `.auv/` artifact store
- implicit run creation for every `invoke`
- minimal `inspect` CLI path over stored run snapshots
- fixture driver for non-destructive runtime validation
- macOS screenshot driver using the built-in `/usr/sbin/screencapture` utility
- macOS display enumeration via Swift + `NSScreen` / `CGDisplay`
- screenshot-backed coordinate readiness checks against the observed macOS display facts
- macOS permission probe for screen recording, accessibility, and automation

Intentionally not implemented yet:

- point-projection helpers or window-observation commands as stable public entrypoints
- app-launch or input-mutation desktop primitives
- OCR driver
- trace replay
- inspect UI
- AIRI-style action executor, approval queue, or MCP tool registration shell

This slice is deliberately narrow. It proves the execution substrate and the
host capability facts before stronger desktop or browser drivers land.

## Immediate Follow-Up

The next implementation steps should continue to respect the same boundary:

1. keep tightening screenshot-backed coordinate-readiness checks before adding input primitives
2. keep run and artifact inspection as the source of truth for driver behavior
3. scale screenshot, click, and scroll only after Swift and Rust boundaries are better verified
4. avoid introducing AIRI server semantics into AUV core just to move faster
