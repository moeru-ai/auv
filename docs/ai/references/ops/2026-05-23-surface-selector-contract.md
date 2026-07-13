# Surface Selector Contract

Date: 2026-05-23

Status: contract draft, v0 scope only

## Why This Exists

`scroll scan` is important, but it is only one way to produce candidates. AUV
needs a selector-like grounding layer so agents can ask for stable targets
without guessing coordinates from screenshots.

The shape is:

```text
candidate query -> surface selector backend -> candidates -> action -> verification
```

The output is still the existing `Candidate` inside `OperationResult`. This doc
does not introduce a second candidate object.

## Boundary

Do not call this a DOM selector. DOM is one possible backend. AUV also needs to
work with native AX, OCR, visual rows, opaque WebViews, keyboard shortcuts, and
future detector outputs.

The v0 Rust contract intentionally supports only:

- `ax`: role / label / path / enabled / visible constraints
- `ocr`: text anchor + optional region + provider score threshold
- `row`: row index / contains text / optional region

Reserved for later docs and implementations:

- `dom`: CSS / XPath / ARIA / CDP snapshot
- `visual`: icon / template / detector class
- `command`: shortcut / menu item / media key

Those are not v0 backends. Listing them here is a design boundary, not an
implementation promise.

## Contract Sketch

```json
{
  "query_id": "play-control",
  "selector": {
    "any_of": [
      { "source": "ax", "role": "AXButton", "label": "播放", "enabled": true },
      { "source": "ocr", "text": "播放", "min_provider_score": 0.75 },
      { "source": "row", "row_index": 1 }
    ],
    "within": "target_window",
    "require_visible": true
  },
  "output_kind": "button",
  "known_limits": []
}
```

Resolution returns:

```text
OperationResult {
  output: Candidates([...])
}
```

The resolved candidates carry grounding and evidence through `Candidate`:

- `target_spec.grounding`: `ax_node`, `ocr_anchor`, or `visual_row`
- `target_spec.anchor_text`: OCR-like text anchor when available
- `target_spec.region_hint`: normalized region when available
- `target_spec.row_index`: 1-based row index for row-grounded candidates
- `evidence.observation`: provider-native details such as bounds, raw OCR
  fragments, source row report, and provider score
- `liveness.preconditions`: what must still hold before action
- `control.requires_*`: what the action needs at execution time

## Confidence Boundary

Do not add top-level `Candidate.confidence` in v0.

Provider outputs may expose `min_provider_score` in selectors and raw
provider scores inside `evidence.observation`. Those numbers are not semantic
truth. A candidate is useful because it has evidence and re-checkable liveness
preconditions, not because a single numeric confidence looks precise.

## Relationship To Observed Collection

Observed collections are evidence artifacts from scroll scans. Surface
selectors are candidate queries.

For list-like opaque UIs:

```text
scroll scan / visual row detector -> observed collection
observed collection / row selector -> Candidate
CandidateRef -> action -> VerificationResult
```

Neko's visual row boxes fit here as a row candidate producer. The green boxes
are not semantic results by themselves. They must be wrapped as observed rows or
candidate evidence before an agent can safely act on them.

## First Implementation Path

1. Keep QQ音乐 `music.search.results` as the first real candidate producer.
2. Represent visual-band rows as `TargetGrounding::VisualRow` with
   `target_spec.row_index`.
3. Keep OCR rows as `TargetGrounding::OcrAnchor` with anchor recheck.
4. Add no DOM/CDP/YOLO backend until the row/AX/OCR contract survives real
   validation.

## Non-Goals

- No universal selector engine in v0.
- No semantic app state adapter in v0.
- No training or detector model integration in v0.
- No new action semantics; actions still consume `Candidate`/`CandidateRef` and
  verify separately.
