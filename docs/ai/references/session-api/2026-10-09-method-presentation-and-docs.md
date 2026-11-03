# Method presentation and docs

Date: 2026-10-09

Status: **accepted** (owner, 2026-10-09). The first slice is implemented. Names
marked *provisional* are open.

> NOTICE (2026-11-03): `presentation.name` is now the lower camelCase operation
> id shared with invoke and MCP (`window.findText`, not `window.find_text`), and
> several RPCs moved services (`FindWindowText` is in `WindowService`). See
> [API naming normalization](2026-11-02-driver-api-naming-normalization.md).
> gRPC paths in the Problem section below are historical.

## Problem

Clients that relay or discover Runner RPCs know only gRPC paths, such as
`/auv.api.driver.v1.TextRecognitionService/FindWindowText`. The playground's
Calls list showed those paths. Discovery had no title or description to show
(`TODO(discovered-tool-presentation)` in the SDK), and there was nowhere to
keep docs and examples a reader could copy.

## Contract

Each method carries two kinds of documentation, delivered in two ways:

| What | Where it is written | How a client gets it |
|---|---|---|
| `name`, `title`, `description` | `option (auv.api.annotations.v1.presentation)` on the RPC | With the method descriptors: reflection, and the SDK's generated code |
| Long-form docs and examples | `docs/<name>.md` next to the proto package | On request: `MethodDocsService.GetMethodDocs` |

### `presentation`

- **`name`.** The operation id: `<service resource>.<rpc without the resource
  noun>` in lower camelCase, for example `window.findText` and
  `media.togglePlayback`. The invoke command and the MCP tool use the same id,
  and a descriptor test in `auv-api-proto` checks the derivation.
- **`title`.** A short title, such as "Find text in a window".
- **`description`.** One plain-text paragraph, with no Markdown and no
  internal notes. It is written so that it can be used directly as a JSDoc
  comment or a Python docstring.

Proto comments stay developer notes, and may hold `TODO` and `NOTICE`. They
are not used as user-facing docs.

### Docs files

- **Location.** One Markdown file per method, `proto/<package path>/docs/<name>.md`,
  for example `proto/auv/api/driver/v1/docs/window.findText.md`.
- **Examples.** The fenced code blocks of the `## Examples` section, each
  titled by the `###` heading above it. The fence language (`ts`, `rust`,
  `sh`, …) is the example's language. Everything else in the file stays
  Markdown.
- **Build.** `auv-api-proto`'s build script embeds every `docs/*.md` as
  `auv_api_proto::METHOD_DOCS`.
- **Check.** A test fails when a file names no annotated method.

### `MethodDocsService`

- **Where it is implemented.** `auv-api-server::method_docs` builds the
  service from three inputs: a Runner's descriptor set, the names of the
  services it serves, and a docs table.
- **How a request is answered.** The service maps the requested gRPC path to
  the method's `presentation.name` and returns the parsed doc. A method
  without docs is `NOT_FOUND`.
- **Why the served services are named explicitly.** A descriptor set also
  describes the services of the files it depends on, so the descriptors alone
  would include services the Runner does not serve.
- **Who serves it.** The local Runner serves it, and reflection lists it.

## Consumers

- **SDK.** `DiscoveredRpcMethod` and `DescribedRpcMethod` carry
  `presentation`. `DescribedRpcMethod.docs()` fetches the long-form docs. It
  resolves `undefined` on `NOT_FOUND`, or on `UNIMPLEMENTED` from a Runner
  that serves no docs.
- **Playground.**
  - SDK calls are labelled by name, for example `window.list`.
  - The editor's hover card and the inspector's Call panel show the title and
    description of a call that has run.
  - "Docs" fetches and renders the Markdown with `react-markdown`, which
    drops raw HTML so a remote Runner cannot inject markup. Examples appear in
    one tab per language and can be copied.

## Coverage

- **`presentation`:** all 45 discoverable driver methods.
- **Docs:** three methods, written as the pattern for the rest:
  - `window.list`;
  - `window.findText`;
  - `input.clickPoint`.

## Deferred

- **`TODO(app-runner-method-docs)`.** The NetEase and Balatro Runners do not
  annotate their methods yet, and do not serve `MethodDocsService`.
- **Hover while writing code.** Showing docs before a call has run needs two
  things: the SDK's types in the editor (`TODO(playground-sdk-types)`), and
  JSDoc on the SDK's methods generated from `description`.
- **Python.** Generating docstrings for a Python client follows the same
  pattern as the JSDoc generation.
- **Docs for the remaining methods.** They are written per method, as the
  methods need them.
- **Docs in the npm package.** Not shipped. Add a `docs/` copy when offline or
  IDE use needs it.
