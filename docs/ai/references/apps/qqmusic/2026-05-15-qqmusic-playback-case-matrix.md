## QQ音乐 Playback Case Matrix

Date: 2026-05-15

Status: validated narrow-skill coverage note

### Purpose

This note records the first real case matrix for the QQ音乐 macOS playback
baseline.

The point is not to pretend the app is broadly solved. The point is to prove
that the same narrow playback skill can survive more than one visible result
anchor, and to record where it still fails.

### Executable Coverage Entry

The product-facing coverage entrypoint is now:

```bash
auv-cli skill cases run macos.qqmusic.play_visible_anchor.v0
```

The machine-readable matrix lives at:

- `recipes/macos/qqmusic/play-visible-anchor.cases.v0.json`

### Validated Cases

The following three cases were re-run sequentially through
`auv-cli skill cases run macos.qqmusic.play_visible_anchor.v0` and completed
successfully:

1. `ascii-aa-cure-for-me`
   - query: `aa`
   - anchor: `Cure For Me`
   - playback title: `Cure For Me - AURORA`
   - representative verification run: `run_1778828314846_44673_6`

2. `ascii-aa-soft-universe`
   - query: `aa`
   - anchor: `Soft Universe`
   - playback title query: `Soft Universe`
   - representative verification run: `run_1778828323232_44673_13`

3. `ascii-aa-aa-alone-again`
   - query: `aa`
   - anchor: `AA (Alone Again)`
   - playback title query: `AA (Alone Again)`
   - representative verification run: `run_1778828331460_44673_20`

What this proves:

- the same narrow skill survives more than one visible row anchor
- the narrow skill is not accidentally tied to one single AURORA row
- the captured-image OCR verification path still works across multiple anchors

What it does **not** prove:

- generalized QQ音乐 playback for arbitrary queries
- pointer-free activation
- Chinese OCR anchor stability

### Current Known Failure

The matrix also includes one explicit candidate case:

- `chinese-query-chinese-anchor`
  - query: `周杰伦`
  - anchor: `晴天`
  - playback title query: `晴天`

The current run fails at `resolve-ocr-anchor`:

- failure run: `run_1778828350214_45802_3`
- observed output:
  - `Found 0 OCR text matches in the current desktop screenshot after applying the active filters.`

This is useful because it turns "Chinese OCR is shaky" from a vibe into a
reproducible product boundary.

### Why This Matters

This is the first point where the QQ音乐 playback slice starts looking like a
productized narrow skill instead of a single lucky demo:

- there is one formal recipe
- there is one formal case matrix
- there is one formal coverage command
- validated cases and known failures are both declared in repo truth

The next step is not "more abstraction". The next step is to use this matrix to
decide whether Chinese OCR anchor resolution deserves its own signal upgrade or
its own fallback contract.

The Chinese case is now also being tracked in the row-based fallback matrix:

- `recipes/macos/qqmusic/play-visible-row.cases.v0.json`
