# `auv` Inspect Viewer — UI Kit

A speculative recreation of the **browser-based inspect viewer** described in
[`docs/ai/references/inspect/2026-05-19-trace-run-inspect-design.md`](https://github.com/moeru-ai/auv/blob/main/docs/ai/references/inspect/2026-05-19-trace-run-inspect-design.md)
from `moeru-ai/auv`.

**Important caveat:** This viewer does **not exist yet** in the repository. The
design doc specifies the data model, the HTTP/WebSocket endpoints, and the
viewer's load flow — but no UI code has been written. This kit is one
opinionated rendering of that contract, useful for explorations and as a
visual target for a future implementation.

## What's grounded

- **Layout vocabulary** — the design doc explicitly mentions a fixed left
  sidebar for run-list nav, a span-tree view, an events stream, and an
  artifact panel. This kit places all four.
- **Endpoints** shown on the connection bar are exactly the ones in
  `auv-cli inspect serve [--host --port]` with default `127.0.0.1:8765`.
- **Run/Span/Event/Artifact** shapes are lifted from the `v1alpha1`
  manifest specs in the design doc.
- **Status vocabulary** comes from `src/model.rs`
  (`RunStatus::{Completed, Failed}`) and the OpenTelemetry-compatible
  `status_code: unset | ok | error`.
- **Live-stream pulsing dot** mirrors the WebSocket live stream described
  in §10 of the design doc.

## What's invented

- All visual styling: layouts, padding, hover states, focus rings.
- The artifact thumbnail/preview pane — the doc says artifacts have metadata
  and a file path, but doesn't dictate how they render.
- The "filter by status" controls in the sidebar — implied, not specified.

## Files

- `index.html` — a working click-through with three runs and a live-streaming
  span tree.
- `Layout.jsx` — top status bar, sidebar, main pane, right rail scaffold.
- `Sidebar.jsx` — run list.
- `SpanTree.jsx` — collapsible indented span tree.
- `EventsRail.jsx` — events.jsonl tail.
- `ArtifactPanel.jsx` — selected artifact preview.
