# Protobuf JSON Schema Library Research

Date: 2026-08-17

Status: Research complete; no implementation decision beyond the current
runtime projector.

## Question

Can a maintained JavaScript or TypeScript library replace
`js/packages/sdk/src/apis/auv/json.ts`, which converts a dynamically discovered
Protobuf-ES `DescMessage` into JSON Schema for AUV API input and output?

## Verdict

No evaluated library is a drop-in replacement.

The closest authoritative implementation is Buf's
[`protoschema-jsonschema`](https://github.com/bufbuild/protoschema-plugins), but
it is an alpha Go/protoc generation plugin. It does not accept a Protobuf-ES
`DescMessage`, cannot run in a browser, and is designed for build-time schemas,
not for an AUV client discovering a previously unknown plugin at runtime.

For the current dynamic-discovery requirement, retain AUV's small Protobuf-ES
descriptor projector. Treat Buf's generator and the official
[`ProtoJSON` specification](https://protobuf.dev/programming-guides/json/) as
semantic references and conformance oracles rather than adding a second
Protobuf runtime.

There are two viable future paths:

1. Keep runtime discovery and harden the current projector with explicit input
   and output modes plus conformance tests.
2. Move schema production to plugin build time and transport the generated
   schema with plugin metadata. Buf's generator then becomes a strong candidate,
   but this changes the plugin protocol and requires an owner-approved contract
   slice.

## Required Shape

- Input is a Protobuf-ES runtime `DescMessage`, obtained through gRPC reflection.
- Output must be usable immediately in Node.js and browser clients.
- Recursive messages need stable `$defs` and `$ref` handling.
- Message fields use ProtoJSON names.
- Real Protobuf `oneof` fields must remain sibling JSON properties with an
  at-most-one constraint.
- 64-bit integers, enums, maps, repeated fields, and well-known types must
  follow ProtoJSON rather than the Protobuf wire representation.
- No generated SDK or local `.proto` source is assumed for a newly discovered
  plugin.

One important ambiguity should be made explicit before further expansion:
ProtoJSON parsers accept a broader language than canonical serializers emit.
For example, parsers accept both JSON and proto field names, numeric strings
for numeric fields, enum names or numbers, and `null` for most fields. Canonical
serialization uses JSON field names, decimal strings for 64-bit integers, enum
names, and normally omits default-valued fields. One schema used for both tool
input and output cannot precisely describe both sets.

## Candidate Comparison

| Candidate | Accepts `DescMessage` | Browser runtime | JSON Schema | ProtoJSON/oneof fit | Verdict |
| --- | --- | --- | --- | --- | --- |
| AUV projector | Yes | Yes | Draft 2020-12 | Designed for current contract | Keep for now |
| Buf `protoschema-jsonschema` | No | No | Yes | Strong reference, not identical | Best build-time option |
| AsyncAPI protobuf parser | No; raw `.proto` | Yes, with its parser stack | Yes | Incompatible oneof and int64 shape | Reject |
| `protobuf-jsonschema` | No; file path | No | Yes | Old, incomplete ProtoJSON | Reject |
| `protoc-gen-jsonschema` variants | No | No | Yes | Configurable, build-time only | Possible build-time alternatives |
| `protobuf.js` | No; separate descriptors | Yes | No | Separate reflection runtime | Not a converter |
| `ts-proto` | No; protoc input | Build-time generator | No | Generates JSON codecs/types | Not a converter |
| `proto3-json-serializer` | No; protobuf.js types | Node-oriented | No | JSON codec only | Not a converter |

## Findings

### Protobuf-ES does not provide this API

The SDK already uses
[`@bufbuild/protobuf`](https://github.com/bufbuild/protobuf-es). Its public
exports include descriptors, registries, and ProtoJSON conversion, but not a
JSON Schema generator. Its `json_types=true` generation option creates
TypeScript types representing JSON values; it does not create runtime JSON
Schema. Protobuf-ES remains the right reflection model for AUV, but a projector
is still necessary.

### Buf `protoschema-jsonschema`

Buf's official
[`protoschema-plugins` README](https://github.com/bufbuild/protoschema-plugins/blob/main/README.md)
documents a `protoschema-jsonschema` protoc/Buf plugin and labels the project
alpha with unstable APIs. The implementation is Go code built around Go
`protoreflect` descriptors, with recursion handling, bundled definitions,
well-known-type mappings, and Protovalidate support.

It is the strongest semantic reference found, but not a runtime package. Source
inspection also found no projection of native Protobuf `oneof` membership into
AUV's at-most-one sibling-property constraint. This is an inference from the
current generator source, not a documented compatibility promise.

Its normal and strict modes also target different accepted JSON languages.
Normal mode deliberately accepts several ProtoJSON parser conveniences; strict
mode rejects numeric strings and adds implicit-default requirements. Neither is
an exact substitute for AUV's current canonical-output-oriented schema.

### AsyncAPI protobuf schema parser

[`@asyncapi/protobuf-schema-parser`](https://github.com/asyncapi/protobuf-schema-parser)
parses raw `.proto` text through protobuf.js as part of AsyncAPI document
processing. It does not consume Protobuf-ES descriptors. Its import support is
limited to bundled Google and validation protos, which is also a poor match for
arbitrary reflected plugin descriptors.

The generated model is incompatible with AUV's call JSON in two material ways:

- `int64` is emitted as JSON Schema `integer` with `format: int64`, while
  canonical ProtoJSON emits a decimal string.
- A Protobuf `oneof` is represented as a property named after the oneof group,
  containing alternatives. ProtoJSON instead places the selected member field
  directly on the enclosing message object.

Adapting it would require translating descriptors, correcting its schema model,
and importing an AsyncAPI/protobuf.js stack: more code than the projector.

### `protobuf-jsonschema`

[`protobuf-jsonschema`](https://github.com/devongovett/protobuf-jsonschema)
is an old Node package whose source synchronously reads a `.proto` file and
parses it with `protocol-buffers-schema`. The last npm release is from 2018. It
does not accept reflection descriptors or support browser execution.

Its source covers basic messages, enums, repeated fields, and maps, but has no
native oneof projection or ProtoJSON well-known-type model. It is not suitable
for modernization or wrapping.

### Build-time protoc generators

[`pubg/protoc-gen-jsonschema`](https://github.com/pubg/protoc-gen-jsonschema)
is a maintained Go protoc plugin with Draft 2020-12 support, JSON-name and oneof
options, and a flag to represent ProtoJSON `int64` as strings. Its well-known
type coverage is narrower than the current AUV projector. It is credible when
the `.proto` files are known during the build, but cannot project a reflected
`DescMessage` inside a browser.

[`chrusty/protoc-gen-jsonschema`](https://github.com/chrusty/protoc-gen-jsonschema)
similarly offers build-time options for oneof enforcement, string-only enums,
and string big integers. It is another generator choice, not a runtime library.

### Libraries that solve adjacent problems

[`protobuf.js`](https://github.com/protobufjs/protobuf.js) provides its own
runtime reflection graph and JSON codecs, but no JSON Schema generator. Adopting
it would require translating Protobuf-ES descriptors into a second runtime.

[`ts-proto`](https://github.com/stephenh/ts-proto) is a protoc TypeScript code
generator. It can emit `fromJSON`/`toJSON` code and metadata for other code
generators, but its `outputSchema` option is not JSON Schema and it does not
operate on dynamically discovered descriptors.

[`proto3-json-serializer`](https://github.com/googleapis/google-cloud-node-core/tree/main/packages/proto3-json-serializer-nodejs)
serializes and deserializes protobuf.js messages, including well-known types.
It is a JSON codec, not a JSON Schema converter, and uses protobuf.js types
rather than Protobuf-ES descriptors.

## Current Projector Gaps Worth Testing

Keeping the local projector does not mean its present behavior is complete.
Primary-source comparison exposed these concrete gaps or policy questions:

- Input and output currently share one schema despite different ProtoJSON
  accepted languages.
- Parser aliases, numeric strings, enum numbers, and accepted `null` values are
  not comprehensively described for inputs.
- Unknown enum numeric values may appear in output, while the schema currently
  lists enum names only.
- `google.protobuf.NullValue` has the special JSON value `null`; treating it as
  an ordinary enum would be incorrect.
- Numeric ranges and exact well-known-type formats are only partially encoded.
- `Any` is necessarily loose unless the embedded type registry is available.

## Recommendation

Do not add any evaluated dependency merely to remove `json.ts`. AUV's current
implementation is small, uses the already-authoritative Protobuf-ES descriptor
model, and uniquely satisfies runtime browser discovery.

If runtime discovery remains the contract, the next narrow slice should define
whether `inputSchema` describes parser-accepted input and `outputSchema`
describes canonical output, then add conformance fixtures for recursion, oneof,
64-bit integers, enums, and well-known types.

If AUV instead chooses build-time schemas, define a versioned schema artifact
in plugin metadata and evaluate Buf's official generator first. That route can
remove most runtime projection policy, but it must specify discovery transport,
schema/version compatibility, and behavior when an older plugin omits the
artifact.

## Research Method

This review used primary sources only: official specifications, package source,
package manifests, and maintainers' repositories. Package popularity or search
ranking was not used as evidence of semantic compatibility.
