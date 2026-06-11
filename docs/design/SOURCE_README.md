# AUV Design System

A visual + interaction system for **AUV** — a Rust CLI for turning
application UI workflows into inspectable, replayable operations.

AUV is **one project among several** under the
[Moeru AI](https://github.com/moeru-ai) umbrella — an online hobby group
exploring the intersection of **Moe (萌え) + AI**. Sibling projects in the
same org include
[`airi`](https://github.com/moeru-ai/airi) (virtual AI persona, targeting a
Neuro-sama recreation),
[`plast-mem`](https://github.com/moeru-ai/plast-mem) (memory layer for cyber
waifus),
[`xsai`](https://github.com/moeru-ai/xsai) (extra-small AI SDK),
and [`citrus`](https://github.com/moeru-ai/citrus) (inactive). This design
system is AUV-specific, but it visually inherits the Moeru AI parent
identity (cyan + lime, pixel-art accents).

Source repos used to build this system:

- [`moeru-ai/auv`](https://github.com/moeru-ai/auv) — primary source of truth (Rust runtime, CLI, recipes, app-local commands, docs).

The reader is encouraged to skim those before designing anything substantial
against AUV. The repository does not ship a UI; the design language here is
*derived* from terminal output, JSON manifest shapes, and the planned
browser-based viewer described in
[`docs/ai/references/2026-05-19-trace-run-inspect-design.md`](https://github.com/moeru-ai/auv/blob/main/docs/ai/references/2026-05-19-trace-run-inspect-design.md).

---

## 1. Product Context

AUV is **not** a generic LLM agent and **not** a CLI wrapper.

It is a recording + replay runtime: an explicit execution model that turns
ad-hoc UI automation into:

| Artifact | What it is | Where it lives |
|---|---|---|
| **Recipe** | A JSON manifest of steps against a target app | `recipes/macos/<app>/<recipe>.v0.json` |
| **Case matrix** | Validated/candidate inputs that exercise a recipe | `recipes/macos/<app>/<recipe>.cases.v0.json` |
| **Trace / Run** | The recording of one workflow execution | `.auv/runs/<run_id>/` |
| **Span / Event / Artifact** | OTLP-shaped records inside a run | `.auv/runs/<run_id>/{spans,events,artifacts}.jsonl` |

### Current surfaces

1. **`auv-cli`** — the user-facing executable. Subcommands:
   `list-commands`, `list-drivers`, `app probe|analyze|distill|validate`,
   `invoke`, `inspect`, `inspect serve`, `skill list|show|run`,
   `skill cases list|show|report|run`.
2. **`auv-cli inspect serve`** — a read-only HTTP + WebSocket inspect server
   that surfaces stored and live run data. Default endpoint
   `127.0.0.1:8765`. The browser viewer that consumes it is **not yet
   implemented** but is fully designed in `2026-05-19-trace-run-inspect-design.md`.
3. **macOS driver** — the only platform-native driver shipped today.
   Currently validated against QQ音乐, Notes, and TextEdit.

### Phase-1 freeze (2026-05-18)

The product is intentionally narrow. Phase-1 is **frozen**:

- macOS runtime + driver + recipe + case-matrix flow exists; the former bundle
  package flow has been retired.
- QQ音乐 has two validated narrow playback strategies (OCR-anchor + row fallback).
- Notes + TextEdit ship as native-app AX-text samples.
- The unresolved boundary — Chinese requested-title semantic selection in the
  row-fallback path — is recorded explicitly as `semanticSelectionStatus =
  not-validated`, not hidden.

This design system reflects that culture: status is always declared, never implied.

---

## 2. Index

| File | What's in it |
|---|---|
| `README.md` | This document. |
| `colors_and_type.css` | All design tokens (colors, type, spacing, radii, shadows, motion). |
| `SKILL.md` | Agent Skill manifest — load this skill in Claude Code or as guidance. |
| `assets/` | Logo mark + wordmark (pixel-art), sparkle accent, parent-org Moeru AI logo. |
| `preview/` | One-card-per-concept previews populating the **Design System** tab. |
| `ui_kits/cli/` | High-fidelity recreation of `auv-cli` terminal output. |
| `ui_kits/viewer/` | Mock of the unbuilt browser viewer described in `trace-run-inspect-design.md`. |

---

## 3. Content Fundamentals

AUV documentation has a strong, recognizable voice. It is one of the most
*aggressively honest* engineering tones in any modern repo. Match this register
in every artifact this system produces.

### Voice attributes

- **Anti-marketing.** No persuasion, no superlatives, no "powerful" or
  "seamless". Every claim is qualified.
- **Boundary-first.** Every spec lists what is **not** proven alongside what
  is. The reader is given the failure mode before the success mode.
- **Provisional naming.** Names like `v1alpha1`, "phase-1", "candidate" are
  used to flag instability. Do not stabilize names too early.
- **Lowercase command identifiers.** `recipe`, `case matrix`, `run`,
  `span`, `event`, `artifact` are nouns; `probe`, `analyze`, `distill`,
  `validate`, `invoke`, `inspect` are verbs. Reuse them.

### Casing

- **Headings, prose:** sentence case. Never title case.
- **CLI commands and recipe IDs:** lowercase with dots and underscores.
  `macos.qqmusic.play_visible_anchor.v0`.
- **Run IDs:** literal monospace strings, e.g. `run_1778947574511_68037_4`.
- **Status vocabulary:** lowercase, hyphenated. `validated`,
  `not-validated`, `candidate`, `phase-1-frozen`, `running`, `failed`, `ok`.

### Pronouns + register

- **"This recipe does not …"**, **"This run does not claim …"** — the artifact
  is the subject, not the user. Avoid "you".
- Imperatives are reserved for operational instructions
  (`Run formatting and tests before submitting changes that touch Rust code.`).

### Example phrasings — verbatim from the repo

> "Phase 1 is frozen. That does **not** mean every behavior is solved."
>
> "The remaining failures are explicit boundaries, not hidden contradictions."
>
> "Do not twist this freeze into false product claims."
>
> "If phase 2 immediately collapses back into 'let's just chase one more OCR
> edge case', that is not progress. That is avoidance."
>
> "This is a freeze of scope, not a claim that every QQ音乐 edge case is
> solved."

### Emoji / icons in prose

- **No emoji.** Anywhere. Not in headers, not in callouts, not in CLI output.
- **Unicode disc characters** (`●`, `○`, `◐`) are used as status sigils inside
  the visual system, never in prose.
- Mermaid graphs are used to describe trees (see
  `2026-05-17-auv-native-app-skill-tree.md`).

---

## 4. Visual Foundations

There is no existing visual brand to inherit; AUV ships zero pixels. This
section *establishes* the system based on the project's culture.

### Color vibe

- **Two ground tones:** warm paper (`--auv-paper`, `#f6f5f1`) for docs +
  prose; deep ink (`--auv-shell`, `#0e1013`) for terminal-grade surfaces.
  Both ship; the viewer is dark-by-default, prose is light-by-default.
- **No gradients.** A CLI tool. Flat surfaces with hairline borders.
- **Two brand hues** — `--auv-brand: #00c4d2` (moeru cyan) +
  `--auv-brand-2: #7fd030` (moeru lime). Sampled from the
  [Moeru AI org logo](https://github.com/moeru-ai). Cyan is the single
  primary-action color; lime accents validated states, sparkles, and the
  pixel-art logo's bottom half.
- **Status-coded everything.** Validated green, candidate amber, boundary
  rose, frozen slate, running cyan-teal, failed red. These map 1:1 onto
  recipe/case-matrix JSON fields and the OTLP `status_code`.

### Type

- **`Geist`** (sans, 300–700) — UI prose, headings, navigation.
- **`JetBrains Mono`** (mono, 400–700) — IDs, run IDs, code, JSON,
  status pills, span names.
- **`Geist Mono`** — mono used in UI chrome (smaller chip text, table
  headers) where JetBrains Mono is too dense.
- **`Silkscreen`** (pixel, 400/700) — the moeru-ai pixel-art accent.
  Used only on the wordmark, hero accents, and section dividers. **Never
  body text.** It is functionally legible only above ~13px.
- Headings are mid-weight (500), never extra-bold. The wordmark uses
  hand-drawn pixel rects rather than a web-font glyph so the mark renders
  consistently even when Silkscreen has not loaded.

### Backgrounds

- Solid colors only. **No imagery, no full-bleed photos, no illustration in
  product surfaces.** The one inherited motif from the parent org is
  **pixel-art**: see `assets/logo-mark.svg` and `assets/sparkle.svg`. Pixel
  sprites may appear as small accents in the viewer's empty states, hero
  surfaces, or `auv-cli` welcome banners — but **never inside data views**
  (span trees, JSON, terminal output).
- Repeating textures: a **1px hairline grid** is acceptable on empty
  inspect-viewer canvases (1px `#e6e4dc` every 24px). A second permitted
  texture is the **pixel-checkerboard placeholder** used in the viewer's
  artifact preview (2px squares in `--auv-shell-2` / `--auv-shell-3`).

### Animation

- **Linear-ish ease** — `cubic-bezier(0.2, 0.0, 0.2, 1)`. No bounces, no
  springs.
- **140ms** default duration, **80ms** for chrome feedback, **220ms** for
  larger surfaces.
- The only animated primitive is `auv-pulse`: a 7px disc, 1.2s loop, used on
  the **running** status pill while a live run streams via WebSocket.
- Page transitions: none. The viewer mounts and renders in place.

### Hover / press states

- **Hover:** swap to one step darker on neutral surfaces
  (`--auv-paper-2` recessing). No glow, no underline, no scale.
- **Press:** `transform: translateY(0.5px)` only; color does not change on
  press. Buttons feel mechanical, like a key click.
- Links: solid `--auv-brand`, underline on hover only.

### Borders + shadows

- **Hairline-first.** `1px solid var(--auv-paper-line)` is the default
  card border. Shadows are used **only** for popovers + dropdowns.
- Three shadow steps total: `--auv-shadow-1` (cards lifted off the page),
  `--auv-shadow-2` (panels), `--auv-shadow-pop` (popovers / context menus).
- No inset shadows.

### Transparency + blur

- **Avoid.** This is a forensic inspection tool; the user must trust pixel
  values they see. No backdrop-filter, no glassy overlays.
- The one exception: the live-run streaming overlay in the viewer may use a
  4% black wash to dim the underlying span while a span is `state: running`.

### Corner radii

- `2px` — pills, status chips
- `4px` — inputs, small buttons
- `6px` — cards, terminal blocks (default)
- `8px` — large content cards
- **No fully-rounded cards.** Pill buttons are reserved for the status
  vocabulary, nothing else.

### Layout rules

- **Fixed grid:** 24px column gutter, 16px row rhythm.
- **Hairline dividers** separate sibling sections rather than whitespace
  alone — this echoes the JSONL log shape AUV emits.
- The viewer uses a **fixed left sidebar** (256px) for the run list and a
  **fixed top status bar** (44px) for the connection state to
  `127.0.0.1:8765`.
- Long IDs are never truncated visually without a tooltip; truncation
  happens at the *middle* of the string (so the trailing sequence number
  stays visible).

### Cards

- White (or `--auv-shell-2`) fill, 1px hairline, 6px radius, no shadow.
- Header row uses `--auv-meta` (uppercase mono micro-caps) for the artifact
  kind label, then the ID in mono.
- A status pill always sits on the **right** of the header row.
- Card padding: `16px 20px`.

### Imagery

- The project has no photography, no people, no illustration. If a marketing
  surface needs imagery, prefer:
  - Screenshots of real terminal output (use the `ui_kits/cli` recreations).
  - A mermaid skill-tree diagram (the `skill-tree.md` reference is canonical).
  - Architectural diagrams in plain SVG, monochrome.

---

## 5. Iconography

AUV ships **no icons**. There is no icon font, no SVG sprite, no Lucide
import in the repo.

The closest thing to an iconographic system is the **status sigil**:

| Sigil | Meaning | Token |
|---|---|---|
| `●` filled | validated | `--auv-validated` |
| `◐` half | candidate / pending | `--auv-candidate` |
| `○` open | not-validated / boundary | `--auv-boundary` |
| `■` filled square | frozen / locked | `--auv-frozen` |
| `●` pulsing | running / live stream | `--auv-running` |
| `×` cross | failed | `--auv-failed` |

These are rendered as colored CSS pseudo-elements (see `.auv-status`), not
glyphs from a font. Match this approach instead of importing an icon set.

### When iconography is needed in the viewer

The browser viewer (when built) will need a small functional icon set for:

- run-list navigation (chevrons, copy-link)
- span-tree expand/collapse
- artifact-type indicators (image / json / log)
- the `inspect serve` connection state

For these, **substitute Lucide** (`https://unpkg.com/lucide-static`) at
stroke-width 1.5, 16px, current-color. **Flag this substitution to the
user — the repo has not standardized an icon set.** Lucide is a stand-in.

### Emoji

Do not use emoji. The repo contains none in code, commits, or docs. The
only non-ASCII characters that appear are Chinese characters in app names
and test queries (`QQ音乐`, `周杰伦`, `晴天`).

---

## 6. Caveats + flagged substitutions

- **AUV ships zero pixels of UI.** The brand here was reverse-engineered
  from the parent org's pixel-art logo + the AUV repo's voice. The cyan
  `#00c4d2` and lime `#7fd030` are sampled from
  [`moeru-ai/.github/moeru-ai.svg`](https://github.com/moeru-ai/.github);
  the AUV pixel mark in `assets/logo-mark.svg` is a derivative I drew, not
  an asset from the org.
- **Fonts** — `Geist`, `JetBrains Mono`, and `Silkscreen` are all open-
  licensed via Google Fonts. If the project owner prefers different
  families (Berkeley Mono, IBM Plex, Press Start 2P), swap in
  `colors_and_type.css`.
- **Icons** — Lucide is a CDN stand-in. If/when the project standardizes,
  swap.
- **The viewer UI kit** (`ui_kits/viewer/`) recreates a UI that **does not
  exist yet**. It is faithful to the design doc but speculative on visual
  detail.

If the project owner wants different brand colors, a different mark, or a
different type system, those should override every choice in
`colors_and_type.css`.
