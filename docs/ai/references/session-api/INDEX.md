# Device / Run / Runner API

Device/Run/Runner control API, protobuf, Runner aggregation, MCP frontend, and
the tombstone for the retired SessionService prototype. The folder name is
retained as a stable responsibility path; Session is not a public resource.

Accepted target architecture:
[`2026-08-03-auv-facade-daemon-runner-architecture.md`](2026-08-03-auv-facade-daemon-runner-architecture.md).
The 2026-07-31 aggregated API is the current implementation baseline, not the
accepted package and routing target. Public SessionService, session-scoped
Connection, legacy VisionService, and `/v1/*:verb` routes were removed on
2026-07-31.

Current implementation ownership still places daemon state and typed capability
forwarding in `auv-api-server`, context/placement policy in `auv-api-client`,
and serving composition in `auv-cli`. The accepted target introduces `auv` as
the canonical local/remote operation interface and `auv-daemon` as the long-lived control owner;
`auv-api-client` and `auv-api-server` become protocol boundaries. MCP remains in
`auv-cli`.

Count: **39**

- [`2026-10-04-windows-helper-release.md`](2026-10-04-windows-helper-release.md) — `auv-helper.exe` naming, explicit Cargo target, Windows CI coverage, two-binary release archive, and the still-separate privileged installation gate.
- [`2026-10-04-windows-helper-install.md`](2026-10-04-windows-helper-install.md) — protected two-binary install, SCM lifecycle, offline first-pairing bootstrap, status, and non-destructive uninstall contract.
- [`2026-10-02-macos-helper-protocol-compatibility.md`](2026-10-02-macos-helper-protocol-compatibility.md) — helper usability decided by daemon wire-protocol range, not frontend version; forward-only upgrades, `frontend-outdated`, signed security-epoch revocation, and the `HOST_INCOMPATIBLE` reason.
- [`2026-10-01-macos-helper-setup.md`](2026-10-01-macos-helper-setup.md) — shared Rust/CLI/N-API helper setup, embedded signed app release path, shipped helper identities (`helperApp` / `--helper-app`), per-user `SMAppService` registration, and remaining clean-host gate.
- [`2026-09-28-remote-device-entry-implementation-design.md`](2026-09-28-remote-device-entry-implementation-design.md) — current locked-existing-session-first Device unlock contract and three-platform implementation plan; signed-out entry deferred.
- [`2026-09-30-device-lock-contract-and-review.md`](2026-09-30-device-lock-contract-and-review.md) — experimental lock API/CLI/MCP contract, configuration-specific gate evidence, pairing experience, and prioritized architecture review.
- [`2026-09-27-device-entry-credential-decision.md`](2026-09-27-device-entry-credential-decision.md) — accepted target-local persistent credential enrollment and deletion; pre-login storage requirements now deferred.
- [`2026-09-27-device-login-host-lifecycle-decision.md`](2026-09-27-device-login-host-lifecycle-decision.md) — historical machine-level daemon lifecycle decision for the deferred signed-out phase.
- [`2026-09-27-device-unlock-authority-decision.md`](2026-09-27-device-unlock-authority-decision.md) — accepted paired-bearer authority for existing-session unlock; earlier sign-in scope deferred.
- [`2026-09-29-windows-scm-host.md`](2026-09-29-windows-scm-host.md) — candidate Windows service lifecycle, restricted storage, and temporary installation procedure; final locked-console evidence is recorded in the Windows host handoff.
- [`2026-09-27-remote-device-unlock-research.md`](2026-09-27-remote-device-unlock-research.md) — historical policy interview, three existing-session unlock proofs, and failed macOS signed-out delivery gates.
- [`2026-09-28-macos-remote-desktop-loginwindow-research.md`](2026-09-28-macos-remote-desktop-loginwindow-research.md) — primary-source comparison of Apple remote desktop services and third-party login-window mechanisms; no AUV support claim.
- [`2026-09-28-macos-locked-session-host-gate.md`](2026-09-28-macos-locked-session-host-gate.md) — historical signed Aqua helper, private Keychain and IPC, and supervised same-session unlock evidence under the former helper identity; current-identity evidence is recorded in the helper setup note, and neither gate is a production support claim.
- [`2026-09-28-linux-gnome-locked-session-host-handoff.md`](2026-09-28-linux-gnome-locked-session-host-handoff.md) — paired GNOME existing-session unlocks with target-local PAM revalidation; display wake, rotated-password rejection, and release installation remain open gates.
- [`2026-09-28-windows-locked-session-host-handoff.md`](2026-09-28-windows-locked-session-host-handoff.md) — paired locked-console unlock with same-session readback and owner confirmation; release installation and broader configurations remain open.
- [`2026-09-08-protobuf-source-distribution-reference.md`](2026-09-08-protobuf-source-distribution-reference.md) — checked-in BSR exports, dependency updates, and clean-checkout Cargo installation checks.
- [`2026-08-17-protobuf-json-schema-library-research.md`](2026-08-17-protobuf-json-schema-library-research.md) — primary-source evaluation of runtime and build-time Protobuf-to-JSON-Schema libraries; no browser-compatible `DescMessage` drop-in exists, with two viable future paths documented.
- [`2026-08-16-windows-local-runner-ipc-handoff.md`](2026-08-16-windows-local-runner-ipc-handoff.md) - Windows local API and daemon-to-Runner named-pipe transports, paired use, tests, and current limits.
- [`2026-08-14-airi-bundled-auv-daemon-research.md`](2026-08-14-airi-bundled-auv-daemon-research.md) — AIRI Electron sidecar packaging, app-owned `auv serve` lifecycle, signing/notarization/TCC risks, platform artifact matrix, and native computer-operation capability gap.
- [`2026-08-13-rust-browser-automation-interface-research.md`](2026-08-13-rust-browser-automation-interface-research.md) — current Rust Playwright bindings and WebDriver/CDP alternatives, with an AUV Rust Locator API recommendation.
- [`2026-08-13-playwright-inspired-auv-js-interface-research.md`](2026-08-13-playwright-inspired-auv-js-interface-research.md) — Playwright Locator、strictness、auto-wait/actionability 与 AUV Window/Display/Runner/typed evidence 的官方来源映射。
- [`2026-08-12-protobuf-openapi-typescript-sdk-research.md`](2026-08-12-protobuf-openapi-typescript-sdk-research.md) — primary-source comparison and implemented 20-operation daemon Rust/OpenAPI generation plus the generated `@auv-js/api-client` package consumed by `@auv-js/sdk`.
- [`2026-08-04-live-pairing-administration-decision.md`](2026-08-04-live-pairing-administration-decision.md) — accepted live administration and shared authenticated authority.
- [`2026-08-11-auv-js-sdk-spec.md`](2026-08-11-auv-js-sdk-spec.md) — implemented function-first JavaScript SDK, HTTP/WebSocket and Node transport, pairing, typed invoke, and AbortSignal contract.
- [`2026-08-11-browser-remote-control-protocol-research.md`](2026-08-11-browser-remote-control-protocol-research.md) — primary-source comparison of WebSocket, RDP, VNC/noVNC, Guacamole, WebRTC, and CDP for a browser AUV transport.
- [`2026-08-11-moonlight-sunshine-pairing-research.md`](2026-08-11-moonlight-sunshine-pairing-research.md) — source-level trace of Moonlight/Sunshine client-originated PIN pairing, certificate exchange, persistence, mTLS authentication, and revocation.
- [`2026-08-03-auv-facade-daemon-runner-architecture.md`](2026-08-03-auv-facade-daemon-runner-architecture.md) — accepted facade, daemon, opaque routing, extension, and Runner target.
- [`2026-07-31-device-run-runner-aggregated-api-design.md`](2026-07-31-device-run-runner-aggregated-api-design.md)
- [`2026-07-31-daemon-session-api-architecture.md`](2026-07-31-daemon-session-api-architecture.md)
- [`2026-08-02-api-client-server-package-architecture-research.md`](2026-08-02-api-client-server-package-architecture-research.md) — primary-source comparison and a capability-scoped client/server alternative.
- [`2026-08-03-rust-server-lifecycle-naming-research.md`](2026-08-03-rust-server-lifecycle-naming-research.md) — primary-source Rust server lifecycle and naming comparison.
- [`2026-08-03-auv-daemon-document-history.md`](2026-08-03-auv-daemon-document-history.md) — Git-backed timeline for when the independent `auv-daemon` owner became the accepted target.

- [`2026-06-10-stateful-session-daemon-js-repl-v0.md`](2026-06-10-stateful-session-daemon-js-repl-v0.md)
- [`2026-06-11-mcp-frontend-surface-v0.md`](2026-06-11-mcp-frontend-surface-v0.md)
- [`2026-06-11-mcp-read-chain-evidence-pack.md`](2026-06-11-mcp-read-chain-evidence-pack.md)
- [`2026-06-18-core-realtime-session-substrate-slice-design.md`](2026-06-18-core-realtime-session-substrate-slice-design.md)
- [`2026-06-18-core-realtime-session-substrate-v0.md`](2026-06-18-core-realtime-session-substrate-v0.md)
- [`2026-06-30-api-session-api-operator-guide.md`](2026-06-30-api-session-api-operator-guide.md)
- [`2026-06-30-api-session-proto-boundary-review.md`](2026-06-30-api-session-proto-boundary-review.md)
- [`2026-06-30-api-session-proto-server-seam-design.md`](2026-06-30-api-session-proto-server-seam-design.md)
- [`2026-06-30-session-api-closeout.md`](2026-06-30-session-api-closeout.md)

## Related

- Parent index: [`../INDEX.md`](../INDEX.md)
- Docs overview: [`../../../README.md`](../../../README.md)
- Shared vocabulary: [`../../../TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md)

## Related lifecycle evidence

- [Linux Portal authorization and local Runner reuse](../driver/2026-09-12-linux-portal-authorization-and-runner-reuse.md): SDK startup ownership, concurrent lazy Runner reuse, and independent Run IDs.
