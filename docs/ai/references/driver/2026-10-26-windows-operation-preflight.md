# Windows operation preflight and gate results

PR: [#423](https://github.com/moeru-ai/auv/pull/423).
Evidence level: deterministic executor tests and compilation.

The executor classifies every step before it acquires a Windows operation context.
Each step requires an explicit, recognized `action.type` and a compatible verification gate.
Step names and IDs cannot authorize media actions.
An invalid trailing step or gate prevents all earlier actions.

## Gate semantics

`WgcAliveGate.min_non_black_ratio` is a finite fraction in `0.0..=1.0`.
`WindowHealth.non_black_ratio` is always a percentage in `0.0..=100.0`.
A value of `0.75` in `WindowHealth` means 0.75%, including in test fixtures.
The executor converts only the gate threshold to a percentage.

Verified capture requires a fresh frame, positive dimensions, and a valid ratio that meets the declared threshold.
The fixed 50% `alive` policy does not override this threshold.
Invalid ratios, minimized targets, stale frames, and late results cannot produce confirmation.
Fast mode remains unconfirmed, and a skipped minimized capture does not count as an executed step.

`StatusAndVolumeGate` accepts only the supported `Playing` status, ignoring ASCII case.
Target volume and tolerance must be finite values in `0.0..=1.0`.
The gate timeout controls the playback poll.
A required title change needs different nonempty normalized titles.
An artist-only change cannot satisfy this gate.
The title poll uses the declared timeout.

The WGC timeout rejects late confirmation after the native capture returns.
It does not cancel an in-flight synchronous OS call.
The inline `NOTICE:` reserves cancellation for a separate driver lifetime slice.

## Caller boundary

`auv op run --operation NAME --mode verified --file operation.json --json` keeps the same syntax.
Custom operations must supply explicit action types and valid gate values.
Both built-in QQ Music definitions remain recognized.
Rust signatures and Protobuf messages do not change.
CLI and MCP map `GateFailed` to the existing failed result with `confirmed=false`.
No Protobuf or Runner API changes occur.

## Evidence

Four new gate regressions failed at head `38adf63c` before the repair.
A separate reproduction showed that an artist-only change produced a broader identity-change verdict.
The repaired tests also cover malformed volume values, an invalid trailing gate, and late capture results.

- `cargo test --locked -p auv-driver-windows -p auv-auto-loop`: passed, including 188 driver library tests and 34 auto-loop tests.
- `cargo check --locked`: passed on Windows.
- `cargo check --locked -p auv-driver-windows --lib --target x86_64-unknown-linux-gnu`: passed.
- `cargo test --locked`: passed, including 157 CLI tests. One CLI test remained ignored.
- `cargo fmt --check` and `git diff --check`: passed.

No live media action or isolated desktop transition formed part of the new regression tests.
These results do not establish end-to-end application success.
