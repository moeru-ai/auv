# Phase 3 — Mainline Compliance Audit

Date: 2026-05-22

Status: snapshot audit (compliance against
`2026-05-22-phase-3-mainline-acceptance.md`)

## Why this exists

Pairs with the mainline acceptance doc. Each recipe in the repo is
rated against each of the seven rules. Rules 1, 2 are code-gates as
of commit `72ef671`; the rest are doc/review-gates. This audit is
the source of truth for which exemptions are active and which
recipes are out of compliance.

The audit is a snapshot, not a contract. Re-run it whenever a recipe
is added, edited, or its status changes.

## Recipe-by-rule matrix

13 recipe manifests in `recipes/macos/`. Status pulled from each
manifest's top-level `status`; smart-press steps detected by
`command_id == "debug.smartPress"`.

| Recipe | Status | Rule 1 | Rule 2 | Rule 3 | Rule 4 | Rule 5 | Rule 6 | Rule 7 |
|---|---|---|---|---|---|---|---|---|
| `macos.demo.dual_cursor_press_notes.v0` | experimental-recipe | n/a (no smartPress) | n/a | review needed | n/a (demo) | n/a | n/a | grandfathered |
| `macos.demo.smart_press_cross_app.v0` | experimental-recipe | **pass** (demo namespace) | pass (no validated cases) | n/a (demo) | n/a (demo) | n/a | n/a | grandfathered |
| `macos.netease_cloud_music.play_visible_anchor.v0` | needs-revalidation | n/a | n/a | review needed | **review** — recipe predates Rule 4; status already flagged for revalidation | n/a | n/a | grandfathered |
| `macos.notes.create_and_verify_note.v0` | experimental-recipe | n/a | n/a | pass (verifyAxText cites `ax.matched_text`) | n/a | n/a | n/a | grandfathered |
| `macos.notes.create_and_verify_note.v1` | experimental-recipe | n/a | n/a | pass | n/a | n/a | n/a | grandfathered |
| `macos.notes.create_and_verify_note.v2` | experimental-recipe | n/a | n/a | pass | n/a | n/a | n/a | grandfathered |
| `macos.qqmusic.open_search_submit_query.v0` | validated-recipe | n/a | n/a | review needed | grandfathered (Phase 1 original) | n/a | n/a | grandfathered |
| `macos.qqmusic.play_visible_anchor.v0` | validated-recipe | n/a | n/a | pass (`debug.findImageText` is contract) | grandfathered (Phase 1 original) | n/a | n/a | grandfathered |
| `macos.qqmusic.play_visible_row.v0` | experimental-recipe | n/a | n/a | pass (`debug.verifyNowPlayingTitle`) | grandfathered (Phase 1 original) | n/a | n/a | grandfathered |
| `macos.qqmusic.play_visible_row.v1` | experimental-recipe | **pass** (active exemption — see below) | **pass** (all cases candidate) | pass (`debug.verifyNowPlayingTitle`) | **pass** (Phase 3 #6 boundary cited in objective) | n/a | n/a | grandfathered |
| `macos.qqmusic.search_ocr_anchor.v0` | validated-recipe | n/a | n/a | pass (`debug.findScreenText` + `expect.signal_equals`) | grandfathered | n/a | n/a | grandfathered |
| `macos.qqmusic.select_result_anchor.v0` | validated-recipe | n/a | n/a | pass | grandfathered | n/a | n/a | grandfathered |
| `macos.textedit.create_and_verify_text.v0` | experimental-recipe | n/a | n/a | pass (verifyAxText) | n/a | n/a | n/a | grandfathered |

Legend:
- **pass** — explicitly compliant
- n/a — rule doesn't apply to this recipe
- review needed — manual review required, not yet completed in this audit pass
- grandfathered — predates the rule, no enforcement today

## Active exemptions (Rule 1)

Every step-level `mainline_exemption` currently in the repo. If this
list grows beyond ~3 entries without a corresponding shrinkage, the
exemption mechanism is being abused.

| Recipe | Step | Category | Reason summary |
|---|---|---|---|
| `macos.qqmusic.play_visible_row.v1` | `press-result-play` | `experiment` | Phase 3 #6 controlled measurement of whether QQ音乐 play control is AX-pressable; explicit fork of validated v0; all cases stay candidate per Rule 2. |

**One exemption total.** If a new smart-press recipe needs an
exemption, add it here at the same time and justify why a demo-
namespace variant won't do.

## Rule 5 — inspect serve mutation surface

Manual scan of `src/inspect_server.rs` routes:

```
GET  /
GET  /runs
GET  /runs/{run_id}
GET  /runs/{run_id}/spans
GET  /runs/{run_id}/events
GET  /runs/{run_id}/artifacts
GET  /runs/{run_id}/artifacts/{artifact_id}
GET  /runs/{run_id}/stream    (WebSocket)
GET  /assets/{name}
POST /write/runs/{run_id}/updates   (in-process inspect server writes
                                     only; not a remote control
                                     endpoint)
```

The `POST /write/...` route is internal: the CLI process pushes
recorded run updates to a local inspect_server it spawned. It is
**not** a remote mutation surface for triggering new runs or
executing AUV commands; it only ferries already-recorded canonical
records into the live viewer. **Compliant with Rule 5 today.**

If Rule 5 enforcement gets stricter (e.g. "no POST at all"), this
route would need either a tighter scope or a rename. For now,
flagged as compliant-with-note.

## Rule 6 — YOLO / realtime tracking

Scan: no CV detector, no realtime tracking loop, no YOLO/EfficientDet
import anywhere in `src/`. **No drift today.**

## Rule 7 — probe provenance (the big gap)

**13 of 13 recipes lack a `provenance` field.** All are grandfathered
because the field didn't exist before this audit.

The retroactive fix path, in order of cost:

1. **Cheapest**: every recipe gets `provenance: { source:
   "phase-1-grandfathered" }` as a literal marker. Costs ~13 small
   JSON edits, no behaviour change. Establishes the field shape so
   new recipes are forced to declare real provenance.
2. **Honest**: for each grandfathered recipe, find a real probe
   artifact (or run record) from the era when it was first written
   and link it. Higher cost; produces an actual provenance trail.
3. **Strict**: turn Rule 7 into a code-gate (required field at
   manifest validation). Forces the path above to complete.

Recommended sequence: ship (1) in a follow-up `chore(recipes)`
commit so the field shape is real; do (2) opportunistically per
recipe; defer (3) until (1) lands and a quiescent moment exists to
absorb the code-gate.

## Drift watch — recipes that need re-examination soon

- **`macos.qqmusic.play_visible_row.v1`**: holds the only active
  Rule 1 exemption. The exemption category is `experiment`; once
  hands-off evidence accumulates (per the recipe's own promotion
  rule), either spawn a non-smart child recipe and retire v1, or
  document why v1 should stay long-term.
- **`macos.netease_cloud_music.play_visible_anchor.v0`**: marked
  `needs-revalidation` — that label is older than this audit.
  Either revalidate or move it to a more honest status.
- **`macos.demo.dual_cursor_press_notes.v0`**: de-promoted to
  candidate after a hands-off replay failure. Rule 3 review
  pending — confirm that no claim in the recipe or its bundle
  membership cites overlay events as proof.

## How to use this doc

When you add or change a recipe:

1. Add a row to the matrix above (or update its row).
2. If you add a step-level `mainline_exemption`, add it to the
   "Active exemptions" table.
3. If you add a route under `inspect_server`, update Rule 5 scan.
4. If you add a CV detector, add target/detector/verification specs
   per Rule 6.
5. If you add a `provenance` field, drop the recipe from the Rule 7
   gap list.

If you find an existing recipe that the matrix lies about, fix the
matrix and link the discrepancy.

## What this doc does NOT do

- It does not run the gates. The gates live in
  `validate_skill_manifest_with_commands` and
  `validate_case_matrix_against_skill` (commit `72ef671`); this is
  a paper snapshot, not a checker.
- It does not retroactively kill any recipe. The
  needs-revalidation netease recipe and the dual-cursor demo demote
  stay as Codex/the user left them.
- It does not gate Codex's screenshot-first inspect / overlay
  evidence work. That work is mainline-aligned (it powers the
  inspect/replay step at the end of the mainline arrow) and is not
  in scope for these rules.
