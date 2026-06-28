# 2026-06-30 AUV Core-A5b-query D2 falsifier graduation review

Date: 2026-06-30

Status: **docs-only graduation review** for proof-matrix row **70a**. Runs the
70a falsifier set after [D1 query backend label contract](2026-06-30-auv-core-a5b-query-d1-query-backend-label-contract.md).
Records **docs-only closeout** — extraction candidacy **defer**, chain ends
without D3 code. **No code extraction approved.**

## Scope boundary

**In scope:**

- Row **70a** falsifiers and graduation questions
- D3 extraction candidacy adjudication (defer expected)
- Proof-matrix footnote **¹⁴** and blocker update

**Out of scope:**

- Rows 70b / 70c
- D3 trait crate implementation
- `inspect.rs`, donor enum, or manifest field changes

**Primary inputs:**

- [D1 query backend label contract](2026-06-30-auv-core-a5b-query-d1-query-backend-label-contract.md)
- [Core-A6 row 70 split](2026-06-30-auv-core-a6-row-70-split-owner-decision.md)
- [A5b-prep split review](2026-06-30-auv-core-a5b-prep-backend-label-discipline-split-review.md)
- [Core-A4 falsifier gate](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- [Core-A3 stage status helper design](2026-06-29-auv-core-a3-stage-status-triad-helper-design.md) (comparison baseline)
- `crates/auv-stage-status/src/lib.rs`

---

## Falsifier table (row 70a)

| ID | Claim | 70a handling | Verdict |
| --- | --- | --- | --- |
| **F2-full-row** | Half-row evidence cannot graduate monolithic row 70 | **Retired** — Core-A6 split row 70 into 70a/70b/70c; 70a judged independently | **Retired (not applicable)** |
| **F1-raw-command** | Stable labels insufficient; manifests need raw command text | MC-12/MC dual-backend tests + osu/Balatro v1 manifests persist enum labels only; no shell text in JSON | **Pass** |
| **F3-quality-strings** | osu witness strings substitute for backend enum | Scoped to **70c** quality surface; not a 70a query-backend falsifier | **Out of scope** |
| **F-naming** | `selected_backend` vs `query_backend` blocks graduation | [D1](2026-06-30-auv-core-a5b-query-d1-query-backend-label-contract.md) records **accepted divergence**; inspect mirrors donor fields | **Not blocker** (convergence backlog) |
| **F-cardinality** | MC four variants vs osu/Balatro one variant blocks helper | D1 policy: cardinality is contract note, not discipline failure | **Not blocker** |
| **F4-pressure** | Shared consumer forces `as_str()` trait extraction now | No donor-neutral shared consumer reads query-backend labels through a common trait; `inspect.rs` uses donor types and `.as_str()` directly per vertical | **Open → blocks D3 candidacy** |
| **F5-coincidence** | Two-donor recurrence might be accidental | Balatro X2/X5 third donor on query surface; discipline rule recurs on MC + osu + Balatro | **Mitigated** |
| **F-nominal-abstraction** | Trait with only `as_str()` removes insufficient repetition / is nominal | Three small `impl` blocks + local serde enums + distinct manifest fields remain entirely donor-local; shared surface is one method | **Triggered → blocks D3 candidacy** |

**D2 pass (review language):** falsifiers that apply to 70a **pass or are
retired/out-of-scope**, except **F4** and **F-nominal-abstraction**, which
correctly block code extraction candidacy.

---

## Required adjudications

### 1. Row 70a verdict language

**Maintain** `candidate, helper-only admissible` (review language only).

Evidence: three-donor probe recurrence on the discipline rule (A5b-prep, X5,
D1 inventory); Core-A6 owner split explicitly assigned this language to 70a.
No falsifier downgrades 70a to `not admissible yet`. F4 and F-nominal-abstraction
block **extraction**, not **admissible review language**.

### 2. Why not “insufficient repetition — should not extract”?

**Honest answer:** the objection is **partially true** for **code** extraction,
but it does **not** revoke helper-only admissible review language.

| Repeated today | Not repeated enough for A3-style extraction |
| --- | --- |
| Discipline rule (stable label, no raw command, inspect exposes label) | Identical enum variant set across donors |
| `#[serde(rename_all = "snake_case")]` + local `as_str()` per enum | Shared serde-owning enum (`StageStatus` pattern) |
| Three donors persist query-backend enums | Donor-neutral consumer calling a shared trait |

Compared to **Core-A3 / `auv-stage-status`**:

| Dimension | A3 `StageStatus` | 70a query backend |
| --- | --- | --- |
| Variant set | **Identical** `ready \| blocked \| failed` on every donor | **Different** per donor (MC 4, osu 1, Balatro 1) |
| Extraction shape | Shared **enum** + type aliases; crate owns serde | Cannot type-alias merge; trait-only leaves enums/serde/manifests donor-local |
| Consumer pressure | Multiple stage producers + inspect `.as_str()` on shared type | Inspect uses donor enums directly; no cross-donor trait consumer |
| Repetition removed | Duplicate enum definitions + serde tests | Would remove ~3×5-line `as_str` match blocks only |

So: **“insufficient repetition” is a valid reason to defer D3 code**, and that
is exactly what this review records. It is **not** a reason to deny that the
**discipline contract** recurs or to strip helper-only admissible language —
recurrence is at the **rule** level, not at the **shared-type** level.

### 3. Why not “wait for a shared consumer, then extract”?

**F4 is open.** No layer today needs a donor-neutral `QueryBackendLabel` trait:

- `src/inspect.rs` formats each donor's enum inline.
- `src/run_read.rs` parses MC wire strings into `TrainingResultSpatialQueryBackend`
  locally; osu/Balatro paths are similarly donor-scoped.
- No runtime, read-side, or CLI module iterates “any query backend” through a
  shared interface.

Default policy when F4 is open: **stop at docs-only closeout**. Extraction
candidacy requires **donor-neutral repetition** (duplicate glue worth removing)
**independent of** a hypothetical future consumer. That bar is **not met** —
see F-nominal-abstraction.

Waiting for a shared consumer would postpone a decision without adding evidence:
the trait shape does not unlock a consumer that does not exist yet. Owner could
reopen only if a **named** cross-donor read path appears **and** F-nominal-abstraction
is re-adjudicated.

### 4. D3 extraction candidacy

| Gate | Result |
| --- | --- |
| D2 falsifiers (70a) | Pass for review; **F4 + F-nominal-abstraction block extraction** |
| Donor-neutral repetition worth a crate | **Not met** |
| Core-A6 implementation gate | **Closed** — “No code extraction approved” |
| Owner explicit D3 approval | **Not received** |

**Verdict: `defer` — docs-only closeout.**

This is the **expected successful endpoint** for Core-A5b-query D1–D2. It is
**not** “D2 fail”; it is **not** automatic approval of D3.

---

## D3 candidacy vs docs-only closeout

```text
D1 contract ──► D2 falsifier review ──► docs-only closeout (this note)
                                              │
                                              ▼
                                    D3 blocked until:
                                    • F4 + F-nominal-abstraction re-open with new evidence
                                    • D2 records candidacy = yes
                                    • Owner explicitly approves D3 extraction
```

**Do not** read `helper-only admissible` as “implement `QueryBackendLabel` now.”
Core-A6 and this D2 review both say: review language **yes**, code extraction
**defer**.

---

## Comparison summary (70a vs A3)

| Question | A3 answer | 70a answer (D2) |
| --- | --- | --- |
| Shared enum possible? | Yes | **No** — variant sets differ |
| Trait-only sufficient? | N/A (enum extracted) | **No** — nominal abstraction |
| Serde in shared crate? | Yes (`auv-stage-status`) | **Must not** — D1 ownership boundary |
| Helper-only admissible? | Yes → extracted | Yes → **docs-only closeout** |
| Default next step | Code (owner-approved A3) | **Stop** — no D3 without new gates |

---

## Proof-matrix impact

| Column | Change |
| --- | --- |
| **Verdict** | **Unchanged** — `candidate, helper-only admissible`¹² ¹³ |
| **Blockers** | Updated — docs-only closeout; F4 + F-nominal-abstraction; extraction defer |
| **Footnotes** | **¹⁴** → this review |

See [proof matrix row 70a](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md).

---

## Chain status

| Phase | Status |
| --- | --- |
| D1 contract | **Complete** |
| D2 falsifier / graduation | **Complete — docs-only closeout** |
| D3 trait crate | **Skipped** — no owner approval; F4 open; F-nominal-abstraction triggered |

**Follow-up candidates (observations only, not started):**

- Second `render_backend` donor → revisit **70b** / Core-A5b-render
- osu persisted `quality_backend` enum → revisit **70c** / Core-A5b-quality
- Named cross-donor inspect consumer → reopen F4 for 70a only with owner slice

---

## Related references

- [D1 contract](2026-06-30-auv-core-a5b-query-d1-query-backend-label-contract.md)
- [Core-A6](2026-06-30-auv-core-a6-row-70-split-owner-decision.md)
- [A5b-prep](2026-06-30-auv-core-a5b-prep-backend-label-discipline-split-review.md)
- [Core-A4](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- [Core-A3 design](2026-06-29-auv-core-a3-stage-status-triad-helper-design.md) (extraction contrast)

## One-sentence summary

Core-A5b-query D2 **passes** 70a review-language graduation and **defers**
D3 extraction with an honest **docs-only closeout** because F4 shows no shared
consumer pressure and a trait-only `as_str` helper would be nominal abstraction
compared to the A3 shared-enum precedent.
