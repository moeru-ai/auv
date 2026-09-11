# `auv-api-server` interface convergence plan

Date: 2026-08-04

Status: in-progress owner interview. Only confirmed decisions are normative;
open questions must not be treated as implementation approval.

## Scope classification

This is a proposed narrow refactor of the existing `auv-api-server` crate. It
does not approve behavior changes, a new crate, or forced module splitting or
merging.

The proposed `auv-daemon` extraction and the documents that describe it are
out of scope for this plan. This plan evaluates the responsibilities and
interfaces that exist in `auv-api-server` today without assuming a future
ownership migration.

Pairing persistence and administration are also excluded from the first
implementation slice. The slice will not change the pairing file, its schema,
the `PairingStore` behavior, or the CLI behavior of `auv devices enable`,
`disable`, or `unpair`. `PairingStore` remains a known temporary exception to
the stateless-interface rule until an owner separately decides whether local
pairing administration should use the file, an RPC, or another operation
interface.

## Confirmed interface rules

### Public-interface admission

A bare `pub` item must have either:

- a current, non-test consumer outside `auv-api-server`; or
- an owner-approved library contract that deliberately exists without a
  current production consumer.

Test access and speculative future reuse do not justify a public item. A test
should exercise a stable interface or remain within the visibility of the
implementation it covers.

### Stateless external interface

The external interface should be stateless in the following domain sense:

- callers outside the crate do not directly own or manipulate mutable daemon
  domain state, stores, registries, or supervisors;
- opaque resource-lifecycle handles are allowed when they encapsulate bound
  listeners, connections, inherited transports, shutdown, or equivalent OS
  resources;
- an opaque handle does not make domain state public merely because its
  implementation necessarily retains resource state.

Consequently, a bound server or inherited transport may be a valid external
interface, while direct external manipulation of a pairing store, runner
registry, or supervisor requires separate owner approval.

## Established inventory

The current source contains 86 explicitly visible functions or methods:

| Visibility | Source declarations |
| --- | ---: |
| `pub` | 37 |
| `pub(crate)` | 42 |
| `pub(super)` | 7 |

Of the 37 bare-`pub` declarations, 28 are reachable through the crate's public
module paths. On Unix, 27 are compiled because the Unix and non-Unix
`inherited_transport` declarations are mutually exclusive. Approximately 11
logical functions or methods have a current non-test production consumer
outside the crate; this count is evidence for the interview, not yet an
approved target count.

## Open questions

- Which external caller roles may configure and start protocol serving?
- Should internal control-service adapters consume one cohesive handler
  interface or several responsibility-specific interfaces?
- Which `pub(crate)` functions represent real cross-module seams, and which
  expose steps that should remain inside a deeper module?
- What regression evidence is required for a visibility-only refactor?
