# Daemon DeleteRunner Concurrency Race Fix

Date: 2026-10-26
Branch: `fix/daemon-delete-runner-race`
Baseline: `e72b3da2fe17c4b177c6cb90660b1b680c816ca6` (origin/main)
Scope: `crates/auv-daemon/src/daemon/runner.rs`, `runner_test.rs`, plus the minimal error-mapping additions required to surface the new conflict as gRPC `FailedPrecondition`.

## Problem

`RunnerSupervisor::delete()` on `e72b3da2` claimed deletion by setting
`phase = Draining` under the registry lock, waited for active operations to
drain, and then removed the entry with
`.expect("draining Runner remains registered")`. Every step after the first
lock release assumed the entry was still registered and still owned by this
call. That assumption fails under six real interleavings.

## Root causes (verified call chains)

1. **Concurrent delete panic + registry lock poisoning.** Two `delete()` calls
   on one runner both pass the drain loop (`notify_waiters` wakes both), the
   first `remove` wins, the second hits the `.expect` on `Option::None`. The
   panic fires while the `MutexGuard` temporary is alive, poisoning the
   registry lock; every later registry access panics for the remaining
   daemon lifetime.
2. **Ephemeral release steals a Draining entry.** `decrement_activity_locked`
   removed the entry when the last operation ended for an `Ephemeral` runner
   without checking phase. While a delete was draining, the last
   `OperationPermit::drop` removed the Draining entry itself and stopped it:
   the delete owner then panicked on `remove` (or hung in the drain loop when
   no grace deadline existed, because `drained` is computed via
   `get(...).is_some_and(...)`).
3. **Lost wakeup in the drain loop.** The loop created
   `activity_changed.notified()` before checking the predicate, but `Notified`
   registers with the `Notify` only on its first poll, and `notify_waiters`
   stores no permits. A notification between the predicate check and the
   first poll was lost; with no grace deadline the delete hung forever.
4. **Idle stop races the delete owner.** `decrement_activity_locked` re-set
   `idle_deadline` and scheduled `schedule_idle_stop` even on a Draining
   entry, so an `UnlessIdle` runner could be removed and stopped behind the
   delete owner's back.
5. **Shutdown races the delete owner.** `shutdown()` `mem::take`s the whole
   registry; a draining delete then panicked on `remove`, or hung because a
   permit released against a vanished entry returns early without notifying.
6. **Ownership loss on stop failure or cancellation.** The entry was removed
   from the registry *before* stopping. If `stop_managed_in_place` failed or
   the delete future was cancelled mid-stop, the `tokio::process::Child` was
   dropped (`kill_on_drop` is false): a live child process with no registry
   trace and no retry path.

## Fix

Ownership invariant: **a Draining-phase entry in the registry ⇔ exactly one
delete owns that runner's deletion.** Only `delete()` sets `Draining` inside
the registry; every other path (permit drop, idle stop, ephemeral release)
leaves Draining entries alone, and `shutdown()` is the single legal taker.

- `delete()` claims synchronously under one lock hold (absent → `Unknown`;
  already Draining → new `RunnerError::Deleting`; otherwise claim), then
  spawns a detached owner task and awaits it. Claim and spawn have no await
  between them, so a cancelled caller can never strand a claimed but unowned
  entry.
- The owner task performs the drain wait, removes the entry, and stops it.
  Cancellation of the caller no longer cancels the deletion.
- The drain wait registers its `Notified` waiter (single manual poll) *before*
  re-checking the predicate, closing the lost-wakeup window; an entry that
  vanished (shutdown took it) returns `Unknown`.
- On stop failure the owner task re-inserts the entry under the `Failed`
  phase: the daemon keeps tracking the child and a later `DeleteRunner` can
  retry, instead of leaking an unmanaged process. `kill_on_drop` stays off.
- `decrement_activity_locked` gates lifecycle-driven automatic stops on
  `phase == Ready`, and `schedule_idle_stop`'s predicate requires `Ready`.
- `shutdown()` notifies `activity_changed` after taking the registry so
  draining deletes observe the takeover instead of waiting forever.
- Error mapping: `RunnerError::Deleting` → `ControlError::RunnerDeleting` →
  gRPC `failed_precondition`. No protobuf schema change.

State transitions before/after (registry-observable):

| From | Trigger | Before | After |
| --- | --- | --- | --- |
| Ready | delete claims | Draining, owner unknown to others | Draining = delete owner; concurrent delete → `Deleting` |
| Draining + Ephemeral | last permit drop | entry removed + stopped by permit path | counts balanced; owner removes + stops |
| Draining + UnlessIdle | last permit drop | idle deadline reset, idle stop scheduled | no schedule; owner stops |
| Draining | shutdown | panic or hang | owner gets `Unknown`; shutdown stops |
| Stopping (removed) | stop error / cancel | child untracked, entry gone | entry re-inserted as Failed, retryable |
| Failed | delete retry | n/a (entry was gone) | claim → stop → Stopped, or Failed again |

## Concurrency test matrix

All tests are deterministic: interleavings are controlled by await points,
registry-state polling (the claim is visible only after it happened), and
explicit permit drops; every wait is bounded by `tokio::time::timeout` so a
regression fails instead of hanging. No test starts the user's daemon or
runners; RemoteGrpc fixtures use lazy channels (no endpoint), the live-fixture
test spawns a local gRPC server inside the test, and the executable tests use
inert test-owned sleeper children that are always terminated.

| Test | Interleaving controlled | Old behavior | New behavior |
| --- | --- | --- | --- |
| concurrent delete, single owner | poll until Draining, then second delete | panic (lock poisoned) | second gets `Deleting`; first completes |
| delete waits for active operations | permit held until asserted | n/a (regression guard) | completes only after release |
| Ephemeral release during drain | permit dropped after Draining observed | hang / panic | owner completes, entry removed |
| shutdown during drain | shutdown called after Draining observed | hang | `Unknown`; shutdown stops |
| idle/ephemeral vs Draining (unit) | direct `decrement_activity_locked` / `schedule_idle_stop` calls | entry stolen | entry untouched |
| stop failure retention | policy function with synthetic stop error | child untracked | Failed entry, retry succeeds |
| cancelled delete future | caller `JoinHandle` dropped during drain | entry stuck Draining | deletion completes |
| RemoteGrpc detach | live local gRPC fixture | detach (unchanged) | endpoint still serving after delete |
| Draining rejects new operations | begin after claim | rejected (regression guard) | rejected |
| force / grace | real sleeper child | unchanged semantics | unchanged semantics |

## Evidence

Commits (branch `fix/daemon-delete-runner-race`):

- `3f5503b1` `test(auv-daemon): cover concurrent Runner deletion races`
- `d78c46be` `fix(auv-daemon): prevent concurrent Runner deletion races`

Pre-fix failure evidence: running the new suite against the old
(`e72b3da2`) `runner.rs` produced 54 passed / 5 failed, matching the
failure modes predicted in the root-cause section:

- `concurrent_delete_single_owner_second_delete_conflicts` — the second
  delete hung in the drain loop until the 500 ms timeout fired instead of
  returning the conflict error.
- `shutdown_takes_over_from_draining_delete` — the draining delete hung
  until its timeout after shutdown took the registry.
- `ephemeral_release_during_drain_completes_delete` — the Ephemeral permit
  release removed the Draining entry and the delete hung to its timeout.
- `idle_stop_and_ephemeral_leave_draining_entries_to_delete_owner` —
  `decrement_activity_locked` removed the Draining entry.
- `cancelled_delete_future_still_completes_deletion` — the claimed entry
  was stranded as Draining with no owner.

The behavior-preservation tests (`delete_waits_for_active_operations`,
`remote_grpc_delete_detaches_without_stopping_endpoint`,
`draining_runner_rejects_new_operations`,
`force_and_grace_semantics_preserved`) passed on the old code as
regression guards.

Post-fix validation on the fix branch (Windows host):

- `cargo fmt --check` — clean.
- `git diff --check` — clean.
- `cargo check --locked -p auv-daemon` — exit 0.
- `cargo test --locked -p auv-daemon --lib` — 60 passed, 0 failed.
- `cargo clippy --locked -p auv-daemon --all-targets` — exit 0; the only
  warnings are pre-existing in untouched crates (`auv-api-client`,
  `auv-api-server`, `auv-media-macos`, `auv-tracing`).
- `cargo check --locked -p auv-api-server` — compiles (error-mapping
  consumer); `git diff e72b3da2..HEAD -- proto/` is empty.

Cross-platform behavior is validated by the repository's GitHub Actions
Rust checks on Windows, Ubuntu, and macOS (see PR).

## Independent adversarial review

Subagent B (independent, read-only, no shared authorship) reviewed both
commits against an 11-item adversarial checklist. Verdict: **PASS**; no
P1 or P2 findings. Per-item results:

1. Double-stop: safe — registry removal is exclusive under the mutex and
   shutdown takes the map atomically; every lifecycle auto-stop path is
   phase-gated to Ready.
2. Wait-after-removal deadlock: safe — the conflict check is synchronous;
   no delete ever waits on an entry that can vanish.
3. Lost wakeup: safe — the waiter is registered before the predicate
   re-check and every predicate-enabling transition calls
   `notify_waiters`; `active_operations` cannot increase while Draining.
4. Orphaned child: safe — stop failure re-inserts as Failed; caller
   cancellation cannot strand a claimed entry (claim and spawn share one
   synchronous section).
5. Fake success: none — the response is built only after
   `stop_managed_in_place` returns `Ok`, which sets `Stopped` only after
   the child is reaped.
6. Draining interactions: safe — admission, activity decrement, idle
   scheduling, and routing all gate on Ready without panic or underflow.
7. Lock across `.await`: none — every guard is confined to a statement or
   block that ends before any await.
8. New lock-order deadlock: none — one registry mutex, `Notify` is never
   signaled while holding it.
9. Error mapping: `Deleting` → `RunnerDeleting` → `failed_precondition`;
   protobuf untouched.
10. Test honesty: verified empirically (reviewer rebuilt the test commit
    on the old implementation and reproduced the exact 5 failures).
11. Behavior preservation: force, grace, RemoteGrpc detach, affinity
    zeroing, and Ready-lifecycle semantics unchanged.

Findings (all P3, none blocking):

- Owner-task panic would strand the entry as Draining — accepted as a
  documented deferral; a NOTICE marker at `delete_runner_task` records the
  gap, reason, and re-open trigger.
- `attach_run` lacks a phase check (pre-existing; follow-up candidate).
- Concurrent-cleanup force-delete of a just-created runner can surface
  `RunnerDeleting` instead of the original admission error — truthful and
  strictly better than the old double-stop race.
- `StopRun` → `release_run_affinity` after delete zeroed affinities can
  report an accounting-underflow error — identical on the base commit,
  out of scope.

## Remaining risks

- `attach_run` does not check phase and can add a run affinity to a Draining
  runner; accounting stays balanced (the later release decrements it) and the
  delete owner is unaffected, but new-affinity-during-delete admission is
  pre-existing behavior left unchanged for this slice.
- `delete()` zeroes `run_affinities` at claim time (existing choice); a
  concurrent `release_run_affinity` for a cancelled run can report an
  accounting-underflow error. Pre-existing, unchanged.
- A daemon process exit (runtime drop) while a delete owner task is between
  claim and child termination can still drop an un-waited child; that is the
  pre-existing process-exit boundary (`kill_on_drop` remains off by design)
  and needs an owner-approved lifecycle decision, not an in-delete fix.
- A real OS-level kill failure cannot be injected portably, so the
  stop-failure path is covered at the policy-function level plus by review;
  the end-to-end failing-stop scenario relies on that coverage.
