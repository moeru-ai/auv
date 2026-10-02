# macOS Helper Protocol Compatibility and Revocation

Status: **accepted and implemented on 2026-10-02** (PR #217). The owner
accepted the recommended option for each of D1–D4.

Related: [macOS helper setup](2026-10-01-macos-helper-setup.md),
[AIRI bundled daemon research](2026-08-14-airi-bundled-auv-daemon-research.md).

## Problem

Before this change, setup treated the installed helper as usable only when its
`CFBundleShortVersionString` equals the helper crate version compiled into the
calling frontend (`setup.rs` `status()`). Every frontend that embeds a helper
— the `auv` CLI, the `@auv-js/cli` N-API binding, and any Electron app that
bundles either — therefore demanded its own exact version.

Two frontends at different versions on one user account replaced each other's
helper on every `install`, including downgrading a newer helper. That made AUV
impossible to distribute through several independent frontends.

## Owner decision (2026-10-02)

CLI and SDK packages are only frontends. A helper is usable whenever its
protocol is compatible, regardless of how many frontends or daemons exist or
which versions they are. Setup must not require version equality.

## Who actually depends on the helper

Only the root daemon calls the helper: `host::read_request` rejects every
socket peer whose UID is not 0. Setup frontends install, register, and inspect
the helper but never call its operations. The compatibility relation is
therefore between a daemon's wire protocol and the installed helper, not
between a setup frontend's crate version and the helper.

The wire header already carried a protocol version (then `lib.rs`
`VERSION = 1`). Two gaps made it insufficient as the contract:

- `host::decode_header` mapped a version mismatch to `Unauthorized`, so a daemon
  cannot tell an incompatible helper from an identity failure.
- `Operation::Lock` (5) was added inside version 1
  (`NOTICE(device-entry-macos-lock-ipc)`). Every `ai.moeru.auv.helper` build
  already supports it because that identity is new in PR #217, so version 1 can
  be defined as operations 1–5 without breaking a released helper.

## Contract

A helper supports a closed protocol range `[min, max]`. A frontend speaks one
protocol `P`: the version its own daemon build sends (`PROTOCOL_VERSION`). The
helper host accepts `SUPPORTED_PROTOCOLS`.

| Installed helper | Meaning | `status` | `install` |
|---|---|---|---|
| `min ≤ P ≤ max` | compatible | current runtime state (`running`, …) | keep it; upgrade only if the embedded helper is strictly newer and the helper is not `busy` |
| `max < P` | helper too old | `update-required` | replace with the embedded helper |
| `P < min` | this frontend too old | `frontend-outdated` | refuse with `FrontendOutdated`; never downgrade |

Crate versions are used only to decide whether a compatible upgrade is
available, never to decide usability. Because replacement only moves forward,
two frontends can no longer replace each other's helper back and forth.

Future protocol changes must add operations by raising `max`, and drop support
only by raising `min`.

## Accepted decisions

- **D1 — declaration.** Signed Info.plist keys `AUVHelperProtocolMin` and
  `AUVHelperProtocolMax`, read statically like the bundle version. The setup
  test `packaged_info_plist_declares_the_supported_protocol_range` keeps
  `package/Info.plist` equal to `SUPPORTED_PROTOCOLS`. A helper without the
  keys is `invalid`.
- **D2 — state.** `frontend-outdated` (Rust `State::FrontendOutdated`, error
  `Error::FrontendOutdated`, and the `MacosHelperState` union in
  `@auv-js/cli`).
- **D3 — upgrades.** `install` replaces a compatible helper only when the
  embedded crate version is strictly newer (semver) and the helper is not
  `busy`. An unreadable installed version keeps the compatible helper.
- **D4 — wire diagnostics.** The host accepts any header version in
  `SUPPORTED_PROTOCOLS` and answers others with `HostError::ProtocolUnsupported`
  (status byte 19). Magic and UID checks still run first and stay
  `Unauthorized`. The daemon maps it to the public reason described in
  [Public reason](#public-reason).

## Evidence

Unit tests in `auv-device-helper-macos`: `compatibility_depends_only_on_the_declared_protocol_range`,
`frontends_at_different_versions_do_not_replace_each_others_helper`,
`an_unreadable_installed_version_keeps_the_compatible_helper`,
`packaged_info_plist_declares_the_supported_protocol_range`,
`unsupported_protocol_version_is_reported_distinctly`, and
`unsupported_protocol_from_another_uid_is_still_unauthorized`. No live
two-frontend installation has been exercised yet.

## Security epoch revocation

Accepted on 2026-10-02 as the owner-delegated follow-up to
`REVIEW(helper-downgrade-policy)`. Protocol compatibility answers whether a
helper *can* serve a daemon; the security epoch answers whether the daemon
*trusts* that signed build. They change independently.

- Each helper declares `AUVHelperSecurityEpoch` as an Info.plist **string**.
  The daemon trusts a helper only when it satisfies the identity requirement
  and `info[AUVHelperSecurityEpoch] >= "MIN_SECURITY_EPOCH"`.
- Revocation is one release step: raise the epoch in `package/Info.plist` and
  `MIN_SECURITY_EPOCH` together. New daemons reject every older signed helper.
  Older daemons keep trusting the newer helper, so revocation never blocks a
  rolling update. The epoch starts at 1.
- The rule lives in the code requirement, which covers the signed Info.plist,
  so a same-user attacker cannot reinstall a revoked helper to satisfy it
  without re-signing with the pinned Team ID.
- Setup validates identity statically, then reads the epoch. A revoked but
  genuine helper is `update-required` and `install` replaces it; it is not
  `invalid`. A socket peer that is a revoked build reports the same state.
- The daemon client checks identity and epoch separately, so a revoked helper
  surfaces as `HostError::Revoked` instead of an identity failure.

Evidence: `security_epoch_clause_compares_string_epochs_numerically` signs
throwaway bundles ad hoc and confirms that `"10" >= "2"` holds numerically, that
`"1"` is rejected, and that an integer-typed value never matches. This is the
basis for `NOTICE(helper-security-epoch-requirement)`. Measured on macOS 26.3.
`packaged_info_plist_declares_a_trusted_security_epoch_as_a_string` keeps the
packaged helper trusted by the daemon built with it.
`helpers_below_the_minimum_security_epoch_are_not_trusted` covers setup's
trust decision, including a missing epoch.

## Public reason

`DEVICE_ENTRY_ERROR_REASON_HOST_INCOMPATIBLE = 13` (Rust
`DeviceEntryErrorReason::HostIncompatible`, SDK `'hostIncompatible'`) reports
both a protocol mismatch and a revoked helper over the paired Device API. Local
enrollment reports `LocalControlError::HostIncompatible` as gRPC
`FAILED_PRECONDITION`. Both mean that updating AUV on the target resolves the
failure and retrying does not. The reason is platform-neutral; only macOS
produces it today. Older clients that predate the value treat it as an unknown
enum value, which is acceptable because no released daemon emits it.

## Out of scope

- Locking against concurrent installs (`TODO(helper-setup-install-lock)`).
