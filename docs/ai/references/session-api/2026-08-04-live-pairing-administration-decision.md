# Live pairing administration uses shared authenticated authority

Date: 2026-08-04

Status: accepted

Pairing administration is performed against the live daemon through canonical
`auv` operations. `auv-daemon` is the sole owner of pairing persistence;
`auv-api-server` and `auv-api-client` provide the protocol adapters. CLI, MCP,
and other client interfaces never open or mutate the pairing store directly.

The local owner and every active paired Device bearer have equal authority to
create bootstrap tokens, enable or disable paired Devices, unpair Devices, and
revoke credentials. Pairing does not define a separate administrator role.
`PairDevice` remains the only unauthenticated operation and requires a valid
one-time bootstrap token.

This deliberately selects a shared-administrator trust realm over local-owner
administration or role-scoped credentials. Compromise of any active paired
credential therefore grants pairing administration until that credential or
Device is disabled, unpaired, or revoked. Mutations affect the next
authorization lookup.
