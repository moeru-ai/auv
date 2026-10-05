# Listener Authentication, Owner Channel, and Registration

Date: 2026-10-06
Status: implemented on Linux and macOS; Windows owner-pipe changes deferred.

This note records the accepted split between three daemon concerns that were
previously coupled: where a listener binds, how it authenticates callers, and
whether the daemon registers as the caller's default local daemon. The shared
vocabulary lives in [`TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md)
under the daemon listener section.

## Problem

Before this change, `auv serve` passed `pairing_store.is_some()` into
`parse_listener`. Every `http://` listener, including `127.0.0.1`, became a
paired-bearer listener when a pairing store was configured, and an
unauthenticated owner listener otherwise. `CreatePairingToken` required owner
authority or an existing bearer, so an HTTP-only daemon had no documented way
to issue its first token.

On Linux and macOS, first pairing still worked by accident. When no Unix
listener was configured, `auv-api-server` bound a hidden
`auv-parent-<pid>-<hash>.sock` for executable Runner callbacks. That socket had
owner authority and pairing, and discovery published it. `--no-discovery`
therefore broke first pairing in this setup. Windows had no such fallback.

## Accepted contract

| Concern | Rule |
|---|---|
| Authentication | Follows the transport. Unix sockets and named pipes prove the owner. Every `http://` listener requires a paired Device bearer, loopback included. There is no unauthenticated TCP listener. |
| Owner channel | On Linux and macOS the daemon always binds an owner Unix socket if none was configured. It issues first tokens, answers discovery, and receives executable Runner callbacks. The hidden Runner parent socket is gone. |
| Pairing | Always available. `--pairing-store` only overrides `<store-root>/pairings.json`. |
| Token issuance | `CreatePairingToken` accepts only the local owner. A paired bearer receives `PERMISSION_DENIED`. |
| Registration | `auv serve` registers by default: the owner socket sits next to the discovery descriptor and the descriptor is published. `--no-register` (formerly `--no-discovery`) binds a private owner socket in the temp directory and publishes nothing. |

SSH access stays supported. A user who logs in to the daemon host is the owner
and uses the discovered Unix socket. A user who forwards the Unix socket with
`ssh -L` keeps owner authority, because the forwarding `sshd` process runs as
that user. A user who forwards the TCP port pairs once and uses a bearer.

`auv api-server serve` was removed. Its default mode depended on
unauthenticated loopback TCP, and `auv serve` covers its remaining topologies.

## Deferred decisions

- `TODO(windows-owner-listener)` in `auv-daemon`: Windows still adds an owner
  pipe only when no listener is configured. For the LocalSystem service, the
  pipe owner is SYSTEM, so ordinary users cannot reach it. Resolve this with
  the Windows service lifecycle in
  [`2026-10-05-windows-helper-and-daemon-service-architecture-research.md`](2026-10-05-windows-helper-and-daemon-service-architecture-research.md).
- `TODO(pairing-administration-authority)` in the pairing gRPC adapter:
  revoke, enable or disable, and unpair still accept any paired bearer. The
  owner must decide whether a Device may manage other Devices or only itself.

## Evidence

- `auv-daemon` server tests:
  `registered_daemon_with_only_a_paired_listener_publishes_its_owner_socket`,
  `unregistered_daemon_uses_a_private_owner_socket_and_publishes_nothing`, and
  `only_the_local_owner_issues_pairing_tokens_while_paired_devices_share_administration`.
- `auv-cli` end-to-end test
  `http_only_daemon_issues_the_first_token_through_its_owner_socket`: it starts
  `auv serve --listen http://127.0.0.1:PORT` with no pairing flags, rejects an
  anonymous HTTP token request, issues a token through discovery, pairs over
  HTTP, and checks that a `--no-register` daemon leaves the published descriptor
  unchanged.

Earlier design notes that mention `auv api-server serve` or `--no-discovery`,
for example
[`2026-07-31-device-run-runner-aggregated-api-design.md`](2026-07-31-device-run-runner-aggregated-api-design.md),
are historical for those surfaces.
