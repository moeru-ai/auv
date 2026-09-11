# `auv-daemon` document history

Date: 2026-08-03

Status: repository-history note. This records when the independent
`auv-daemon` crate became an accepted target; it does not approve implementing
the pending extraction.

## Conclusion

The independent `crates/auv-daemon` owner was introduced by the accepted
2026-08-03 architecture, not by the earlier Device/Run/Runner design. The
2026-07-31 design assigned responsibilities to the daemon process role without
naming a Rust crate. The 2026-08-02 package research instead showed
`auv-api-server` as the daemon composition root. The 2026-08-03 architecture
then changed that package boundary: `auv-daemon` became the long-lived state and
composition owner, while `auv-api-server` became a protocol-serving boundary.

The ownership text is committed. It first appears in commit
`308f0cd0a6dd6ee738f2fd51e5b8221b8d80c284` (`chore(docs): updated for new
remote auv`, authored and committed 2026-08-03 20:51:14 +08:00). The extraction
itself remains explicitly pending.

## Evidence timeline

### Before the 2026-08-03 documentation commit

The parent commit,
`d9da8d93f6a11937b0061afbdcdbd832dd5ac6f8`, contains no `auv-daemon` text.
It also does not contain the documents dated 2026-07-31, 2026-08-02, or
2026-08-03 examined below. All three entered committed history together in
`308f0cd0a6dd6ee738f2fd51e5b8221b8d80c284`. Their filename dates describe the
design timeline; they are not separate Git commit dates.

Repository-wide `git log --all -S'auv-daemon' -- .` reports
`308f0cd0a6dd6ee738f2fd51e5b8221b8d80c284` as the earliest commit containing
the exact term.

### 2026-07-31: daemon role, no `auv-daemon` crate requirement

The historical Device/Run/Runner design says that "the daemon" owns external
authentication, authorization, Runner creation, private IPC, routing, health,
draining, and discovery
([`2026-07-31-device-run-runner-aggregated-api-design.md`, lines 31-35](2026-07-31-device-run-runner-aggregated-api-design.md)).
Its serving section describes one foreground daemon using a shared control
plane and Runner supervisor (lines 600-638), and its schema section calls the
wire owner "Core AUV protobuf" under `auv/api/core/v1` (lines 771-789).

That document contains no `auv-daemon` occurrence in the committed snapshot.
It specifies a process role and behavior, but does not require a new Rust crate
or move ownership out of `auv-api-server`.

### 2026-08-02: `auv-api-server` as composition root

The package research states that the existing composition root appears to own
capability semantics (lines 51-62). Its proposed crate/dependency sketch then
names `auv-api-server` as the "daemon composition root" (lines 331-355), while
warning against creating crates without a real dependency, build, platform, or
ownership boundary (lines 357-361). Its recommended sequence preserves one
composition root and explicitly says not to infer that every package needs a
crate immediately (lines 405-417):

[`2026-08-02-api-client-server-package-architecture-research.md`](2026-08-02-api-client-server-package-architecture-research.md)

This note therefore did not require `auv-daemon`; its package sketch placed the
composition role in `auv-api-server`. It entered history already marked as
superseded for implementation direction by the 2026-08-03 architecture (lines
5-8).

### 2026-08-03: independent owner accepted, extraction pending

The accepted architecture assigns Device/Run state, Runner registration and
lifecycle, capability route resolution, first-party composition, and persistent
state to `crates/auv-daemon`; it limits `auv-api-server` to protocol serving
([`2026-08-03-auv-facade-daemon-runner-architecture.md`, lines 73-94](2026-08-03-auv-facade-daemon-runner-architecture.md)).

Its dependency diagram makes the intended compile-time direction explicit:
`auv-daemon -> auv-api-server`, and `auv-cli -> auv-daemon` (lines 243-269).
The migration list nevertheless labels introduction of `auv-daemon` and the
movement of stores, providers, supervision, and first-party composition as
**Pending** (lines 295-306). The document also says the sequence is a handoff,
not blanket approval to implement every step (lines 320-325).

The same ownership was added to
[`TERMS_AND_CONCEPTS.md`, lines 286-312](../../../TERMS_AND_CONCEPTS.md) in the
same commit. That durable vocabulary says `auv-daemon` owns state and control
semantics and starts listeners through `auv-api-server`.

## Committed versus working-tree state

- The `auv-daemon` ownership passages in the accepted architecture,
  `TERMS_AND_CONCEPTS.md`, and the session API index are present in `HEAD` and
  originate in `308f0cd0a6dd6ee738f2fd51e5b8221b8d80c284`.
- Current uncommitted edits to those documents adjust later terminology and
  implementation status; they did not introduce the `auv-daemon` ownership
  decision.
- No `crates/auv-daemon` directory exists in the current worktree. The document
  records an accepted target boundary whose extraction is still pending, not an
  already-landed crate.

## Reproduction commands

```text
git log --all --reverse --format='%H %aI %s' -S'auv-daemon' -- .
git grep -n 'auv-daemon' 308f0cd0 -- docs
git grep -n 'auv-daemon' 308f0cd0^ -- docs
git ls-tree -r --name-only 308f0cd0^ docs/ai/references/session-api
git show 308f0cd0:docs/ai/references/session-api/2026-08-02-api-client-server-package-architecture-research.md
git diff -- docs/TERMS_AND_CONCEPTS.md docs/ai/references/session-api
```
