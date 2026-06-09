# AUV macOS AX Copilot - MVP Evidence Pack / Handoff Reference

Status: narrow copilot MVP has run end to end on real hardware and been verified across two real action classes. This is a docs-only checkpoint record.

Date: 2026-06-09

Related main commit: `6fad52b feat(candidate-action): add human gesture consent path`

## 1. One Sentence

AUV has run a real macOS GUI automation copilot vertical with a human in the loop, full auditability, and semantic verification, and the same spine now holds across both click and keyboard-delivered TypeText:

```text
observe -> AX recognition -> stability -> refusal-gated promotion -> human consent
  -> readiness -> decide -> execute(single action) -> deliver -> semantic verify -> trace
```

Both the allow path and the refusal path have live evidence, and the action seam is no longer proven only for click.

## 2. Canonical Closed-Loop Proof

Canonical run: `run_1780938107439_13406_0`

Command shape: `candidate-action run --human-gesture-consent`

Target:

- app: `com.apple.TextEdit`
- query: `First Text View`
- role: `AXTextArea`

Observed facts:

- promotion consent: `consent_provenance=human_gesture`
- promotion consent grade: `consent_grade=human_approved`
- execution consent: `consent_provenance=human_gesture`
- execution consent grade: `consent_grade=human_approved`
- readiness: `ready`
- input delivery: `attempted`
- operation status: `completed`
- semantic verification: `semantic_matched=true`

Artifact lineage:

```text
candidate.action.command.consent.approved
  -> candidate-promotion
  -> candidate-action-decision
  -> candidate-action-execution
  -> VerificationResult(method=semantic_match, semantic_matched=true)
```

Frontmost preflight confirmation:

- `run_1780938092925_12548_0` showed frontmost `TextEdit (Untitled 6)` before the canonical run.

## 3. Refusal-Side Proof

The gate does not only allow; it also refuses honestly.

### No consent

Run: `run_1780936502809_50041_0`

- result: `permission_missing`
- meaning: recognition, projection, freshness, and stability all passed; only authorization was missing

### Human-gesture timeout

Run: `run_1780936878562_78284_0`

- events: `consent.requested` -> `consent.not_approved status=timed_out`
- result: `permission_missing`
- meaning: the command did not self-issue approval when human approval was absent

### Readiness block after approval

Run: `run_1780936530806_50887_0`

- approval path completed through `local_auth_device_owner_authentication`
- frontmost app was stolen by `net.java.openjdk.cmd`
- result: `blocked_not_ready`
- input delivery: `not_attempted`

This was later disproven as an environment-side disturbance, not a structural contract gap, by the clean rerun that produced the canonical success path.

## 4. Honesty Invariants Held

- One recognition contract: `RecognitionResult`
- One action-result seam: `ActionResolverDecision` plus `InputActionResult`
- No third parallel action-result schema
- Refusal-first promotion gate with typed refusals recorded into trace
- Readiness is a pre-execution hard gate; not-ready blocks before executor delivery and records `input_delivery=not_attempted`
- `human_approved` consent cannot be self-issued by the program; it is minted only by local `LocalAuthentication` device-owner approval
- Execution consent is bound to `execution_id` and checked against run, promotion, decision, candidate, action, provenance, and grade
- Cross-execution reuse is rejected as `ExecutionConsentMismatch`
- Semantic verification is post-action re-observation, not activation theater
- Promotion, decision, execution, and verification all record artifacts and read-side lineage

In practical terms: "copilot, not bot" is enforced by the system, not asserted by convention.

## 5. Cross-Action Generalization Proof

The same audited spine now holds for a second typed action, not just the original click path.

Generalization run: `run_1781020033303_42222_0`

Command shape:

```text
candidate-action run --target-app com.apple.TextEdit --query 'First Text View' --role AXTextArea --action type-text --text 'AUV_TYPE_TEXT_MARKER_2026_06_09_V6' --human-gesture-consent
```

Observed facts:

- promotion consent: `consent_provenance=human_gesture`
- promotion consent grade: `consent_grade=human_approved`
- execution consent: `consent_provenance=human_gesture`
- execution consent grade: `consent_grade=human_approved`
- action method: `window-targeted-type-text`
- selected delivery path: `window_targeted_keyboard`
- readiness: `ready`
- input delivery: `attempted`
- operation status: `completed`
- semantic verification: `semantic_matched=true`

What this proves:

- the contract seam does not collapse outside the original click path
- the same promotion -> consent -> readiness -> decide -> execute -> verify chain holds for a keyboard-delivered typed action
- `approved_action` binding is real across action classes, not a click-only special case

This is the first live proof that the AUV copilot spine generalizes beyond "point at a thing and click it once".

## 6. Honest MVP Boundary

This is an MVP checkpoint, not a generalized product surface.

Current scope:

- two action classes proven live:
  - window-targeted mouse / single click path
  - window-targeted keyboard / `TypeText`
- one app family proven live: TextEdit / AX-addressable targets
- entrypoint is a dev-grade CLI path: `candidate-action run --human-gesture-consent`

Deferred / not part of this checkpoint:

- broader product UX around approval
- wider typed action surface
- detector / YOLO consumption as the primary path
- game-state interpretation work

## 7. Known Non-Blocking Items

### Inspect server not running

- local writes to `127.0.0.1:8765` failed during runs
- source of truth remains local `.auv/runs/<run_id>/` artifacts
- inspect server is only a read convenience layer here, not the authoritative record

### Transient `186x155` frame anomaly

- readiness blocked safely before delivery
- root cause remains unresolved
- `readiness_debug` now preserves window snapshots so the next recurrence is self-explaining

### Smoke env backdoors still exist

- `AUV_L8B_WINDOW_NUMBER`
- `AUV_L8B_WINDOW_FRAME_*`

The command path is now the truthful integrated path; these env backdoors are cleanup candidates.

### `src/app/mod.rs` remains a structure hotspot

Collabi overview currently shows many path-overlap warnings centered on `src/app/mod.rs`. That is structural noise, not an active claim conflict for this MVP slice.

### Collabi board noise

At the time of capture:

- `claims = 0`
- many `sync_required` lanes are old intent-overlap noise rather than active conflicts

## 8. Key Seam Locations

- contract seam: `src/contract.rs`
- AX recognition: `src/ax_recognition.rs`
- stability: `src/stability.rs`
- promotion gate: `src/candidate_promotion.rs`
- promotion recording: `src/candidate_promotion_recording.rs`
- decide / execute / readiness / consent consistency: `src/candidate_action_decision.rs`
- command entry: `src/candidate_action_command.rs`, `src/cli.rs`, `src/main.rs`
- human approval native bridge:
  - `crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Auth.swift`
  - `crates/auv-driver-macos/src/native/auth.rs`
  - `crates/auv-driver-macos/src/native/binding.rs`
- read side:
  - `src/run_read.rs`
  - `src/inspect.rs`
  - `src/inspect_server/mod.rs`

## 9. Validation

The implementation and read-side fallout were validated with:

```text
cargo test -q candidate_action_command --lib
cargo test -q candidate_action_decision --lib
cargo test -q run_read --lib
cargo test -q inspect_server --lib
cargo test -q parse_candidate_action_run_command --bin auv-cli
cargo test -q native_human_approval_status_labels_are_stable -p auv-driver-macos
cargo test -q execution_consent_cannot_be_reused_for_another_execution_id --lib
hack/generate-swift-bridge
cargo check -q
swift build    # crates/auv-driver-macos/native/swift
cargo fmt --check
git diff --check
```

Live evidence additionally includes:

- frontmost clean preflight: `run_1780938092925_12548_0`
- canonical success path: `run_1780938107439_13406_0`
- cross-action TypeText success path: `run_1781020033303_42222_0`
- no-consent refusal: `run_1780936502809_50041_0`
- timeout refusal: `run_1780936878562_78284_0`
- readiness refusal after approval: `run_1780936530806_50887_0`

## 10. Decision, Not Slice

This checkpoint closes a narrow but real MVP vertical. It is still narrow, but it is no longer a one-action proof. The next step is a product decision, not an automatic engineering slice.

Possible directions:

- turn the path into a more formal product entrypoint
- expand the typed action surface behind the same gates
- package it for demo / open-source / external presentation
- pause here and keep the checkpoint as a stable, documented milestone
- hand the line back to another owner or team member

Deferred slices intentionally not started here:

- re-activate-after-consent seam
  - only justified if frontmost loss is shown to be structural rather than environmental
  - any activation must be recorded, consented, and followed by a full readiness re-check
- typed action expansion
- retention / redaction
- replay / golden-artifact regression
- inspect server repair
