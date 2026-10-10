# Device entry errors as gRPC status

Status: implemented on 2026-10-11. Owner decisions: switch in one step, and use
the status-code mapping below.

## Change

The `DeviceService` session RPCs (`ListUserSessions`, `GetUserSession`,
`EnsureUserSessionUnlocked`, `EnsureUserSessionLocked`) used to report a policy
or OS-state failure as **gRPC OK** with `oneof result { …; DeviceEntryError
error }`. Interceptors, tracing, metrics and retry policy therefore saw those
failures as successes, and every client had to unwrap two layers.

A daemon now fails these RPCs with a gRPC status:

- The status code follows the reason (table below).
- `grpc-status-details-bin` is a standard `google.rpc.Status` whose `details`
  hold exactly one `Any` with type URL
  `type.googleapis.com/auv.api.daemon.v1.DeviceEntryError`. It carries the
  same typed `DeviceEntryErrorReason` as before.
- The message is `Device entry failed: <REASON_NAME>`, so clients without the
  typed decoder still show the reason.

`auv_api_proto::device_entry` holds the mapping, the encoder (`status`) and the
decoder (`reason`); both the daemon and the Rust SDK use it.

| Reason | gRPC code |
| --- | --- |
| `UNAUTHORIZED`, `DISABLED` | `PERMISSION_DENIED` |
| `UNENROLLED`, `SUSPENDED`, `CREDENTIAL_REJECTED`, `OCCUPIED_DESKTOP`, `UNSUPPORTED_OS_STATE`, `HOST_INCOMPATIBLE` | `FAILED_PRECONDITION` |
| `AMBIGUOUS_USER` | `INVALID_ARGUMENT` |
| `STALE_SESSION` | `ABORTED`: list sessions again, then retry |
| `SERVICE_UNAVAILABLE`, `AUDIT_UNAVAILABLE` | `UNAVAILABLE` |
| `OUTCOME_UNVERIFIED` | `UNKNOWN`: the OS may or may not have applied the change |

Argument errors that never reach the policy (for example an empty
`session_selector`) stay plain `INVALID_ARGUMENT` statuses with no
`DeviceEntryError` detail.

## Client surfaces

- Rust `auv::devices`: a status with the detail becomes
  `DeviceError::Entry(reason)`; other statuses stay `DeviceError::Client`.
- JS `@auv-js/sdk`: the Node gRPC transport keeps `grpc-status-details-bin` on
  `AuvRpcError.details`. `listUserSessions`, `getUserSession` and
  `ensureUserSessionUnlocked` rethrow a status with the detail as
  `AuvDeviceEntryError`. The SDK does not generate `google.rpc.Status`, so it
  reads the `details` field directly (`NOTICE(device-entry-status-decode)`).
- MCP and the CLI read the Rust SDK error, so their output is unchanged.

Callers of the SDKs see the same typed errors as before.

## Compatibility

| Client | Daemon | Result |
| --- | --- | --- |
| New | New | Typed reason from the status |
| New | Older than 2026-10-11 | Typed reason from the in-body `oneof` (`NOTICE(device-entry-legacy-error)`) |
| Older | New | Success is unchanged. A failure is a generic RPC error whose message names the reason; the typed reason is not available |

The owner accepted the third row for the 0.0.x series: the operation still
fails correctly, and only the type of the error is lost until the client is
upgraded.

The `oneof … DeviceEntryError error = 2` fields stay in the proto, documented
as no longer sent, so newer clients can still read older daemons.
`TODO(device-entry-error-oneof-removal)` marks the proto fields and the Rust
read path. Remove them, and reserve field 2 in each response, once no supported
daemon sends the in-body error.

## Evidence

- `auv-api-proto`: every reason round-trips through a status, codes follow the
  table, the details decode as a standard `google.rpc.Status`, and unrelated or
  malformed details yield no reason. A shared byte fixture
  (`unenrolled_status_matches_the_shared_fixture`) pins the exact encoding that
  the JS test decodes.
- `auv-daemon` server tests call the real gRPC service over a Unix socket and a
  paired HTTP listener, and assert the status code and the typed reason for all
  four RPCs.
- Rust SDK tests cover the status path, a plain status, and the legacy in-body
  error. JS SDK tests cover the status path for all three SDK methods, plain
  and malformed statuses, and the legacy in-body error.
