# Playground agent execution contract

Status: implemented.
This note defines the MCP interface, connection lifetime, execution rules, and evidence limits.
`codemode` names the internal execution mechanism. The MCP execution tool is `execute`.
The [Playground README](../../../../apps/repl-playground/README.md#agent-execution) contains setup instructions and script examples.

## Exterior connection

The Playground uses the EAP SDK and the separate Client SDK previews pinned in `pnpm-workspace.yaml`.
[`exterior.ts`](../../../../apps/repl-playground/src/features/agent/exterior.ts) adapts account access, device discovery, and callback configuration.
Exterior's protocol SDK owns authorization, token refresh, and HTTP MCP routing. The Client SDK serves a single execution tool backed by Pi Code Mode.
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
The page connects a fresh MCP SDK `McpServer` for each MCP session.
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

## MCP execution

The page exposes `execute({ code, timeoutMs? })`. `code` is nonblank JavaScript
up to 100,000 characters. `timeoutMs` ranges from 1 to 120,000 and defaults
to 30,000. The Client SDK validates the arguments through the official MCP
SDK and returns text, images, and an MCP error flag. ACP user prompts pass
through unchanged.

Pi renders declarations from the app's registered operations and supplies
the same operations to JavaScript as `tools.*`. The script can use
`ALL_TOOLS` to search the operation names and descriptions. The
[Playground's AUV adapters](../../../../apps/repl-playground/src/features/agent/codemode-tools.ts)
call the existing Runner SDK and expose plain data. SDK clients remain in
the page and a script uses IDs such as `windowId` or a capture reference.

## Execution and state

Execution requires an active ACP session. Only one Agent script or editor
execution can use the selected desktop at a time. An overlapping request
receives a busy error. The page starts an AUV Run for an execution with a
selected backend and associates SDK calls with it.

Each Agent call runs in a fresh Pi sandbox on a browser worker. The page can
carry Pi's successful JSON store updates across calls in the same
conversation and device context. Changing that context clears the store.
An MCP cancellation, ACP cancellation, timeout, context change or disconnect
stops execution and aborts pending operations. Delivered actions remain in
effect.

The Agent timeline records script source, SDK operations and their results,
and captures. A capture returns a reference. An explicit image operation can
provide bounded PNG or JPEG output. The editor has its own REPL, stepper,
variable inspection, and replay.

## Evidence

The browser integration uses real HTTP MCP clients, synthetic Exterior and
ACP peers, Pi's browser worker, the AUV SDK, and a Mock Runner. See the
[execution fixture](../../../../apps/repl-playground/agent/execution.browser.test.tsx)
and [connection fixture](../../../../apps/repl-playground/agent/connection.browser.test.tsx).
The [agent interface research](2026-10-20-codemode-agent-interface-research.md)
describes the earlier design.

An earlier live probe reached the Mock desktop from OpenCode 2.0.26 through the
hosted relay using a prior `codemode` tool. Verification with a real external
Agent and the Pi-backed `execute` tool on the hosted relay is pending.

## Sources

- [Exterior SDK serving tools](https://github.com/lulu0119/exterior/blob/aec6c44ae53b0a896f7b206a0e3b4a56a73fbc7e/packages/sdk/README.md#serving-tools-to-an-agent)
- [Exterior endpoint ownership](https://github.com/lulu0119/exterior/blob/aec6c44ae53b0a896f7b206a0e3b4a56a73fbc7e/docs/adr/0030-a-web-app-serves-mcp-through-the-relay.md)
- [Exterior app setup](https://github.com/lulu0119/exterior/blob/aec6c44ae53b0a896f7b206a0e3b4a56a73fbc7e/docs/app-setup.md)
- [ACP MCP server configuration](https://agentclientprotocol.com/protocol/v1/session-setup#mcp-servers)
