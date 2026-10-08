# Method presentation and docs

Date: 2026-10-09

Status: **accepted** (owner, 2026-10-09). The first slice is implemented. Names
marked *provisional* are open.

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

- **`name`.** Dotted segments in lower_snake_case that follow the SDK's API
  path, for example `window.find_text` and `macos.media.toggle_play_pause`.
  This is the only stored casing. Clients derive the others: JavaScript uses
  `camelCaseName` from `@auv-js/sdk` and gets `window.findText`.
- **`title`.** A short title, such as "Find text in a window".
- **`description`.** One plain-text paragraph, with no Markdown and no
  internal notes. It is written so that it can be used directly as a JSDoc
  comment or a Python docstring.

Proto comments stay developer notes, and may hold `TODO` and `NOTICE`. They
are not used as user-facing docs.

### Docs files

- **Location.** One Markdown file per method, `proto/<package path>/docs/<name>.md`,
  for example `proto/auv/api/driver/v1/docs/window.find_text.md`.
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
  - SDK calls are labelled by name, for example `windows.list`.
  - The editor's hover card and the inspector's Call panel show the title and
    description of a call that has run.
  - "Docs" fetches and renders the Markdown with `react-markdown`, which
    drops raw HTML so a remote Runner cannot inject markup. Examples appear in
    one tab per language and can be copied.

## Coverage

- **`presentation`:** all 44 discoverable driver methods.
- **Docs:** three methods, written as the pattern for the rest:
  - `windows.list`;
  - `window.find_text`;
  - `input.click`.

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
