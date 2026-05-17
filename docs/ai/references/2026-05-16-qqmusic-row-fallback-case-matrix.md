## QQ音乐 Row Fallback Case Matrix

Date: 2026-05-16

Status: validated fallback coverage note

### Purpose

This note records the row-based fallback slice for QQ音乐 playback.

The point is to keep the OCR baseline intact while validating a separate
visible-row activation path for cases where Chinese OCR anchors are not
reliable enough for grounding.

### Executable Coverage Entry

The product-facing coverage entrypoint is:

```bash
auv-cli skill cases run macos.qqmusic.play_visible_row.v0
```

The machine-readable matrix lives at:

- `recipes/macos/qqmusic/play-visible-row.cases.v0.json`

### Validated Cases

The following case is the current canonical row-based fallback baseline:

1. `ascii-aa-row-fallback`
   - query: `aa`
   - row index: `2`
   - playback title: `Cure For Me - AURORA`

What this proves:

- visible-row detection can drive activation without relying on a Chinese OCR anchor
- the row fallback verifies the now-playing title through the AX tree

What it does **not** prove:

- generalized QQ音乐 playback for arbitrary queries
- row fallback stability across layout changes
- pointer-free activation

### Validated Chinese Case

The matrix also includes one validated Chinese case:

1. `chinese-query-row-fallback`
   - query: `周杰伦`
   - requested title: `晴天`
   - row index: `1`
   - verified playback title: `天空仍灿烂`

This proves the Chinese fallback path can activate a visible row on the Chinese
result page and verify one concrete now-playing title through the AX tree
without screenshot OCR.

It does **not** prove that the current row fallback semantically selected
`晴天`. The current verified playback title is `天空仍灿烂`, so this case should
be read as an activation-path proof, not as title-selection proof.
