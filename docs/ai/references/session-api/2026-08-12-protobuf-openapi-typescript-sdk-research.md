# Protobuf-to-OpenAPI and TypeScript SDK Research

Date: 2026-08-12

Status: daemon unary Rust/OpenAPI generation implemented; JavaScript slice deferred

## Question

Can AUV replace handwritten REST routing and JavaScript RPC projection code
with a pipeline that starts from protobuf/gRPC definitions, generates OpenAPI,
and then uses Hey API to generate a JavaScript/TypeScript SDK?

## Verdict

Yes, but OpenAPI generation alone is not enough.

AUV currently exposes a handwritten **protobuf-binary-over-HTTP** protocol.
Hey API generates ordinary HTTP clients from OpenAPI and is therefore a natural
fit for an **HTTP/JSON** API, not for a body whose field numbers and binary
codec are known only from protobuf descriptors. A successful migration needs
both halves below to agree:

```text
.proto + google.api.http
  -> generated Rust HTTP/ProtoJSON runtime
  -> generated OpenAPI
  -> generated Hey API TypeScript client
```

The best bounded experiment is one unary service, `PairingService`, using
`google.api.http`, ProtoJSON, a Rust generated-route candidate, and an actual
Hey API client integration test. Do not replace the whole REST layer based only
on a successfully generated OpenAPI file.

For the first experiment:

- use `protoc-gen-openapiv2` as the stable specification baseline;
- separately evaluate `tonic-rest 0.1.5` as the Rust runtime candidate;
- generate an exact-pinned Hey API client from the result;
- retain a small handwritten AUV facade for domain conveniences.

The newer `protoc-gen-openapiv3` is attractive because it emits OpenAPI 3.1 and
models ProtoJSON directly, but its own documentation labels it Alpha and tells
production users to stay with the v2 generator for now.

## Implementation outcome

The owner approved the Rust half of the experiment on 2026-08-12, first for
PairingService and then for every daemon-owned unary service. On 2026-08-13,
the owner approved the generated JavaScript package and its integration into
`@auv-js/sdk`. The current implementation:

- annotates 20 RPCs across Discovery, Device, Pairing, Run, Runner, and
  RunnerClass with `google.api.http`;
- uses exact-pinned `tonic-rest-build 0.1.5` to generate Axum JSON handlers
  which call the existing Tonic service adapters;
- authenticates each protected request in one listener middleware, then stores
  `CallerId` in the request extensions;
- forwards `CallerId` from Axum into generated Tonic requests, so REST and gRPC
  adapters consume the same authenticated request context;
- keeps WebSocket credential authentication in the `Open` frame because the
  browser protocol does not send that credential in the HTTP request;
- configures the Pairing messages for the ProtoJSON shapes used here, including
  camel-case names, original proto-name aliases, default-value omission,
  `Duration`, and `Timestamp`;
- generates `proto/openapi/auv-daemon.swagger.json`, containing 18 paths and
  20 operations, through exact-pinned grpc-gateway `openapiv2 v2.30.0`;
- generates the independent `@auv-js/api-client` package under `proto/sdk/js`
  through exact-pinned `@hey-api/openapi-ts 0.99.0`;
- uses those generated operations for daemon-owned unary HTTP calls in
  `@auv-js/sdk`, while the same facade continues to use protobuf RPCs for gRPC and
  Unix transports;
- keeps AUV-owned conveniences such as `ttlMs`, `Date`, generated Device IDs,
  selectors, enum validation, and narrowed resource results in `@auv-js/sdk`;
- verifies Discovery, Device, Pairing, Run, Runner, and RunnerClass JSON routes
  with daemon integration tests and verifies the
  unchanged gRPC path with both a real `auv serve` process and the `auv`
  pairing CLI.

## Current AUV shape

The repository now has one source for daemon REST paths and two generated
consumers:

1. The daemon v1 proto files own all daemon-service HTTP paths through
   `google.api.http`.
2. `crates/auv-api-server/src/rest.rs` mounts generated JSON routes for
   all six daemon services. Dynamic Runner invocation remains the separate
   protobuf/WebSocket transport because its services come from runtime
   descriptors rather than daemon-owned concrete Tonic implementations.
3. `proto/sdk/js` turns that Swagger artifact into `@auv-js/api-client`.
   `js/packages/sdk/src/apis/auv-daemon` binds the generated operations to
   the existing protobuf request/response descriptors and contains no daemon
   REST path literals.

One detail still matters: the pairing facade is not only
transport boilerplate. It also maps `ttlMs` to
  `google.protobuf.Duration`, maps `Timestamp` to `Date`, generates a default
  device ID, and presents narrower return values. OpenAPI/Hey API can replace
  the raw path, request, response, and Fetch client layer; it should not be
  expected to invent this AUV-owned facade policy.

## The contract that must change

The standard `google.api.http` mapping describes HTTP verbs, resource paths,
path parameters, query parameters, and request bodies. Its JSON representation
is ProtoJSON. ProtoJSON is not equivalent to adding ordinary Serde derives to
prost structs:

- field names are lower camel case;
- enums serialize as names;
- `bytes` serialize as base64;
- 64-bit integers serialize as decimal strings;
- `Timestamp` and `Duration` have special string formats;
- unknown fields are rejected by conforming parsers.

The protobuf documentation also warns that ProtoJSON has weaker schema
evolution guarantees than binary protobuf. This makes a move from the current
binary body a real wire-contract decision, not an implementation-only cleanup.

Source: [ProtoJSON format](https://protobuf.dev/programming-guides/json/).

If AUV must keep `application/protobuf` on its browser REST transport, Hey API
is not the natural generator. In that branch, continuing with
`protoc-gen-es` and generating HTTP bindings from protobuf descriptors would
preserve the current wire model more directly.

## Candidate comparison

| Candidate | Produces runtime? | OpenAPI | Fit for AUV | Main concern |
|---|---:|---:|---|---|
| gRPC-Gateway `protoc-gen-openapiv2` | Go gateway only | 2.0 | Best stable spec baseline; Buf-friendly | Does not generate a Rust/Axum runtime |
| gRPC-Gateway `protoc-gen-openapiv3` | Go gateway only | 3.1 | Strong ProtoJSON schemas and paths | Explicitly Alpha |
| `tonic-rest` | Rust/Axum | 3.1 companion tool | Closest end-to-end Rust match | Very young; binding and WKT limitations |
| `tonic2axum` | Rust/Axum | utoipa-generated | Version-compatible and supports streaming modes | Default JSON/error behavior does not match AUV/ProtoJSON closely enough |
| Envoy gRPC-JSON transcoder | Envoy sidecar/filter | Separate generator needed | Mature, schema-driven runtime | Conflicts with AUV's small single-binary/local-daemon shape |
| `protoc-gen-connect-openapi` | No | 3.1 | Useful spec experiment and Buf remote plugin | Community Connect-oriented tool; does not add a Tonic runtime |
| Gnostic/Kolla `protoc-gen-openapi` | No | 3.x | Can generate a spec | Lower priority than gRPC-Gateway's current generators |
| `pbjson-build` | JSON codec only | No | Useful compatibility component | Does not generate routes or OpenAPI |
| `utoipa` alone | Rust-derived docs | 3.x | Good for Rust-first REST APIs | Duplicates the proto-owned contract in Rust annotations |

### gRPC-Gateway generators

`protoc-gen-openapiv2` is the established option. It consumes
`google.api.http`, can describe custom paths and bindings, and has additional
annotations for schemas, operations, security, responses, and headers. It
emits Swagger/OpenAPI 2.0. The normal gRPC-Gateway runtime generator emits a Go
reverse proxy, so AUV can use the OpenAPI generator without adopting that
runtime.

Sources:

- [gRPC-Gateway introduction](https://grpc-ecosystem.github.io/grpc-gateway/docs/tutorials/introduction/)
- [OpenAPI v2 customization](https://grpc-ecosystem.github.io/grpc-gateway/docs/mapping/customizing_openapi_output/)
- [Buf remote-plugin usage](https://github.com/grpc-ecosystem/grpc-gateway#usage-with-remote-plugins)

`protoc-gen-openapiv3` emits OpenAPI 3.1 directly from `google.api.http`. Its
documented mapping includes `additional_bindings`, body/path/query splitting,
string enums, string 64-bit integers, well-known types, and a default
`google.rpc.Status` error. It does not consume the v2 customization annotations
and its output may still change around oneofs, wrappers, enums, and path
templates.

Source: [gRPC-Gateway OpenAPI 3.1 output](https://grpc-ecosystem.github.io/grpc-gateway/docs/mapping/openapi_v3/).

### `tonic-rest`

`tonic-rest` reads `google.api.http` from a descriptor set and generates Axum
handlers which call the same Tonic service trait. Its companion
`tonic-rest-openapi` produces and patches an OpenAPI 3.1 document. Version
`0.1.5` matches AUV's current `tonic 0.14`, `prost 0.14`, and `axum 0.8` stack.
It also provides adapters for common well-known types and proto enum JSON.

This is the most relevant Rust experiment, but not yet a safe default:

- the project is at `0.1.x` with narrow adoption;
- `additional_bindings` are not supported;
- only `body: "*"` and no-body bindings are supported, not partial body fields;
- repeated well-known-type fields are not auto-wired;
- AUV must explicitly prove that its HTTP authentication produces the same
  caller authority as the current gRPC and handwritten REST paths;
- AUV must test all used ProtoJSON edge types, especially its `uint64` fields,
  rather than assuming ordinary Serde output is conformant.

Sources:

- [`tonic-rest` repository](https://github.com/zs-dima/tonic-rest)
- [`tonic-rest-build` limitations](https://docs.rs/tonic-rest-build/latest/tonic_rest_build/#planned)
- [`tonic-rest-openapi` documentation](https://docs.rs/tonic-rest-openapi/latest/tonic_rest_openapi/)

### `tonic2axum`

`tonic2axum` also reads `google.api.http`, generates Axum routes, can generate
utoipa OpenAPI declarations, and has HTTP/WebSocket streaming modes. Its crate
versions also match `tonic 0.14`, `prost 0.14`, and `axum 0.8`.

It is not the first recommendation because its current runtime serializes
prost structs through general Serde and returns plain-text Tonic errors. That
does not match AUV's current RFC 9457 error body, and raw prost Serde is not a
complete ProtoJSON contract. Its path parser also documents support for only
simple nested variables.

Source: [`tonic2axum` repository](https://github.com/nu11ptr/tonic2axum).

### Envoy and Connect-oriented tooling

Envoy's gRPC-JSON transcoder is a mature runtime driven by descriptors and
`google.api.http`, but it introduces an extra process/configuration boundary.
It is a reasonable fallback for an already-Envoy-based deployment, not the
first fit for a local AUV daemon and Unix-socket workflow.

Source: [Envoy gRPC-JSON transcoder](https://www.envoyproxy.io/docs/envoy/latest/configuration/http/http_filters/grpc_json_transcoder_filter.html).

`protoc-gen-connect-openapi` emits OpenAPI 3.1 and supports Connect endpoints,
gRPC-Gateway annotations, Protovalidate, and a Buf community remote plugin. It
does not make the existing Tonic server speak Connect or install AUV's custom
REST routes, so it is a useful generator comparison rather than the preferred
runtime path.

Source: [`protoc-gen-connect-openapi` repository](https://github.com/sudorandom/protoc-gen-connect-openapi).

## Hey API fit

Hey API accepts OpenAPI input and can generate TypeScript types, SDK functions,
schemas, and clients for Fetch, Axios, and other transports. Its documentation
labels the package as being in initial development and recommends pinning an
exact version.

The generated output can replace:

- repeated request/response TypeScript declarations;
- handwritten Fetch calls;
- hardcoded REST verb/path pairs;
- standard bearer header configuration;
- raw OpenAPI-described response and error types.

It does not replace:

- Tonic/gRPC clients;
- protobuf binary encoding;
- AUV's `AuvConnection` transport selection and routed-runner semantics;
- domain conveniences such as `ttlMs`, `Date`, generated device IDs, or return
  value narrowing;
- server authorization and error policy.

Source: [Hey API get started](https://heyapi.dev/docs/openapi/typescript/get-started).

## Implemented Hey API package

The generated package and `@auv-js/sdk` integration satisfy these gates:

- server routes and generated client paths come from the same annotations;
- local-owner, unauthenticated enrollment, and paired-bearer cases match the
  existing authority model;
- Duration, Timestamp, enums, absent fields, and 64-bit integers have canonical
  ProtoJSON and matching OpenAPI schemas;
- the generated client correctly handles the chosen structured error body;
- cancellation still reaches Fetch through `AbortSignal`;
- no second app/domain execution path is introduced;
- the browser bundle does not regain a Node-only gRPC dependency;
- generation is reproducible; CI clean-diff enforcement remains a candidate
  follow-up when generated-artifact checks are added to the repository CI.

Two OpenAPI contract gaps remain explicit: the Swagger document does not yet
describe the listener-wide bearer default with the public `PairDevice`
exception, and it does not describe RFC 9457 error responses. `@auv-js/sdk` supplies
the bearer header and preserves its structured `AuvHttpError` mapping. Adding
those declarations is a separate schema-contract slice rather than a second
route list.

## Recorded decision boundary

The daemon-owned unary API selected the general REST/TypeScript SDK direction:
its HTTP mapping is now JSON/ProtoJSON and OpenAPI-backed. This does not turn
runtime-provided driver services into static daemon REST handlers, and it does
not justify deleting the domain-level pairing facade.
