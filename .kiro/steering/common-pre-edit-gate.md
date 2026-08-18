---
inclusion: always
---

# Pre-Edit Hard Stop

Do not call Write/StrReplace until this block is in chat for the target file(s):

1. **Classification** — exactly one slice type (bug fix / test-only / docs-only / narrow refactor / owner-approved feature)
2. **Veto** — CONTRIBUTING.local.md implementation checklist; any yes → shrink slice first
3. **Non-goals** — explicit out-of-scope items
4. **Callers** — importers/users; confirm no duplicate (Grep/Glob)
5. **Regression** — which test catches behavior change, or n/a for docs-only
6. **Validation** — minimal command(s) for this diff only

Also: affected public API, data schemas if any, user instruction verbatim.

GateGuard denies the edit tool until facts are presented and you retry.
`ECC_GATEGUARD=off` is blocked — present facts instead of bypassing.
