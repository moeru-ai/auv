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

`source_width` and `source_height` describe the stored artifact before this
fetch resizes it. `width` and `height` describe the returned PNG.
For capture artifacts, the receipt attributes `image.source_width` and
`image.source_height`, when present, describe native captured pixels before
logical downscaling. These values can differ on a display with a backing scale factor.

Capture evidence from `display.capture`, `window.capture`, and
`screen.captureRegion` uses logical resolution. The producer already divides
the native dimensions by the capture's backing scale factor.
See [capture evidence](../../../../crates/auv-cli-invoke/src/artifact.rs) and
[image resolution](../../../../crates/auv-tracing/src/image.rs).

For a returned PNG pixel `(px, py)`, use this screen coordinate formula:

```text
x = capture.bounds.x + px * source_width / width
y = capture.bounds.y + py * source_height / height
```

Use `capture.bounds.x` and `capture.bounds.y` from that artifact's attributes
in the `invoke` result. Do not divide by the capture's scale factor again.
For example, a native 2880×1800 capture at scale 2 produces a 1440×900 artifact.
A fetch bounded to 720×450 returns that size. Its pixel `(360, 225)` maps to
logical offset `(720, 450)` from the capture bounds origin.
The formula applies to capture artifacts with bounds attributes. A `scan.frame`
fixture has no implied screen mapping.

The server instructions and both tool descriptions direct clients to
`artifact_read_image`. An `invoke` result's `file_path` refers to the server
filesystem; a client does not need filesystem access to fetch the image.

Unknown fields, including a file path or store root, produce an invalid-params
error. Invalid IDs, unavailable receipts, and refused image data produce the
same error category. Storage I/O failures, changed file length or digest, and
PNG encoding failures produce an internal-error response. These storage checks
compare a registered file with its verified write receipt. A mismatch describes
server state, rather than an invalid client parameter. The handler preserves
these categories from the reader.

## Invoke filesystem authority

MCP `invoke` serves trusted clients with the server process's filesystem
permissions. `project_root` selects defaults; it does not confine explicit paths.
The image receipt catalog restricts fetch eligibility, not command filesystem
authority.

An explicit `invoke.store_root` can create a tracing store outside the project.
`scan.frame` can load an external fixture directory and return an image receipt.
That receipt becomes eligible for fetching after a successful recording flush.
The fixture loader also resolves the image name from its manifest without
confining the resulting path to the fixture directory.
See [MCP authority](../../../../crates/auv-cli/src/commands/mcp.rs),
[scan invocation](../../../../crates/auv-cli-invoke/src/commands/scan.rs), and
[fixture loading](../../../../crates/auv-scan/src/producer/mod.rs).

Workspace confinement requires a separately approved frontend path policy.
That policy must cover store roots, fixture directories, manifest image paths,
and symlink resolution. Restricting only the fixture directory cannot confine
the image path. This PR preserves the existing invoke path authority.

The default frontend creates a tracing worker for each invoke. The worker
exits after its producer handles close and queued work completes; creating a
worker per request does not itself establish a thread leak.
See [dispatch lifecycle](../../../../crates/auv-tracing/src/dispatch.rs).
Concurrent-request admission remains a separate scheduling decision.

## Receipt scope and limits

Only successful `invoke` results with a successful tracing flush add image
receipts. A failed flush preserves the command result and `recording_failure`,
but omits `artifacts` from the response and does not register new receipts.
The artifact file can already exist before flush fails. File persistence alone
does not admit a receipt to this catalog.
The catalog holds the latest 128 distinct image receipts in return
order. Returning an existing receipt refreshes its position. Catalog expiry
does not delete a stored artifact.

Catalog locks recover from poison, preserve complete receipts, and finish any
pending FIFO eviction. Recovery does not replace a completed `invoke` result
with a protocol error. A replacement receipt is prepared before the old receipt
is removed. A panic during preparation therefore preserves the old receipt.
Recovery emits a server warning.

OCR source captures emitted without a returned `invoke` receipt stay outside
this catalog. Exposing that evidence requires an owner-approved command receipt
contract. This tool does not scan a Run for additional images.

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
| Decoder internal allocation budget | 128 MiB, best effort |
| External decoded pixel buffer | 128 MiB |
| Encoded output PNG | 8 MiB |

One decoder reads both the header and the pixels. For PNG, its constructor
receives limits before header parsing, because compressed metadata can expand
during this step. In image 0.25.10, JPEG and WebP receive limits after header
parsing. The 16 MiB encoded-source limit applies before every decoder constructor.
The reader checks the external pixel buffer size separately before pixel
decoding. These checks do not share a remaining allocation balance. Metadata
and pixel buffers can coexist and exceed 128 MiB together. Source bytes,
allocator overhead, resizing buffers, and PNG encoding buffers also consume
process memory outside these checks.

In image 0.25.10, `PngDecoder::set_limits` changes the wrapper limits without
changing the underlying `png::Reader` allocation balance.
See the [dependency source](https://docs.rs/image/0.25.10/src/image/codecs/png.rs.html#264-272).
The PNG reader keeps the constructor limits and does not set them again after the
header parse. A strict combined cap requires allocation accounting from each
codec. That work remains deferred until an owner-approved slice adds this
accounting.

An oversized optional ICC profile does not always reject a PNG. In png 0.18.1,
`parse_iccp` ignores profile parsing errors, so the decoder can discard that
profile and continue with the pixels.
See the [PNG parser source](https://github.com/image-rs/image-png/blob/2a3f980245e3ae38b82ade96533e7b450e8477bb/src/decoder/stream.rs).
The 160 MiB ICC fixture below exercises this behavior. Its successful pixels do
not contain a retained 160 MiB profile.

Image resizing uses the Triangle filter. This avoids the integer overflow in
the image 0.25.10 thumbnail path for 16-bit PNG images.
Assignment replaces the decoded image after resizing, releasing the original
pixel buffer before PNG encoding. After encoding, the reader explicitly drops
the final image before output checks, Base64 allocation, and response construction.
Shadowing the original image would retain its buffer until the enclosing scope
ends. See the [Rust drop rules](https://doc.rust-lang.org/reference/destructors.html#drop-scopes).
These release points shorten buffer lifetimes; they do not define a process
memory cap or a measured peak-memory reduction.

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
16-bit pixel values after resizing, discarded ICC profiles, catalog recovery,
and the 8 MiB output limit with a smaller successful request.
The [MCP transport test](../../../../crates/auv-cli/src/commands/mcp_test.rs)
covers discovery, the request schema, explicit image delivery, and rejection
of images from another server instance. Tool-level tests preserve completed
invoke results after catalog poison and classify storage failures as internal.
Additional regressions cover replacement panic recovery, coordinate instructions,
and a 2× capture through the shared producer, WebP storage, and PNG resizing.
Storage regressions assert internal-error codes for changed length and digest.
A failing telemetry exporter reproduces flush failure after a successful file
write and verifies the omitted receipt, preserved result, and fetch refusal.
An explicit-path fixture verifies the existing authority boundary through
`invoke(scan.frame)` and `artifact_read_image`, using temporary files outside
the project directory.

Windows verification of the current reader, based on `main@8bf7e4f1`:

| Command | Result |
| --- | --- |
| `cargo build --locked -p auv-device-helper-windows --bin auv-helper` | Passed |
| `cargo test --workspace --locked --no-fail-fast` | 1,640 passed, 25 ignored, 0 failed |
| `cargo check --locked` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo run --quiet --locked -- invoke --help` | Passed |
| `cargo run --quiet --locked -- mcp serve --help` | Passed |
| `git diff --check` | Passed |

The workspace run used `AUV_WINDOWS_HELPER_EXE_PATH` to select the built helper.
It includes 48 MCP tests. The cache regression first failed with UUID-ordered
eviction. It passes after receipt-ordered eviction.

Header and 16-bit resizing regressions first failed against `d85ea044`.
Catalog poison and storage error classification regressions first failed
against `0990c064`. All pass with the current reader. The output-limit test
uses incompressible RGB pixels below the source limit. It confirms refusal of
PNG output above 8 MiB and success after a smaller size request.

The ICC regression confirms that a 160 MiB profile is absent from the limited
PNG decoder, while the returned image contains the expected pixel. This
distinguishes profile refusal from pixel refusal.

Coordinate instructions and duplicate-receipt panic regressions first failed
against `f5c0ff7b`. The capture fixture records 24×12 native pixels at scale 2,
stores 12×6 logical pixels, and fetches a 6×3 PNG. Its receipt retains native
dimensions while the fetch reports stored and returned dimensions separately.

Storage corruption and flush-admission regressions first failed against
`f9ce0bbf`. The path-authority fixture passes at that revision and the current
reader, confirming an existing boundary rather than a new path restriction.
Windows debug MIR from `cargo rustc --locked -p auv-cli --lib -- --emit=mir`
with rustc 1.98.1 shows both image drops after Base64 encoding on the old resize
path. With the fix,
assignment releases the original image before PNG encoding, and explicit drop
releases the final image before Base64 encoding. This checks destruction order,
not allocator or operating-system memory return.

Additional real MCP stdio checks at `0990c064` used `scan.frame` fixtures:

- A 512×512 RGBA16 PNG shrank to 1×1 with channel values
  `[65535, 50000, 30000, 65535]` intact.
- A header with a 127 MiB decompressed ICC profile and 2 MiB EXIF chunk failed
  during header parsing with an invalid-params error.
- A 163,167-byte PNG whose compressed ICC payload represents 160 MiB returned
  its pixels after the decoder discarded the over-budget profile. Sampled
  process private memory peaked near 204 MiB during the read, compared with
  402 MiB before the fix. These samples do not define a process memory cap.

The images in these tests come from fixtures. Desktop capture and input
support remain defined by the invoke registry and their platform evidence.
