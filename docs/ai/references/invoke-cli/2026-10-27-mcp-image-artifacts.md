# Explicit MCP image artifact reads

Status: implemented. Evidence level: automated fixtures through the MCP tool
router and transport on Windows. Verification results appear below.

## Purpose and ownership

An MCP client can receive a capture artifact reference without access to the
server filesystem. `artifact_read_image` returns image content on an explicit
request. Normal `invoke` results continue to return artifact references and
metadata.

The MCP frontend owns the request schema, receipt catalog, and MCP content.
`auv-tracing` owns the stored artifact bytes and their write receipts. Image
reads do not select a Device, repeat a capture, or upload pixels from the client.
`GetCaptureImage` remains the explicit fetch for a live `CaptureRef`.

## Request and response

The CLI entry point remains `auv mcp serve`. The new tool accepts:

| Field | Required | Meaning |
| --- | --- | --- |
| `run_id` | Yes | Canonical, non-nil UUID from the artifact URI |
| `artifact_id` | Yes | Canonical, non-nil UUID from the artifact URI |
| `max_width` | No | Maximum PNG width, from 1 to 4096 pixels. Default: 2048 |
| `max_height` | No | Maximum PNG height, from 1 to 4096 pixels. Default: 2048 |

For example, an `invoke` result can contain this artifact URI:

```text
auv://runs/019a2000-0000-7000-8000-000000000001/artifacts/019a2000-0000-7000-8000-000000000002
```

The corresponding MCP `tools/call` parameters are:

```json
{
  "name": "artifact_read_image",
  "arguments": {
    "run_id": "019a2000-0000-7000-8000-000000000001",
    "artifact_id": "019a2000-0000-7000-8000-000000000002",
    "max_width": 1024,
    "max_height": 1024
  }
}
```

These IDs are illustrative. A client uses the URI from its own `invoke` result.

The response contains a JSON text block and standard MCP PNG image content.
The text contains `run_id`, `artifact_id`, `source_width`, `source_height`,
`width`, and `height`. The output preserves the aspect ratio. Small images
retain their original dimensions.

Unknown fields, including a file path or store root, produce an invalid-params
error. Invalid IDs, unavailable receipts, and refused image data produce the
same error category.

## Receipt scope and limits

Only successful `invoke` results with a successful tracing flush add image
receipts. The catalog holds the latest 128 distinct image receipts in return
order. Returning an existing receipt refreshes its position. Catalog expiry
does not delete a stored artifact.

Each `McpServer` has its own catalog. Clones of that server share the catalog.
The stdio entry point creates one server for its connection. Another server
instance cannot fetch those images by knowing their IDs.

The reader accepts PNG, JPEG, and WebP source artifacts. It checks the stored
length, SHA-256 digest, and image format before decoding. Limits apply per
request:

| Resource | Limit |
| --- | --- |
| Encoded source | 16 MiB |
| Source dimension | 8192 pixels on each axis |
| Source pixel count | 16,777,216 pixels |
| Decoder allocation budget | 128 MiB |
| Encoded output PNG | 8 MiB |

One decoder reads both the header and the pixels. Its limits apply before
header parsing, because compressed PNG metadata can expand during this step.
The reader reserves the pixel buffer from the allocation budget before pixel
decoding. The image crate treats this budget as best effort. It does not cap
process memory, allocator overhead, resizing buffers, or PNG encoding buffers.

Image resizing uses the Triangle filter. This avoids the integer overflow in
the image 0.25.10 thumbnail path for 16-bit PNG images.

An output-size error asks the client to request a smaller image. File reads,
decoding, scaling, and encoding run on a blocking worker. Catalog locks cover
receipt access only.

Durable lookup across server restarts requires a separate authorized store
reader contract. Aggregate concurrent-read budgets require a separate MCP
scheduling contract. Both remain deferred for an owner-approved slice.

## Verification

The [image tests](../../../../crates/auv-cli/src/commands/mcp_image_test.rs)
cover PNG decoding, dimensions, aspect ratio, receipt scope,
invalid paths and IDs, byte integrity, content type, source limits, and catalog
expiry. PR #430 regression tests also cover the header allocation budget and
16-bit pixel values after resizing.
The [MCP transport test](../../../../crates/auv-cli/src/commands/mcp_test.rs)
covers discovery, the request schema, explicit
image delivery, and rejection of images from another server instance.

Initial Windows verification at `d85ea044`, based on `main@8bf7e4f1`:

| Command | Result |
| --- | --- |
| `cargo build --locked -p auv-device-helper-windows --bin auv-helper` | Passed |
| `cargo test --workspace --locked --no-fail-fast` | 1,628 passed, 25 ignored, 0 failed |
| `cargo check --locked` | Passed |
| `cargo fmt --check` | Passed |
| `cargo run --quiet --locked -- invoke --help` | Passed |
| `cargo run --quiet --locked -- mcp serve --help` | Passed |
| `git diff --check` | Passed |

The workspace run used `AUV_WINDOWS_HELPER_EXE_PATH` to select the built helper.
It includes 36 MCP tests. The cache regression first failed with UUID-ordered
eviction. It passed after receipt-ordered eviction.

The images in these tests come from fixtures. Desktop capture and input
support remain defined by the invoke registry and their platform evidence.

### PR #430 follow-up verification

Two regression tests first failed against `d85ea044`. The dimension probe
admitted oversized metadata before the 128 MiB decode budget applied. The
16-bit image test failed with an integer overflow. Both tests pass with one
decoder limited before header parsing and Triangle resizing.

Windows verification after the fixes:

| Command | Result |
| --- | --- |
| `cargo test --locked -p auv-cli --lib commands::mcp::` | 38 passed, 0 failed |
| `cargo test --workspace --locked --no-fail-fast` | 1,630 passed, 25 ignored, 0 failed |
| `cargo build --locked -p auv-device-helper-windows --bin auv-helper` | Passed |
| `cargo check --locked` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo run --quiet --locked -- invoke --help` | Passed |
| `cargo run --quiet --locked -- mcp serve --help` | Passed |
| `git diff --check` | Passed |

The workspace run selected the built helper through
`AUV_WINDOWS_HELPER_EXE_PATH`. Real MCP stdio requests also used `scan.frame`
fixtures to exercise the following cases:

- A 512×512 RGBA16 PNG shrank to 1×1 with channel values
  `[65535, 50000, 30000, 65535]` intact.
- A header with a 127 MiB decompressed ICC profile and 2 MiB EXIF chunk failed
  during header parsing with an invalid-params error.
- A 163,167-byte PNG with a 160 MiB decompressed ICC profile returned its pixels.
  Sampled process private memory peaked near 204 MiB during the read, compared
  with 402 MiB before the fix. These samples do not define a process memory cap.
