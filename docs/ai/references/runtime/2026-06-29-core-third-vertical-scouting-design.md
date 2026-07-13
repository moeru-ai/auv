# 2026-06-29 AUV Core-X1 third-vertical scouting design

Date: 2026-06-29

Status: design-only scouting and admissibility framing. No implementation, no
proof-matrix verdict upgrades, no helper extraction.

Branch: `docs/core-x1-third-vertical-scouting`

## Purpose

Core-A3 closed the stage-status triad helper line (`auv-stage-status`). Core-A4
re-confirmed rows **69** (quality measurement verdict) and **70** (persisted
backend label discipline) as **`candidate, not admissible yet`** on the main
proof matrix. Two verticals — Minecraft MC-10..17 and osu full chain — now
supply probe-local recurrence, but **not** third-donor semantic triangulation for
`metric_partial` semantics or render/quality `backend` enum discipline.

Core-X1 scouts repo-native candidates for a **third vertical consumption donor**
without treating any candidate as an existing donor. The output is:

- ranked shortlist with honest substrate audit
- elimination matrix for hard rejects
- falsifier gate (X-F1..X-F6) before Core-X2 implementation
- pointer to MVP proof contract (companion doc)

Companion MVP contract (Balatro path-to-donor only):
[`2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md`](2026-06-29-core-third-vertical-admissibility-mvp.md)

## Non-goals

This note does **not**:

- implement a third vertical or open `Core-X2-*` probes
- extract Core-A5a (quality verdict) or Core-A5b (backend label) helpers
- open Core-B enum graduation, Core-C2, MC-20, or controller/planner slices
- upgrade proof-matrix verdict columns (footnote⁶ forward pointer only)
- treat Balatro, STS, or any scout as an existing third donor today
- re-open the archived macOS AX copilot as the active product lane
- judge Netease Music as a bad vertical — only as **wrong lane** for rows 69/70

## Why now (evidence anchor)

| Row | State after Core-A3 / Core-A4 |
| --- | --- |
| 65 Stage status triad | `helper-only admissible` — extracted (`auv-stage-status`) |
| 69 Quality verdict | `not admissible` — F3 triggered (`metric_partial` semantics diverge) |
| 70 Backend discipline | `not admissible` — query-half only on osu; render/quality enum MC-only |

Core-A4 lists **third vertical** as a reopen trigger for row 69
([`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-30-query-readiness-closeout.md)
L173). Core-A2 falsifier review notes **coincidence risk (F5)** until a third
donor exists
([`2026-06-27-auv-core-a-query-readiness-falsifier-review.md`](2026-06-30-query-readiness-closeout.md)
L145; full-chain re-review in
[`2026-06-28-auv-core-a2-full-chain-falsifier-review.md`](2026-06-30-query-readiness-closeout.md)).

```text
Minecraft MC-10..17 ──┐
                      ├── rows 69 / 70 stuck (two donors, semantic gap)
osu full chain ───────┘
Core-X1 scout ──► third vertical candidate ──► row 69 / 70 / Core-C1 pressure
```

## Scoring rubric

Score each candidate **Low / Medium / High** on:

| Dimension | What it tests |
| --- | --- |
| **R69 pressure** | Can witness→quality produce a third `metric_partial` **semantic** datapoint (not label-only)? |
| **R70 pressure** | Can it persist a stable `render_backend` or `quality_backend` enum (not free strings)? |
| **R67 pressure** | Dual-backend spatial query compare (bonus; not primary X1 goal) |
| **Core-C1 pressure** | Distinct live admission substrate vs osu playfield / MC window-readiness |
| **Artifact substrate** | Committed fixtures, manifests, run-store roles already in repo |
| **Honest MVP scope** | Can MVP avoid controller/planner/OCR mega-runtime as center? |
| **Vertical independence** | Truly third domain (not MC-18 sibling provider, not osu clone) |

**Hard disqualifiers** (auto-eliminate):

- Requires controller / planner / autoplay policy as center
- No persisted artifact chain (CLI-only toy)
- Generic agent platform without consumption-pattern stages
- Archived vertical re-opened as full product lane without owner naming
- Same-vertical provider seam only (e.g. MC-18 closed-scene toy)

## Candidate inventory

Repo-native candidates only. **Donor vs scout** language is mandatory: a scout
has substrate; a donor has closed consumption-chain evidence on the proof matrix.

### Balatro (`crates/auv-game-balatro/`)

| Aspect | Repo state |
| --- | --- |
| Crates / docs | `auv-game-balatro` crate; [`README.md`](../../../../crates/auv-game-balatro/README.md); CLI object-oriented surface |
| Substrate today | `detector`, `observation`, `model`, `card_corner`, `cache`, `cli`; setup manifest; Hugging Face–resolved ONNX; macOS live capture + `--verify` on mutating commands |
| Consumption stages | **None** — no semantic gate, spatial query, witness, or quality manifests |
| Operation wire | [`operation.rs`](../../../../crates/auv-game-balatro/src/operation.rs) **placeholder** — opaque stubs; owner-deferred operation slice |
| Artifact roles | Observation frames, detection JSON, CLI `--verify` evidence; **not** run-store consumption manifests |
| X1 verdict | **Primary scout candidate** — **most donor-like**, **not** existing donor |

**Gap vs MC / osu donors:**

| Layer | MC / osu (donors) | Balatro (today) |
| --- | --- | --- |
| Semantic gate | Persisted manifests + stage triad | **Missing** |
| Spatial query | Target-conditioned answers | **Missing** |
| Witness / quality | WQ1 / MC-16–17 artifacts | **Missing** |
| Substrate | Mature run-store roles + fixtures | Detector + observation + CLI + verify only |
| Operation wire | Real operation-result chain | `operation.rs` placeholder — deferred |

| Dimension | Score | Notes |
| --- | --- | --- |
| R69 | **High** | Card/detection eval can bind witness→quality without play policy |
| R70 | **High** | Room for `quality_backend` / detector enum distinct from MC `external_command` |
| R67 | **Low** | No dual-backend compare seam planned |
| Core-C1 | **Medium** | macOS window targeting available; not required for 69/70 |
| Artifact substrate | **High** | Live + image observation, committed tests, setup manifest |
| Honest MVP | **High** | Observe-only consumption path avoids SKILL.md / planner center |
| Vertical independence | **High** | Distinct game UI domain |

### Slay the Spire (STS)

| Aspect | Repo state |
| --- | --- |
| Anchor | [`2026-06-06-game-slay-the-spire-observe-only-recognition-fixture-boundary.md`](../apps/game-observe/2026-06-06-game-slay-the-spire-observe-only-recognition-fixture-boundary.md) |
| Substrate | Docs + fixture boundary; **no** `auv-game-sts` crate |
| Consumption stages | Recognition → detector-recognition artifact only; **no** semantic/query/witness/quality |
| X1 verdict | **Runner-up narrative only** — greenfield; weaker short-term R69/R70 pressure |

| Dimension | Score | Notes |
| --- | --- | --- |
| R69 | **Low** | Would need full chain build from recognition boundary |
| R70 | **Low** | No backend enum surface yet |
| R67 | **Low** | N/A |
| Core-C1 | **Low** | Observe-only boundary forbids action chain |
| Artifact substrate | **Medium** | Fixture boundary defined; minimal committed fixtures |
| Honest MVP | **High** | Observe-only scope is honest |
| Vertical independence | **High** | Distinct domain |

### Netease Music (`crates/auv-netease-music/`)

| Aspect | Repo state |
| --- | --- |
| Substrate | View-parser, scroll, transport; core-lane app automation |
| Consumption stages | **No** — app/runtime/view-parser lane, not spatial-result consumption |
| X1 verdict | **Eliminate from X1 third-donor set** — wrong lane for rows 69/70 |

**Aside (not a quality judgment):** Netease remains valuable for
app/runtime/view-parser convergence. It does **not** pressure proof-matrix rows
69/70 or the witness→quality semantic split. No Doc-2 MVP contract.

### Archived AX copilot (`docs/archive/verticals/ax-copilot/`)

Frozen admission + semantic verify proof on a **different seam** (not MC-17
quality). Per [`CLAUDE.md`](../../../../CLAUDE.md), reference for Core-C1 only —
**eliminate** as third consumption donor.

### MC-18 closed-scene toy (`docs/ai/references/apps/minecraft/2026-06-27-minecraft-probe-18-reference.md`)

Second **Minecraft** query provider sibling to MC-15. Helps row **67**
(provider compare) within the same vertical — **eliminate** as third donor.

### OCR / recipe mega-runtime

Netease recipes, window OCR plans, legacy skill surfaces — no consumption-pattern
stages, controller/OCR as center — **eliminate**.

## Elimination matrix

| Candidate | Hard reject reason |
| --- | --- |
| Netease Music | Wrong lane (app/runtime/view-parser); does not pressure rows 69/70 |
| Archived AX copilot | Archived vertical; different seam; not consumption-pattern donor |
| MC-18 closed-scene toy | Same vertical (Minecraft); provider sibling only |
| OCR / recipe mega-runtime | No persisted consumption chain; mega-runtime center |
| osu clone / extension | Same vertical as existing second donor |
| Generic agent platform | No staged semantic→query→witness→quality artifacts |

**Not eliminated (ranked):** Balatro (primary scout), STS (runner-up narrative).

## Ranked shortlist

### 1. Balatro observe-only consumption path (primary scout)

**Framing:** Balatro is **not** donor #3 today. It is the **shortest honest path
to building donor #3** given existing detector, observation, CLI `--verify`, and
setup manifest substrate.

**Tradeoffs:**

- **Pro:** Repo-native crate; row 69/70 pressure reachable without greenfield
  crate; observe-only MVP can defer play policy (`SKILL.md` out of scope)
- **Con:** Full semantic→query→witness→quality chain **must be built** in
  Core-X2; `operation.rs` remains placeholder unless live admission is in scope

### 2. Slay the Spire observe-only (runner-up)

**Framing:** Narratively clean recognition→lineage boundary, but **greenfield**
for consumption stages. Weaker short-term R69/R70 pressure than Balatro.

**Tradeoffs:**

- **Pro:** Turn-based UI; honest observe-only fixture boundary already documented
- **Con:** No game crate; would need new artifact producers before matrix pressure

## Recommended scout target

**Owner-facing recommendation:** pursue **Balatro observe-only consumption path**
as Core-X2 default, with **STS** as deferred alternative if owner rejects Balatro
substrate or wants a cleaner but slower greenfield.

**Explicit non-claims:**

- Balatro is **not** an existing third donor
- STS is **not** shortlisted for short-term row 69/70 unlock
- Netease is **not** demoted as a vertical — only excluded from this donor set

**Build gap (Core-X2, not X1):** semantic/query manifests, witness/quality
producers, persisted `quality_backend` enum; replace `operation.rs` placeholders
only if live admission is owner-approved.

See MVP proof contract:
[`2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md`](2026-06-29-core-third-vertical-admissibility-mvp.md)

## Falsifier gate (scouting itself)

Before Core-X2 implementation is approved, this scouting design must pass:

| Falsifier | Fail condition | X1 assessment |
| --- | --- | --- |
| **X-F1** | Candidate cannot reach witness **or** quality **or** backend enum without controller | **Pass** — Balatro MVP is observe/fixture-centered; controller not required for 69/70 |
| **X-F2** | Candidate is same-vertical variant (MC provider, osu clone) | **Pass** — Balatro is third game domain |
| **X-F3** | MVP would duplicate osu/Minecraft without new contract pressure | **Pass** — targets `metric_partial` semantics + `quality_backend` enum gap |
| **X-F4** | No committed fixture path for regression | **Pass** — Balatro has image tests + setup manifest; Core-X2 must add consumption fixtures |
| **X-F5** | Candidate helps row 69/70 only by label collision, not semantic triangulation | **Pass (design intent)** — MVP contract requires explicit partial-metric policy |
| **X-F6** | Doc treats scout as existing donor | **Pass** — Balatro labeled scout / path-to-donor throughout |

## Cross-links

| Document | Relationship |
| --- | --- |
| [`2026-06-27-auv-core-spatial-result-consumption-pattern.md`](2026-06-30-query-readiness-closeout.md) | Stage definitions for MVP chain |
| [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-core-spatial-result-consumption-proof-matrix.md) | Rows 69/70 blockers; footnote⁶ |
| [`2026-06-28-auv-core-a2-full-chain-falsifier-review.md`](2026-06-30-query-readiness-closeout.md) | Coincidence risk until third donor |
| [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-30-query-readiness-closeout.md) | Row 69/70 defer; third vertical reopen trigger |
| [`2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md`](2026-06-29-core-third-vertical-admissibility-mvp.md) | Balatro MVP proof contract |

## Follow-up (not in this slice)

| If owner accepts… | Next slice | Trigger |
| --- | --- | --- |
| Balatro path | `Core-X2-balatro-consumption-probe` | Owner names candidate + MVP doc accepted |
| STS path | `Core-X2-sts-recognition-probe` | Same |
| Row 70 split | Separate graduation review | Owner narrows row to query-backend-only |

**Still defer:** Core-A5a/A5b until X1→X2 produces third-donor evidence.

## One-sentence summary

Core-X1 recommends **Balatro as the most donor-like scout candidate** (not an
existing donor) to pressure rows 69/70 and Core-C1 coincidence risk, with STS as
greenfield runner-up and Netease/MC-18/AX/OCR eliminated from the third-donor set
for honest lane reasons.
