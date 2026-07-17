export const vacantControlBoundaryInstructions = `
You are reviewing one Rust source file.

Use the report_finding tool for each warning. Do not report anything when the file has no qualifying issue.
`.trim();

export const vacantControlBoundaryPrompt = `
Task:
Warn about vacant control boundaries.

A vacant control boundary is a function whose body performs only shallow local control flow, such as a single guard, mode check, or early return, and then delegates the real behavior to another same-file function. The outer function does not own meaningful policy, resource lifecycle, error semantics, validation, retries, observability, concurrency, dependency selection, or reusable API shape.

This is a warning-level design smell, not a correctness error.

Report the declaration line of the outer function whose boundary is not earning its existence. Do not report the delegated function unless it has the same smell independently.

Do not key on names, attributes, strings, comments, platform gates, or exact syntax. Infer the smell from control flow and responsibility:
- shallow local branch or early return
- direct delegation to another local function for the main behavior
- little or no transformation between input and delegated call
- no durable boundary that would help another caller, frontend, test, or runtime path

Report even when the shallow function has an attribute, registration marker, or generated-call surface, if the body itself merely performs a local guard and hands off to a private same-file function. A required signature can justify the entrypoint, but it does not justify splitting the real body into a second function unless that second boundary is independently useful.

Common qualifying shapes include:
- an entry function checks a flag or mode and then returns another local function call
- an entry function has one early error/success branch and otherwise delegates unchanged inputs
- an entry function exists mainly so the delegated function can have a similar name plus an implementation suffix
- separate conditional-compilation bodies exist only because the outer function delegated instead of owning the conditional body

Do not report functions that add a real boundary, including:
- framework, macro, ABI, trait, or callback adaptation where the signature itself is the boundary and the body owns the behavior directly
- stable public facade that intentionally hides volatile internals
- public ergonomic aliases, constructor synonyms, builder vocabulary, or DSL-style names that preserve a caller-facing API contract even when they delegate to a canonical constructor
- namespace accessors that create typed sub-API handles or capability views, such as session methods returning DisplayApi, WindowApi, InputApi, or similar objects
- binary, example, test, benchmark, or build-script entrypoints whose required role is to adapt the process/tooling entry signature to a library or platform-specific implementation
- cross-module, cross-crate, or platform facade functions that intentionally preserve a stable public seam over native, generated, or conditionally compiled implementation details
- meaningful validation or normalization that callers should not duplicate
- resource acquisition and cleanup scope
- error conversion that defines caller-visible semantics
- tracing, metrics, retry, cache, permission, transaction, or lifecycle ownership
- test helpers or fixture builders whose value is local readability

Treat the file path as context. If the path indicates an example, test, benchmark, build.rs, binary wrapper, platform adapter, or public API facade module, require stronger evidence before reporting. Do not report merely because such a boundary is thin.

When suggesting a fix, prefer either merging the delegated body into the caller-facing function or moving the delegated behavior behind a boundary that carries real policy. If uncertain, return no finding.
`.trim();
