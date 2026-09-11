# Rust server lifecycle naming research

Date: 2026-08-03

## Question

How do established Rust server projects name the module, lifecycle owner,
bound-listener state, serving operation, and post-start control surface?

This note records naming evidence for the `auv-api-server` boundary. It is not
an accepted rename proposal.

## Method

The GitHub CLI code-search endpoint was attempted first, but the authenticated
search quota was exhausted. The repositories below were then cloned with
`gh repo clone --depth=1` and inspected at the fixed commits linked below.

## Primary-source evidence

| Project | File/module | Lifecycle shape | Control after startup |
| --- | --- | --- | --- |
| Axum | `axum/src/serve/mod.rs` | [`serve(listener, service)` returns a concrete `Serve` future](https://github.com/tokio-rs/axum/blob/98884fb7bebb457af520f6e59c71249d03311c90/axum/src/serve/mod.rs#L103-L116), rather than hiding the lifecycle in an async helper. | [`Serve` exposes `with_graceful_shutdown` and `local_addr`](https://github.com/tokio-rs/axum/blob/98884fb7bebb457af520f6e59c71249d03311c90/axum/src/serve/mod.rs#L200-L255). |
| Tonic | `tonic/src/transport/server/` | [`transport::Server` is explicitly a builder](https://github.com/hyperium/tonic/blob/7a0d0be671f8ded25c31765546af289fad8b194d/tonic/src/transport/server/mod.rs#L104-L143), consumed by `serve` or an incoming-stream variant. | Shutdown is supplied to [`serve_with_shutdown` or `serve_with_incoming_shutdown`](https://github.com/hyperium/tonic/blob/7a0d0be671f8ded25c31765546af289fad8b194d/tonic/src/transport/server/mod.rs#L677-L766). [`TcpIncoming` separately owns binding and `local_addr`](https://github.com/hyperium/tonic/blob/7a0d0be671f8ded25c31765546af289fad8b194d/tonic/src/transport/server/incoming.rs#L83-L135). |
| Actix Web / actix-server | `actix-web/src/server.rs`, `actix-server/src/server.rs` | [`HttpServer::new(...).bind(...)`](https://github.com/actix/actix-web/blob/bf2c859d4c9e16a3f74d3d5d363e06b95d98bf6f/actix-web/src/server.rs#L75-L125) retains bound addresses, then [`run()` returns a `Server` future](https://github.com/actix/actix-web/blob/bf2c859d4c9e16a3f74d3d5d363e06b95d98bf6f/actix-web/src/server.rs#L1275-L1292). | The running [`Server` yields a `ServerHandle`](https://github.com/actix/actix-net/blob/194ec108c3273f9a2abedb26609eb0385698c026/actix-server/src/server.rs#L125-L154), whose [`pause`, `resume`, and `stop` methods](https://github.com/actix/actix-net/blob/194ec108c3273f9a2abedb26609eb0385698c026/actix-server/src/handle.rs#L9-L52) are cloneable control capabilities. |
| Salvo | `crates/core/src/server.rs` | A bound acceptor is passed to [`Server::new`](https://github.com/salvo-rs/salvo/blob/165e059ad46948d8cadfc526ad5ef42d11fe1a06/crates/core/src/server.rs#L120-L163); `Server` exposes bound `holdings` and is consumed by [`serve`](https://github.com/salvo-rs/salvo/blob/165e059ad46948d8cadfc526ad5ef42d11fe1a06/crates/core/src/server.rs#L245-L257). | [`ServerHandle` provides graceful and forceful stop](https://github.com/salvo-rs/salvo/blob/165e059ad46948d8cadfc526ad5ef42d11fe1a06/crates/core/src/server.rs#L29-L112), and is obtained from the server before serving. |
| Rocket | `core/lib/src/rocket.rs`, `shutdown/handle.rs` | Rocket uses a product-named type-state lifecycle instead of `Server`: `Rocket<Build>` becomes `Rocket<Ignite>` and is consumed by [`launch`](https://github.com/rwf2/Rocket/blob/3a54d079aef060a8f732bd04ea54b0581a604087/core/lib/src/rocket.rs#L1184-L1226). | The instance yields a cloneable [`Shutdown`](https://github.com/rwf2/Rocket/blob/3a54d079aef060a8f732bd04ea54b0581a604087/core/lib/src/rocket.rs#L630-L654), whose [`notify()`](https://github.com/rwf2/Rocket/blob/3a54d079aef060a8f732bd04ea54b0581a604087/core/lib/src/shutdown/handle.rs#L68-L104) requests graceful shutdown. |
| Tokio mini-redis | `src/server.rs` | The official example exposes a flat [`run(listener, shutdown)`](https://github.com/tokio-rs/mini-redis/blob/3d93b42bc363220f85af4fc9e1bebd35b588a4a3/src/server.rs#L123-L155), but immediately builds a private stateful [`Listener`](https://github.com/tokio-rs/mini-redis/blob/3d93b42bc363220f85af4fc9e1bebd35b588a4a3/src/server.rs#L1-L45). | Only the supplied shutdown future is public. This is a useful small-server/tutorial contrast, not a rich daemon-control surface. |

## Findings

The dominant vocabulary separates these responsibilities:

- `server` names the lifecycle/configuration owner or its module;
- `bind` creates or attaches listeners and makes endpoint discovery reliable;
- `serve`, `run`, or `launch` names the consuming execution operation;
- `ServerHandle` or `Shutdown` is a cloneable capability when callers must
  interact with the running server;
- `incoming`, `listener`, or `acceptor` names the lower-level transport input.

`serve` is therefore not inherently a bad module name: Axum uses it for the
operation and returned future. For a crate-specific daemon boundary that owns
configuration, multiple listeners, readiness, and shutdown, however, `server`
is the stronger recurring module/type vocabulary.

## AUV outcome

Before this refactor, AUV already had a lifecycle instance under transport
vocabulary:

```text
bind(ApiServeConfig) -> BoundApi
BoundApi::{endpoint, endpoints, discovery_endpoint}
BoundApi::serve(self, CancellationToken)
```

The owner-approved narrow refactor made that role explicit without changing
its lifecycle semantics:

```text
Server::bind(ServerConfig) -> Server
Server::{endpoint, endpoints, discovery_endpoint}
Server::serve(self, CancellationToken)
```

The implementation now lives under `auv_api_server::server`, while `serve`
remains the consuming execution operation. This follows the Actix and Salvo
distinction between a server instance and its execution operation.

A dedicated `ApiServerHandle` should be introduced only when AUV has a concrete
post-start control requirement, such as an owned shutdown capability, readiness
observation, endpoint discovery from another task, or configuration reload. The
external `CancellationToken` already supplies shutdown signalling; duplicating
it solely for naming symmetry would add surface area without new policy.

Under that vocabulary, Runner gRPC forwarding is private server
request-dispatch machinery under `server::runner_grpc_proxy`. Runner-side
inherited IPC is the separate public `runner_transport` module: it is an
incoming transport adapter and is not grouped with daemon server orchestration
merely because the two meet on the same end-to-end connection path.
