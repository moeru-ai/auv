# Playground agent execution contract

Status: implemented.
This note defines the MCP interface, connection lifetime, execution rules, and evidence limits.
`codemode` names the internal execution mechanism. The MCP tool name is `execute`.
The [Playground README](../../../../apps/repl-playground/README.md#agent-execution) contains setup instructions and script examples.

## Exterior connection

The Playground uses `@exterioragentprotocol/sdk` at revision `aec6c44ae53b0a896f7b206a0e3b4a56a73fbc7e`.
[`exterior.ts`](../../../../apps/repl-playground/src/features/agent/exterior.ts) adapts account access, device discovery, and callback configuration.
Exterior owns authorization, token refresh, HTTP MCP transport, endpoint credentials, and relay cleanup.
AUV owns the selected execution device, Mock desktop, session store, and timeline.

`VITE_EXTERIOR_URL` selects the relay origin. The default is `http://127.0.0.1:8787`.
The default client identity is `https://auv.moeru.ai/playground/oauth-client.json`.
The build includes [the metadata document](../../../../apps/repl-playground/public/oauth-client.json), which needs publication at that HTTPS address.
A registered `VITE_EXTERIOR_CLIENT_ID` supports local development before publication.
The callback follows the application base path: `/playground/oauth/callback`.
App registration and account authorization are separate steps.

The account SDK supplies live tokens through its account methods.
`listTargets` maps each device's agents to selectable targets and retains the device's working directory.
The agent device and selected AUV Device are independent.

The Connect action creates a page-owned `AcpThreadController` in [`agent-session.ts`](../../../../apps/repl-playground/src/features/agent/agent-session.ts).
React views subscribe to this controller.
Disconnect and Reconnect dispose it. A page reload closes its sockets.
StrictMode view remounts retain the existing connection.

Startup prepares the MCP endpoint with `serveMcp`, then requests the single-use ACP ticket.
The page connects a fresh MCP SDK `Server` for each MCP session.
The endpoint closes after failed startup, controller cancellation, or ACP connection closure.
Canceled startup cannot update a replacement connection. Startup failures require explicit reconnect.

The agent must advertise `agentCapabilities.mcpCapabilities.http`.
ACP `session/new.mcpServers` receives:

```ts
{
  type: 'http',
  name: 'auv-playground',
  url: tools.url,
  headers: tools.headers,
}
```

Tool discovery can finish before ACP session creation returns.
The agent runs its ordinary ACP command.
The typed `codeModeConnection` adapter supplies the endpoint through `newSession` and forwards cancellation.
The ACP SDK owns message parsing and request matching.
The controller owns negotiated capabilities, connection errors, and active-session selection.

One endpoint belongs to one ACP connection.
Agents can share that endpoint across sessions, so the host binds execution to its active ACP session.
The local [React ACP patch](../../../../patches/@hafbit__react-acp@0.1.9.patch) exposes `useAcpControllerRuntime` without connection lifecycle effects.
An upstream release that exports this interface can replace the patch.

## MCP tool

The page exposes `execute({ code, timeoutMs? })`.
Its description supplies execution rules and SDK discovery instructions.
ACP user prompts pass through unchanged.

| Parameter | Advertised input | Runtime check |
| --- | --- | --- |
| `code` | Required string, length 1–100,000. | A nonblank string with JavaScript `length` at most 100,000. |
| `timeoutMs` | Optional integer, 1–120,000. Default: 30,000. | The same range and integer check. A missing or null value uses 30,000. |

The low-level MCP `Server` handles protocol requests. It does not apply the advertised tool schema automatically.
[`CodeModeHost`](../../../../apps/repl-playground/src/features/agent/codemode-bridge.ts) separately checks arguments before execution.
The schema declares `additionalProperties: false`; the runtime guard checks only `code` and `timeoutMs`.

Scripts receive `auv`, admitted `sdk` helpers, `api`, `text`, `image`, and `store/load`.
The SDK declaration catalog supplies `api.help()`, `api.search()`, and `api.describe()`.
These synchronous queries make no device calls.
Search returns paths and signatures. Detailed help returns types, examples, and restrictions.
Related type names also work as describe paths.
Numeric SDK enums retain their values.

MCP results contain text/image `content` and `isError`.
Only explicit output and execution status return to the agent; partial output survives failure and cancellation.
The final text item describes completion, failure, cancellation, or timeout.
The interface does not expose internal `RunOutcome` metadata.

OpenCode can wrap the MCP call in its own execution tool.
Its wrapper status and nested Playground status are distinct.
The [Agent UI](2026-10-25-playground-agent-ui.md) retains both.
The [interface research](2026-10-20-codemode-agent-interface-research.md) compares these execution models.

## Execution and state

Execution requires an active ACP session.
The host rejects calls when its `#active` invocation exists or the shared `session.running` getter returns true.
The shared check also covers editor execution. `ExecSession.run` rejects calls while a worker is active.
Busy calls return an error. There is no queue or automatic retry.

[`ExecSession`](../../../../apps/repl-playground/src/runtime/session.ts) keeps the editor worker until Reset.
Each agent call owns a temporary worker, RPC connection, and SDK port.
Cleanup closes these resources after success, failure, cancellation, or timeout.
Each call creates a fresh QuickJS/WASM context with a 64 MiB guest memory limit.
The guest has no filesystem, network, browser, or Node globals.
SDK methods remain callable on returned objects, including `WindowClient`.

The VM clones the committed JSON store and validates each local update.
The host commits candidate state only after an `ok` outcome without cancellation.
Failure, timeout, and cancellation preserve the committed store.
Session replacement cancels the invocation and clears the store.

Request cancellation combines the MCP request signal with the active invocation signal.
Each request has its own signal, including requests with equal IDs on different transports.
Delivered device input remains effective after failure or cancellation.

Calls, captures, output, and full statement and binding history append across agent invocations.
Event positions increase across calls.
Each SDK call retains its source script and line offset.
A metadata-only call preserves the latest capture.

The editor retains separate REPL variables; editor bindings and line highlights exclude other scripts.
An editor run starts a separate timeline.
These rules do not provide persistent chat or replay of an entire agent session.

## Limits and Mock behavior

The compiler erases TypeScript syntax without type checking each script.
SDK calls require the documented argument shapes.
Protobuf encoding can omit unknown fields, so an incorrect selector can reach the default display.
Any stricter validation belongs at the shared SDK serialization boundary.

SDK callbacks and async iterators cannot cross the VM boundary.
Local callbacks such as `Array.map` work.
Function-bearing SDK clients cannot pass through `text()` or the JSON store; output their IDs and metadata instead.
Manual Run, stepping, and replay use the editor REPL environment.
Streaming tool progress remains deferred at its code marker.

`image(capture)` accepts an AUV capture reference and fetches bounded pixels on the host.
Pixels stay outside the VM and session store.
SDK help describes call contracts; the selected device determines platform support.
Script completion proves execution. Application state must establish semantic success.

The Mock Runner implements selected desktop behavior:

| Operation | Behavior |
| --- | --- |
| Region capture | Non-finite coordinates or non-positive dimensions return `INVALID_ARGUMENT` (3). |
| Keyboard | Text typing and pasting work. Key presses support Return/Enter; other keys and shortcuts return `UNIMPLEMENTED` (12). |
| Keyboard validation | Empty sequences, empty key combinations, and missing actions return `INVALID_ARGUMENT` (3). All actions pass validation before delivery. |
| Text search | An empty trimmed query returns no matches. |

These checks do not establish native platform capabilities.
Complete statement recording also adds CPU-loop overhead.
A million-iteration browser loop exceeded its ten-second deadline in the recorded probe.
The normal browser harness excludes this performance case.
The optional `AUV_CODEMODE_BENCHMARK=1` probe reproduces it.
It is not a passing performance guarantee.

## Evidence

Evidence level: automated browser integration with QuickJS, the actual SDK, Mock Runner, synthetic Exterior/ACP peers, and real MCP HTTP transports.
The Vitest browser tests cover [execution](../../../../apps/repl-playground/agent/execution.browser.test.tsx) and [connections](../../../../apps/repl-playground/agent/connection.browser.test.tsx).
They use the Playwright provider and synthetic network peers.
Co-located browser component tests cover Markdown, clipboard access, tool errors, and image dialogs with an in-memory ACP peer.
The tests check:

- Tool discovery before session creation and execution without a device for pure code and SDK help.
- Returned help examples against the actual SDK, display selection, large arrays, and bounded image output.
- Cancellation, timeout recovery, store rollback, retained captures, source history, and editor isolation.
- Connection retention across StrictMode remounts, canceled startup, explicit reconnect, and unsupported-agent handling.
- Unchanged user prompts in first and later sessions, current account-token use, and inspectable tool results.

Host unit tests use the MCP SDK client and linked transports.
They replace worker execution to check admission, store commits, and cancellation decisions.
State tests inspect the next invocation's input store.
VM tests run the compiler and QuickJS with supplied host functions to check marshalling and execution rules.
Mock tests check capture errors, keyboard validation before effects, and empty text queries.

Validation on 2026-10-29 passed: 110 Node tests and six browser tests through the default `test:run` command.
The browser project also passed with `CI=true` using Playwright Chromium on Windows.
Playground typecheck, changed-file ESLint, production build, and `git diff --check` also passed during the browser-test migration.
Full Playground lint still reports six pre-existing sorting errors in `src/store.ts`, which this migration does not change.
Combined runs complete the Node project before browser compilation.
The root test command and CI include both projects. Use `test:unit` or `test:browser` to run one project.
Earlier SDK capture validation passed 21 regressions.
Run the checks from the repository root:

```sh
pnpm -F @auv-js/repl-playground test:run
pnpm -F @auv-js/repl-playground typecheck
pnpm -F @auv-js/repl-playground test:browser
pnpm --filter @auv-js/sdk exec vitest run src/apis/auv/driver.test.ts --project node
git diff --check
```

The local SDK typecheck resolves TypeScript 5.4.5.
It reports three `AbortSignal.any` errors and one `readline` type error at both the PR base and head.
The base check uses source revision `739fe399311ae72b77ffeea530b231c3bb6c34ea`, with the same installed dependencies and generated bindings.
Explicit TypeScript 6.0.3 checks pass for both revisions. The default SDK command still fails.

Evidence level: a live remote Relay probe with ordinary local OpenCode 2.0.26 and model inference.
The page connected through `https://relay.apps.superpowerlulu.win` to the Mock desktop.
Two calls checked a Counter click, screenshot output, retained store data, and fresh variables.
This probe used the tool name `codemode`; the current `execute` name has automated coverage only.
It does not establish physical desktop behavior, cross-device agent support, or complete SDK coverage.

## Sources

- [Exterior SDK serving tools](https://github.com/lulu0119/exterior/blob/aec6c44ae53b0a896f7b206a0e3b4a56a73fbc7e/packages/sdk/README.md#serving-tools-to-an-agent)
- [Exterior endpoint ownership](https://github.com/lulu0119/exterior/blob/aec6c44ae53b0a896f7b206a0e3b4a56a73fbc7e/docs/adr/0030-a-web-app-serves-mcp-through-the-relay.md)
- [Exterior app setup](https://github.com/lulu0119/exterior/blob/aec6c44ae53b0a896f7b206a0e3b4a56a73fbc7e/docs/app-setup.md)
- [ACP MCP server configuration](https://agentclientprotocol.com/protocol/v1/session-setup#mcp-servers)
