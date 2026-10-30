# Codemode agent interface research

The [agent execution contract](2026-10-25-playground-http-mcp-design.md) owns current Playground behavior.

Source inspection date: 2026-10-20.
The AUV section was updated on 2026-10-26 at head revision `ad315ce9d2ddefc3541f08d31ad49baf1b279643`.

Evidence level: source inspection. This note makes no execution, performance, or agent task-success claims.
Playground integration evidence is in [the execution contract](2026-10-25-playground-http-mcp-design.md#evidence).

## Source snapshots

| Project | Source snapshot |
|---|---|
| Pi | [`73f8de80bc87021375f8b525292ac07492253002`](https://github.com/earendil-works/pi/tree/73f8de80bc87021375f8b525292ac07492253002), main, inspected 2026-10-20. The [codemode package](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/codemode/package.json#L1-L4) declares version 1.3.0. |
| OpenCode | [`6ec56821d63100fe5bda45b759364f561c56b359`](https://github.com/anomalyco/opencode/tree/6ec56821d63100fe5bda45b759364f561c56b359), dev, inspected 2026-10-20. This note covers its V2 adapter and codemode package. |
| AUV | [Playground source at ad315ce9](https://github.com/lulu0119/auv/tree/ad315ce9d2ddefc3541f08d31ad49baf1b279643/apps/repl-playground). This section describes the later HTTP MCP implementation. |

## Assessment

AUV resembles Pi in its execution model: an independent QuickJS/WASM context executes code inside a worker.
Its agent interface exposes a generated SDK catalog through `api.help`, `api.search`, and `api.describe`.

AUV also has a useful difference.
Scripts retain the SDK's resource methods, such as `const win = await auv.windows.resolve(...); await win.findText(...)`.
A generic JSON tool executor cannot replace this interface without an adapter.

## Pi source findings

Pi separates its standalone execution package from the coding-agent integration.
The Node/Bun adapter creates one worker and QuickJS VM per execution.
The package uses `quickjs-wasi` and has no Pi package dependency.
Sources: [worker host](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/codemode/src/runtime/host.ts#L12-L33),
[package dependencies](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/codemode/package.json#L67-L71).

The coding-agent adapter builds descriptions from input schemas, output schemas, descriptions, and tool guidelines.
It groups declarations by namespace and applies an inline budget.
Deferred tools stay out of that initial catalog.
Sources: [declaration projection](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/coding-agent/src/extensions/codemode/tool.ts#L141-L155),
[catalog construction](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/coding-agent/src/extensions/codemode/tool.ts#L224-L267).

Scripts find omitted tools through `ALL_TOOLS`, `searchTools`, `describeTool`, and `describeNamespace`.
The inspected `describeTool` implementation returns a declaration string, not a structured descriptor object.
Namespace help includes namespace instructions and tool names.
Source: [discovery globals](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/coding-agent/src/extensions/codemode/execute.ts#L587-L656).

Pi renders TypeScript signatures from tool schemas.
Those signatures explain call inputs and resolved outputs.
The standalone package explicitly does not validate values against those schemas.
The coding-agent adapter routes nested calls through `ctx.executeTool`, which retains the host tool execution path.
Sources: [signature generation](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/codemode/src/declarations.ts#L72-L101),
[schema limitations](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/codemode/src/types.ts#L11-L40),
[nested execution](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/coding-agent/src/extensions/codemode/execute.ts#L467-L499).

The coding-agent adapter selects script values from structured results or text.
It reports nested calls separately from the script output and saves store writes only after success.
Sources: [result projection](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/coding-agent/src/extensions/codemode/execute.ts#L422-L433),
[call records](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/coding-agent/src/extensions/codemode/execute.ts#L462-L499),
[store writes and output](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/coding-agent/src/extensions/codemode/execute.ts#L539-L570).

Pi can offer raw source input through a model-specific grammar.
AUV's standard MCP tool instead accepts a JSON object with a `code` string.
Raw source input therefore requires harness support and is not a portable ACP-client-only change.
Sources: [Pi tool registration](https://github.com/earendil-works/pi/blob/73f8de80bc87021375f8b525292ac07492253002/packages/coding-agent/src/extensions/codemode/tool.ts#L371-L390),
[AUV MCP schema](https://github.com/lulu0119/auv/blob/ad315ce9d2ddefc3541f08d31ad49baf1b279643/apps/repl-playground/src/features/agent/codemode-bridge.ts).

## OpenCode V2 source findings

Source: official `anomalyco/opencode`, `dev` resolved on 2026-10-20 to
[`6ec56821d63100fe5bda45b759364f561c56b359`](https://github.com/anomalyco/opencode/commit/6ec56821d63100fe5bda45b759364f561c56b359),
committer date `2026-10-20T02:38:16Z`. Both the generic package
[`@opencode/codemode`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/package.json#L1-L12)
and the actual V2 adapter's
[`@opencode/core`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/package.json#L1-L11)
declare version `2.0.32`. This is a source snapshot, not evidence that this exact
commit was published under that version.

### Actual agent surface

The model receives one `execute({ code })` tool plus tools configured for direct
exposure. Core snapshots registrations, filters whole-tool visibility, and puts
tools with `codemode !== false` into the code catalog. The executor captures those definitions.
[`tool.ts`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/tool.ts#L227-L285),
[`execute` input](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/codemode.ts#L55-L66).

MCP tools default into code mode and retain their input/output schemas. This is
not an all-tools wrapper: file reading, for example, explicitly stays direct.
OpenCode's session-management namespace explicitly enters code mode. A supplied
MCP `codemode: false` keeps that server's tools direct. There are also defaults
that avoid nesting known code-mode MCP servers inside OpenCode's code mode.
[`MCP registration`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/tool/mcp.ts#L40-L49),
[`read`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/tool/plugin/read.ts#L38-L46),
[`session_rename`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/tool/plugin/opencode.ts#L105-L116),
[`MCP defaults`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/plugin/mcp-codemode-defaults.ts#L6-L37).

### Discovery and types

Core supplies a catalog with a default estimated budget of 2,000 tokens, using
four characters per token. It retains every namespace, includes pinned entries,
then selects complete signatures round-robin across namespaces, considering
shorter entries first. Namespace overhead and pinned entries can exceed that
budget. When partial, the instructions advertise synchronous `search(...)` and
require exact returned paths. The function remains present when the catalog is
complete. Catalog changes have replacement/delta instructions.
[`catalog selection`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/codemode/catalog.ts#L48-L139),
[`search instructions`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/codemode/instructions.ts#L8-L39),
[`global binding`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/interpreter/globals.ts#L86-L90).

The search contract, condensed without generated formatting and JSDoc, is:

```ts
search({ query?: string, namespace?: string, limit?: number, offset?: number }): {
  items: Array<{ path: string, description: string, signature: string }>,
  remaining: number,
  next: { offset: number } | null
}
```

`limit` is a positive integer. `offset` is a nonnegative integer. Defaults are
10 results and offset 0. Empty queries browse the catalog. Exact paths take precedence.
Other queries rank names, descriptions, and namespace descriptions. Results
include callable `tools...` paths, including bracket notation for paths that require it.
For example, inside `execute`, `return search({ namespace: "opencode" })`
browses that namespace. The next program can call a returned `path`. Search is
synchronous and counts as an admitted tool call.
[`schema and path renderer`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/tool-runtime.ts#L89-L113),
[`matching and pagination`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/tool-runtime.ts#L214-L299),
[`call accounting`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/tool-runtime.ts#L392-L447).

Signatures include both argument and `Promise<output>` types. JSON Schema
rendering preserves property documentation and constraints as JSDoc. Unresolved
references or depth limits can produce `unknown`. These declarations guide the
model. They do not type-check its program. Generic Effect schemas decode values.
Generic raw JSON schemas only render declarations. Core separately validates
inputs when it can construct a codec, and its raw JSON-schema output boundary
checks JSON data rather than the declared shape.
[`signature generation`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/tool-runtime.ts#L169-L183),
[`schema rendering`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/tool-schema.ts#L71-L152),
[`generic decoding`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/tool-schema.ts#L276-L303),
[`Core validation`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/tool/runtime.ts#L63-L132).

### Execution, output, and authority

This is an owned JavaScript interpreter, not QuickJS. Acorn parses JavaScript
directly with top-level `await` and `return`. TypeScript-only syntax is rejected.
TypeScript signatures do not imply TypeScript execution. Classes are unsupported.
Promise `.then`, `.catch`, and `.finally` are implemented.
[`parser and interpreter`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/interpreter/execute.ts#L27-L54),
[`parser configuration`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/interpreter/execute.ts#L109-L117),
[`supported-language matrix`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/interpreter-support.md#L15-L20),
[`class boundary`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/interpreter-support.md#L153-L162),
[`promise methods`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/interpreter/promises.ts#L379-L383).

The generic runtime exposes only explicit tools and extensions. The actual V2
adapter injects a global `fetch`, advertises it, rejects `file:` URLs, and uses a
30-second HTTP timeout. It invokes host fetch directly. This adapter contains no
MCP-style permission assertion. Response bytes are buffered, with `text`,
`json`, and `bytes` methods exposed. Do not infer absent network access from the
generic runtime alone.
[`generic options`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/codemode.ts#L43-L53),
[`adapter instructions`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/codemode/tool.ts#L70-L78),
[`extension installation`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/codemode/tool.ts#L237-L255),
[`HTTP boundary`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/codemode/web.ts#L5-L42).

Nested tools use the normal Core execution hooks and leaf permissions. MCP checks
permission before calling its server. Child calls reuse the outer invocation
context. Structured output enters the program. Absent structured output falls
back to text or null. Files stay host-side and are attached to the outer result,
even if the program does not return them. MCP text resembling JSON is parsed
when its server declares no output schema.
[`hooks`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/tool.ts#L103-L155),
[`nested dispatch`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/tool.ts#L245-L249),
[`permissions and projection`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/tool/mcp.ts#L49-L110),
[`files and structured output`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/codemode/tool.ts#L89-L149).

Generic execution returns structured success or diagnostic data, including
warnings, logs, and admitted calls. The adapter formats the value/error as text,
adds call-status progress/metadata, and attaches files. It does not preserve the
full diagnostic object in its public output schema. Core bounds the outer tool
result after execution. Nested intermediate values remain available for
in-program filtering.
[`generic result`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/codemode.ts#L68-L118),
[`adapter result schema`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/codemode/tool.ts#L24-L37),
[`formatting`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/codemode/tool.ts#L333-L350),
[`outer retention boundary`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/session/runner/step.ts#L123-L133).

The library supports optional execution time, call-count, and output limits,
with no defaults. This adapter supplies none. Independent calls can run
concurrently. Pending interpreter work is interrupted when execution ends, and
host cancellation interrupts supervised tool fibers. Fetch's source supplies
only its own timeout signal, so interpreter interruption alone does not prove
that an already-started HTTP request is aborted. Prepared catalogs/runtimes are
cached per inventory. Per-program execution state is fresh.
[`limits`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/codemode.ts#L21-L40),
[`adapter construction`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/codemode/tool.ts#L233-L255),
[`execution lifecycle`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/interpreter/execute.ts#L27-L105),
[`cancellation`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/core/src/session/runner/step.ts#L140-L152),
[`extension promise wrapping`](https://github.com/anomalyco/opencode/blob/6ec56821d63100fe5bda45b759364f561c56b359/packages/codemode/src/interpreter/extensions.ts#L139-L159).

## AUV comparison

AUV exposes SDK resource methods rather than a generic JSON tool catalog.
For example, `WindowClient` retains methods after it crosses the VM boundary.
The generated help describes ergonomic SDK calls, including positional arguments and returned resource objects.
Runner discovery instead describes lower-level RPC contracts.

| Boundary | AUV choice |
| --- | --- |
| Agent interface | One `execute({ code, timeoutMs? })` MCP tool through Exterior HTTP MCP. |
| Language | Erasable TypeScript syntax, compiled to instrumented JavaScript without per-script type checking. |
| Execution | A temporary worker and fresh QuickJS context; the editor retains its own worker. |
| Discovery | `api.help`, `api.search`, and `api.describe`, generated from SDK declarations. |
| Results | Explicit text/image content and `isError`; detailed SDK Calls stay in the timeline. |

The [execution contract](2026-10-25-playground-http-mcp-design.md) owns current limits, state rules, connection lifetime, and integration evidence.
This comparison provides source evidence without performance or agent task benchmarks.

Sources at the inspected AUV revision:
[MCP host](https://github.com/lulu0119/auv/blob/ad315ce9d2ddefc3541f08d31ad49baf1b279643/apps/repl-playground/src/features/agent/codemode-bridge.ts),
[VM](https://github.com/lulu0119/auv/blob/ad315ce9d2ddefc3541f08d31ad49baf1b279643/apps/repl-playground/src/workers/codemode.ts),
[help catalog](https://github.com/lulu0119/auv/blob/ad315ce9d2ddefc3541f08d31ad49baf1b279643/apps/repl-playground/agent/help-catalog.ts),
[SDK resource methods](https://github.com/lulu0119/auv/blob/ad315ce9d2ddefc3541f08d31ad49baf1b279643/js/packages/sdk/src/apis/auv/driver.ts),
[Runner discovery](https://github.com/lulu0119/auv/blob/ad315ce9d2ddefc3541f08d31ad49baf1b279643/js/packages/sdk/src/apis/auv/discover.ts).
