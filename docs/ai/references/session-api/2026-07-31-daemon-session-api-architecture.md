# Daemon Session API Architecture (retired)

The experimental public `SessionService`, session-scoped `Connection`, legacy
`VisionService`, and handwritten `/v1/*:verb` routes were removed on
2026-07-31. Device, Run, Runner, and typed capability services now own the
public API.

The historical implementation record is archived at
[`../../../archive/verticals/session-api/2026-07-31-daemon-session-api-architecture.md`](../../../archive/verticals/session-api/2026-07-31-daemon-session-api-architecture.md).
The current architecture is documented in
[`2026-07-31-device-run-runner-aggregated-api-design.md`](2026-07-31-device-run-runner-aggregated-api-design.md).
