# macOS Helper Protocol Compatibility

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
  `Unauthorized`. The daemon logs a fixed local diagnostic and maps the error
  to its existing unavailable reasons:
  `TODO(macos-helper-protocol-reason)` records that adding a public reason
  changes the Device and local-control protobufs.

## Evidence

Unit tests in `auv-device-helper-macos`: `compatibility_depends_only_on_the_declared_protocol_range`,
`frontends_at_different_versions_do_not_replace_each_others_helper`,
`an_unreadable_installed_version_keeps_the_compatible_helper`,
`packaged_info_plist_declares_the_supported_protocol_range`,
`unsupported_protocol_version_is_reported_distinctly`, and
`unsupported_protocol_from_another_uid_is_still_unauthorized`. No live
two-frontend installation has been exercised yet.

## Out of scope

- Revoking a signed but vulnerable helper. That remains
  `REVIEW(helper-downgrade-policy)` and needs a signed minimum-security
  version, separate from protocol compatibility.
- Locking against concurrent installs (`TODO(helper-setup-install-lock)`).
