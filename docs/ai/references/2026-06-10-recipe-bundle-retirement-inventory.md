# Recipe To Rust Orchestration Migration Inventory

Status: proposed, corrected after owner clarification; first TextEdit driver-boundary slice started

Scope: migration inventory for
`2026-06-10-rust-orchestration-recipes-bundles-retirement.md`.

Policy:

- `migrate`: implement Rust orchestration replacement before removing execution.
- `fallback`: keep JSON execution available until the entry migrates or is
  explicitly archived.
- `archive`: keep discoverable as historical proof material after it is no
  longer an active workflow.
- `delete`: remove when no durable reference value remains.
- `hold`: needs owner review before migration or removal.

The first implementation slice should migrate one small recipe from
`recipes/` into Rust-owned operation code while preserving existing CLI
behavior through the old compatibility path. The suggested exemplar is
`recipes/macos/textedit/create-and-verify-text.v0.json` because it has one case
matrix, a narrow workflow, and existing validation tests. Other recipes remain
`fallback` until they are migrated or explicitly archived.

Implementation note, 2026-06-10: `crates/auv-apple-textedit` now owns the
TextEdit operation contract and a `TextEditDriver` boundary. Its macOS adapter
can open `auv-driver-macos`, resolve the main TextEdit window, foreground the
window through typed input preparation, and paste through typed clipboard input.
It intentionally does not reimplement `debug.focusTextInput` or `verify.axText`
through root legacy modules. Those two steps require a public typed AX surface
in `auv-driver-macos` before the app-local operation can replace the CLI
entrypoint end-to-end. Any catalog command ids kept in the crate are
`LegacyRecipeStep` parity data for comparing against the old manifest, not the
Rust operation execution surface.

App-local replacement commands should use app-domain names:

- `crates/auv-apple-textedit`: `document write`, `document compare`,
  `document focus`.
- `crates/auv-apple-notes`: `note new`, `note write`, `note compare`,
  `note focus`.
- `crates/auv-qqmusic`: `search <query>` plus `search results select` and
  `search results click`.

| Path | Kind | Current entrypoint | Disposition | Replacement Rust owner | Approval |
| --- | --- | --- | --- | --- | --- |
| `bundles/native-app-skill-tree.v0.json` | bundle | `auv-cli skill bundle *`, bundle-backed `invoke` | fallback | none until covered commands migrate | owner clarification, 2026-06-10 |
| `recipes/macos/demo/dual-cursor-press-notes.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | fallback | later Rust macOS demo orchestration | owner clarification, 2026-06-10 |
| `recipes/macos/demo/dual-cursor-press-notes.cases.v0.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/demo/smart-press-cross-app.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | fallback | later Rust smart-press coverage orchestration | owner clarification, 2026-06-10 |
| `recipes/macos/demo/smart-press-cross-app.cases.v0.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/netease-cloud-music/play-visible-anchor.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | fallback | later Rust music orchestration | owner clarification, 2026-06-10 |
| `recipes/macos/netease-cloud-music/play-visible-anchor.cases.v0.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/notes/create-and-verify-note.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-apple-notes `note write` | owner clarification, 2026-06-10 |
| `recipes/macos/notes/create-and-verify-note.v1.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-apple-notes `note write` | owner clarification, 2026-06-10 |
| `recipes/macos/notes/create-and-verify-note.v2.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-apple-notes `note write` | owner clarification, 2026-06-10 |
| `recipes/macos/notes/create-and-verify-note.cases.v0.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/notes/create-and-verify-note.cases.v1.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/notes/create-and-verify-note.cases.v2.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/music.result.play.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-qqmusic `search results click --candidate-ref` | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/open-search-submit-query.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-qqmusic internal search phase for `search <query>` | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/play-search-result-candidate.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-qqmusic `search results click --candidate-ref` | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/play-search-result-candidate.cases.v0.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/play-visible-anchor.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-qqmusic `search results click <query> --anchor` | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/play-visible-anchor.cases.v0.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/play-visible-row.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-qqmusic `search results click <query> --row` | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/play-visible-row.v1.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-qqmusic `search results click <query> --row` | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/play-visible-row.cases.v0.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/play-visible-row.cases.v1.json` | case_matrix | `auv-cli skill cases run` | fallback | later Rust case data | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/search-ocr-anchor.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-qqmusic `search <query>` | owner clarification, 2026-06-10 |
| `recipes/macos/qqmusic/select-result-anchor.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-qqmusic `search results select <query> --anchor` | owner clarification, 2026-06-10 |
| `recipes/macos/textedit/create-and-verify-text.v0.json` | recipe | `auv-cli skill run`, bundle/recipe runtime | migrate | crates/auv-apple-textedit `document write <content> --replace --verify` | owner clarification, 2026-06-10 |
| `recipes/macos/textedit/create-and-verify-text.cases.v0.json` | case_matrix | `auv-cli skill cases run` | migrate | crates/auv-apple-textedit `document write <content> --replace --verify` | owner clarification, 2026-06-10 |
| `recipes/scan/list-item-candidate-continue-hook.v0.json` | recipe | scroll-scan recipe hook | fallback | later typed `auv-tracing-interaction` hook | owner clarification, 2026-06-10 |

README files under `recipes/**/README.md` remain documentation artifacts and do
not define executable manifest entries.
