# Device / Run / Runner API

Device/Run/Runner control API, protobuf, Runner aggregation, MCP frontend, and
the tombstone for the retired SessionService prototype. The folder name is
retained as a stable responsibility path; Session is not a public resource.

Target architecture: [`2026-07-31-device-run-runner-aggregated-api-design.md`](2026-07-31-device-run-runner-aggregated-api-design.md).
The Device/Run/Runner contracts are implemented. Public SessionService,
session-scoped Connection, legacy VisionService, and `/v1/*:verb` routes were
removed on 2026-07-31.

Current implementation ownership: `auv-api-proto` owns the wire contract,
`auv-api-server` owns control/capability serving, `auv-api-client` owns typed
transport and placement clients, and `auv-cli` hosts the process frontend. MCP
remains in `auv-cli`.

Count: **11**

- [`2026-07-31-device-run-runner-aggregated-api-design.md`](2026-07-31-device-run-runner-aggregated-api-design.md)
- [`2026-07-31-daemon-session-api-architecture.md`](2026-07-31-daemon-session-api-architecture.md)

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
