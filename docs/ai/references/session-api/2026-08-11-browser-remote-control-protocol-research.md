# Browser Remote-Control Protocol Research

Date: 2026-08-11

## Scope classification

`docs-only`

This note compares primary specifications, official documentation, and
first-party source for browser-capable remote-control transports. It is
research for a possible browser AUV SDK and WebSocket surface, not approval to
implement that surface. The current AUV deferral remains in
`crates/auv-api-server/src/rest.rs`: ordering, cursor, gap recovery,
cancellation, and video/frame streaming need a concrete consumer and an
owner-approved slice.

## Conclusion

A browser-facing WebSocket transport is feasible for AUV, but “use WebSocket”
answers only how bytes cross the browser boundary. Every mature comparison
system adds an application protocol that defines session identity, framing,
flow control, reconnect behavior, and the meaning of acknowledgements.

The best fit for AUV is therefore not a VNC-style untyped desktop tunnel and
not a second action-result model. It is a versioned binary WebSocket envelope
that preserves AUV's typed Protobuf requests and responses, routes them through
the existing `Device -> Run -> RunnerClass` model, and carries explicit request,
stream, sequence, cancellation, and recovery metadata. Continuous video should
remain a separate plane and can start later; typed operations, state events,
and bounded artifacts do not need a video protocol.

## What existing systems actually do

| System | Browser-facing transport | State/media model | Flow and recovery | Input result semantics |
| --- | --- | --- | --- | --- |
| noVNC + websockify | noVNC expects a WebSocket that exposes a standard RFB byte stream; websockify bridges WebSocket to the VNC server's TCP stream. noVNC can also consume an `RTCDataChannel`. | RFB sends encoded framebuffer rectangles. The client retains framebuffer state and normally requests incremental updates. | RFB is demand-driven: the server sends updates in response to client requests, which naturally lowers update rate for a slow client. An `RFB` object represents one connection; reconnect policy is outside the base WebSocket tunnel. | Key and pointer messages are input events, not typed commands. RFB defines no application-semantic success response for a click or key event. |
| Apache Guacamole | A web application tunnels the Guacamole instruction stream between JavaScript and `guacd`; WebSocket is preferred, with an HTTP tunnel fallback. | `guacd` translates RDP, VNC, and other protocols into a common drawing/input/stream instruction protocol. WebSocket messages are chunks of that instruction stream, not the domain protocol itself. | `sync` marks a logical frame and the client echoes it after prior operations complete; the server may stop updates until the client catches up. `ack` applies to received data blobs. `nop` is a keepalive. A connection ID can select an existing active connection, but this is join semantics rather than a generic replay cursor. | Mouse/key instructions describe delivery intent. Blob `ack` and frame `sync` do not prove that a target application reached a desired state. |
| RDP / RD Gateway | Microsoft's documented RD Gateway protocol tunnels RDP through HTTP or RPC-over-HTTP for the main channel and may add a UDP side channel. The official web client requires RD Gateway. Newer Azure Virtual Desktop paths begin with a brokered TCP reverse connection and prefer direct or relayed UDP where available. | RDP has compact binary PDUs for graphics and input and negotiates capabilities and transport paths. | RDP can reconnect to an existing session after a transient failure using a server-issued, session-bound auto-reconnect cookie. Azure RDP Shortpath uses ICE/STUN/TURN and falls back from UDP to TCP. | A fast-path input PDU means an input event was transported efficiently; it does not acknowledge application-semantic success. |
| WebRTC | Media uses RTP/RTCP through browser-managed senders/receivers; arbitrary messages use SCTP data channels over DTLS. ICE selects direct or relayed paths. | Media and data are separate primitives. Data channels may be ordered or unordered, reliable, retransmission-limited, or lifetime-limited. | `RTCDataChannel` exposes `bufferedAmount` and a low-water event. The browser stack supplies congestion control and selectable reliability, but the application still owns reconnect/session continuity and domain acknowledgements. | A data-channel `message` means a message arrived. It does not define whether an input operation was accepted, delivered by the OS, or semantically verified. |
| Chrome DevTools Protocol (CDP) | CDP is commonly carried as JSON messages over WebSocket, but its application protocol is request/response/event multiplexing rather than raw remote-display bytes. | Commands have numeric IDs; replies repeat the ID and contain `result` or `error`. Unsolicited events carry a method and parameters. Flattened sessions add `sessionId` to multiplex targets. | The protocol supplies correlation and target-session routing. A closed WebSocket does not itself provide durable replay or resume; clients reattach and reconstruct state from target/domain APIs. | A command result confirms CDP method processing. Input dispatch responses do not prove that the page or native application reached a desired business state. |

### Evidence behind the comparison

The RFB specification describes framebuffer updates as transitions between
valid framebuffer states, states that the client keeps a framebuffer copy, and
makes updates demand-driven by `FramebufferUpdateRequest`. It separately
defines `KeyEvent` and `PointerEvent` as client-to-server input messages. This
is a feedback loop for display transport, not a semantic action protocol
([RFC 6143](https://datatracker.ietf.org/doc/html/rfc6143)). noVNC's own API
states that one `RFB` object represents one connection and that its channel
must provide a standard RFB stream; its accepted channel types are WebSocket
and `RTCDataChannel`
([noVNC API](https://github.com/novnc/noVNC/blob/master/docs/API.md)).
websockify describes itself as a WebSocket-to-TCP bridge and separately offers
TLS, authentication, token-to-target selection, and traffic recording. Those
are gateway policies around RFB, not features supplied by WebSocket itself
([websockify README](https://github.com/novnc/websockify)).

Guacamole's official architecture puts a JavaScript client and tunnel in front
of `guacd`; the JavaScript library includes both WebSocket and HTTP tunnel
implementations
([Guacamole application guide](https://guacamole.apache.org/doc/gug/writing-you-own-guacamole-app.html),
[architecture](https://guacamole.apache.org/doc/1.5.4/gug/guacamole-architecture.html)).
The WebSocket endpoint sends and receives chunks of Guacamole instructions
([WebSocket tunnel API](https://guacamole.apache.org/doc/guacamole-common/org/apache/guacamole/websocket/GuacamoleWebSocketTunnelEndpoint.html)).
The protocol's `sync` exchange provides display-side backpressure, while
`ack` is scoped to stream blobs and `ready` supplies a connection identifier
that may later be selected to join an active connection
([Guacamole protocol reference](https://guacamole.apache.org/doc/gug/protocol-reference)).

Microsoft's RD Gateway specification documents HTTP or RPC-over-HTTP as the
main tunnel and UDP as an optional side channel
([MS-TSGU](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-tsgu/b68cd6a0-999f-46cf-9038-4d4b3afbc7c3));
the web client deployment guide requires RD Gateway
([RDS web client](https://learn.microsoft.com/en-us/windows-server/remote/remote-desktop-services/remote-desktop-web-client-admin)).
RDP fast-path input is a compact PDU carrying one or more keyboard, mouse, or
related events
([MS-RDPBCGR fast-path input](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/b8e7c588-51cb-455b-bb73-92d480903133)).
RDP automatic reconnection is an explicit application feature: the server
issues a cryptographic cookie bound to the session and only the last connected
client can use its current value
([MS-RDPBCGR automatic reconnection](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/15b0d1c9-2891-4adb-a45e-deb4aeeeab7c)).
Azure Virtual Desktop demonstrates a modern split: start with a brokered TCP
transport, attempt UDP via ICE/STUN/TURN, and fall back to TCP if UDP fails
([RDP Shortpath](https://learn.microsoft.com/en-us/azure/virtual-desktop/rdp-shortpath)).

WebRTC's browser API explicitly separates media senders/receivers from data
channels. A data channel has fixed ordering and reliability properties and
exposes send-queue watermarks; this is materially better suited to continuous
low-latency media or lossy cursor telemetry than a single reliable TCP stream,
but it brings signaling, ICE, DTLS, and reconnect complexity
([W3C WebRTC](https://www.w3.org/TR/webrtc/),
[RFC 8831](https://www.rfc-editor.org/rfc/rfc8831)).

CDP demonstrates the useful RPC pattern: each command has an `id`, responses
correlate with that `id`, events are independent messages, and `sessionId`
can route messages to attached targets. Its transport documentation also makes
clear that the WebSocket URL is discovered after an HTTP control request, not
treated as an authorization or resource model by itself
([CDP protocol](https://chromedevtools.github.io/devtools-protocol/),
[CDP Target domain](https://chromedevtools.github.io/devtools-protocol/tot/Target/),
[Chrome remote debugging protocol](https://chromedevtools.github.io/devtools-protocol/#how-do-i-access-the-browser-target)).

## WebSocket properties AUV must not mistake for protocol semantics

WebSocket supplies ordered, full-duplex text or binary messages over one
connection. RFC 6455 defines framing, ping/pong, and closing, but no RPC IDs,
authorization model, stream cancellation, replay cursor, or reconnection
semantics ([RFC 6455](https://www.rfc-editor.org/rfc/rfc6455)). The browser API
exposes queued outgoing bytes through `bufferedAmount`, but no low-water event;
applications must poll or otherwise schedule around it
([WHATWG WebSockets](https://websockets.spec.whatwg.org/)). Therefore AUV needs
protocol-level credit or acknowledgements for server-to-browser streams rather
than relying only on TCP or `bufferedAmount`.

The browser `WebSocket` constructor accepts a URL and optional subprotocols,
not arbitrary authorization headers. The opening handshake includes browser
credentials such as cookies, and the server can negotiate a subprotocol
([WHATWG WebSockets](https://websockets.spec.whatwg.org/)). A browser AUV
surface should consequently use `wss://`, validate `Origin`, and authenticate
with a same-site secure session cookie or a short-lived, single-use WebSocket
ticket minted over HTTPS. It should not expose the daemon's long-lived Device
bearer in JavaScript, a URL query, or a reusable subprotocol string.

Because one WebSocket is one ordered TCP byte stream, a large capture or frame
can delay following control messages. Application framing can prioritize before
enqueueing, but cannot remove transport head-of-line blocking after bytes have
entered the socket. Separate sockets/lanes or WebRTC become relevant only when
measurements show that bounded operation traffic and artifacts interfere with
interactive media.

## Implications for AUV

### Preserve the typed operation plane

The browser protocol should carry generated Protobuf payloads and stable
service/method identities, not JSON-shaped copies of every command and not raw
mouse/key events as the only abstraction. A minimal conceptual envelope needs:

```text
ClientHello { protocol_versions, sdk_version }
ServerHello { protocol_version, connection_id, limits }

OpenRun / BindRun { request_id, device_ref, run_ref? }
InvokeStart { request_id, run_id, runner_class, service, method, payload }
InvokeInput { request_id, sequence, payload }
InvokeHalfClose { request_id }
Cancel { request_id, reason }

InvokeOutput { request_id, sequence, payload }
InvokeEnd { request_id, status, error_details? }
Event { cursor, run_id?, kind, payload }
Credit { request_id_or_lane, through_sequence_or_bytes }
```

This is a proposed protocol shape, not an accepted schema. Exact names and
whether invocation routing remains opaque belong to an owner-approved design
slice. The important constraints are correlation, ordering per stream,
half-close/cancel support for the existing bidirectional input RPCs, bounded
message sizes, and preserving the owning Protobuf contracts.

### Make resume a resource decision, not a socket trick

A dropped socket must not imply that a `Run` ended, nor that unacknowledged
input can safely be replayed. RDP's session-bound rotating reconnect cookie is
a stronger model than reconnecting with the same bearer; Guacamole's ability
to join an active connection likewise relies on a server-side connection
resource. AUV should issue a short-lived connection/resume token bound to
caller, Device, Run, and permissions.

On resume, treat traffic by class:

- Idempotent observation requests may be retried with the same operation ID.
- Mutating input with an unknown terminal result must be reported as
  `outcome_unknown`; it must not be replayed automatically.
- Durable events need a cursor and replay window. If the cursor is older than
  retained history, return an explicit gap and require a fresh snapshot.
- Ephemeral pointer-progress or video frames may be dropped and restarted from
  current state.
- Run completion and cancellation remain explicit control operations rather
  than consequences of WebSocket GC or disconnect.

### Keep delivery acknowledgement separate from semantic verification

The comparison protocols acknowledge transport, frames, blobs, or RPC method
processing. None can infer that a click opened the intended menu. AUV already
has the correct seam: `InputActionResult` is delivery evidence, not semantic
proof. A WebSocket response should preserve that result exactly. If a caller
requires semantic success, it must request or perform a separate verification
operation and receive a distinct verification result.

Useful acknowledgement layers are:

1. socket/message accepted into the transport queue;
2. request decoded and authorized by the browser gateway/daemon;
3. operation accepted by the routed Runner;
4. Driver delivery result (`InputActionResult`);
5. optional semantic verification result.

Collapsing these into one `ok` boolean would regress the current contract.

### Separate control, artifacts, and future media

The first browser slice should prove typed invocation and bounded outputs:
connect, select Device, create/bind Run, invoke one observation method and one
input method, receive `InputActionResult`, cancel, and finish the Run. Captures
can initially be bounded binary messages or authenticated artifact URLs.

Full remote-desktop presentation is a different producer/consumer contract:

- VNC/noVNC is appropriate when the product is fundamentally a framebuffer
  mirror and raw input channel.
- Guacamole is appropriate when one gateway must normalize several desktop
  protocols into browser drawing instructions.
- WebRTC is appropriate when encoded live video/audio, congestion adaptation,
  NAT traversal, and lower-latency lossy data justify its operational cost.
- AUV's typed operation surface should remain useful without any continuous
  display stream. If video lands later, correlate its session with Device and
  Run but do not make video frames into operation responses.

## Recommended next design slice

Before implementation, approve one narrow protocol design for:

1. one `wss://` endpoint and browser-session authentication;
2. version negotiation and binary envelope limits;
3. request/response/event correlation over one connection;
4. unary invocation plus cancellation, with an explicit deferral marker for
   client/server/bidirectional streaming if they are not included;
5. reconnect classification for idempotent reads versus unknown mutating
   outcomes;
6. one typed observation and one typed input vertical through the existing
   Runner route;
7. no continuous video in the same slice.

That slice would test the actual AUV requirement. Starting with generic remote
desktop streaming would instead optimize the archived visual-control shape
before proving that browser callers can use AUV's shared typed execution model.

## Primary sources

- [RFC 6455: The WebSocket Protocol](https://www.rfc-editor.org/rfc/rfc6455)
- [WHATWG WebSockets Standard](https://websockets.spec.whatwg.org/)
- [RFC 6143: The Remote Framebuffer Protocol](https://datatracker.ietf.org/doc/html/rfc6143)
- [noVNC API](https://github.com/novnc/noVNC/blob/master/docs/API.md)
- [websockify README](https://github.com/novnc/websockify)
- [Apache Guacamole architecture](https://guacamole.apache.org/doc/1.5.4/gug/guacamole-architecture.html)
- [Apache Guacamole protocol reference](https://guacamole.apache.org/doc/gug/protocol-reference)
- [Apache Guacamole application guide](https://guacamole.apache.org/doc/gug/writing-you-own-guacamole-app.html)
- [MS-TSGU: Remote Desktop Gateway Server Protocol](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-tsgu/b68cd6a0-999f-46cf-9038-4d4b3afbc7c3)
- [MS-RDPBCGR: Fast-Path Input Event PDU](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/b8e7c588-51cb-455b-bb73-92d480903133)
- [MS-RDPBCGR: Automatic Reconnection](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/15b0d1c9-2891-4adb-a45e-deb4aeeeab7c)
- [Azure Virtual Desktop RDP Shortpath](https://learn.microsoft.com/en-us/azure/virtual-desktop/rdp-shortpath)
- [W3C WebRTC](https://www.w3.org/TR/webrtc/)
- [RFC 8831: WebRTC Data Channels](https://www.rfc-editor.org/rfc/rfc8831)
- [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/)
- [Chrome DevTools Protocol Target domain](https://chromedevtools.github.io/devtools-protocol/tot/Target/)
