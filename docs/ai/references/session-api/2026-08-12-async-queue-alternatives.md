# JavaScript Async Queue Alternatives

Date: 2026-08-12

Status: research only; no production code changed

## Question

Can `js/packages/sdk/src/transport/async-queue.ts` be replaced by a built-in primitive or
an npm package while preserving all of its transport semantics?

The required contract is stricter than a generic task queue:

- producers push values into an `AsyncIterable`;
- normal `end()` drains buffered values and then completes;
- an ordinary RPC failure drains buffered values and then rejects;
- an abort discards buffered values and rejects immediately;
- the implementation works in browsers and Node.js;
- the client bundle remains small and tree-shakable.

## Verdict

There is no exact built-in primitive or drop-in npm package. The best npm
candidate is `@repeaterjs/repeater` 3.1.0. It can express both failure orders
only with a small adapter and a clearable buffer:

```ts
stop(rpcError) // drain buffered values, then reject
buffer.clear(); stop(abortError) // discard buffered values, then reject
```

Repeater can supply most of the machinery, but it is not a drop-in replacement.
Its public `throw(error)` method is a consumer-side iterator operation: it
rejects that `throw()` call and finishes the iterator, while a later `next()`
completes normally instead of observing the same error. AUV's abort arrives
from the producer side and must remain visible to the consumer, so an adapter
must clear a held buffer and then use producer-side `stop(error)`. Its executor
is also lazy, its producer API is backpressure-oriented, and its published core
is substantially larger than AUV's current 40-line state machine. The
recommended decision is therefore **keep the local queue for now**, unless
reducing owned concurrency code is worth a focused Repeater integration and
bundle-size spike.

## Built-ins

### WHATWG `ReadableStream`

`ReadableStreamDefaultController.close()` has the right graceful completion
behavior: consumers can read already-enqueued chunks before the stream closes.
However, `controller.error(error)` performs `ResetQueue` before erroring the
stream. It therefore implements AUV's immediate/discarding failure, not its
ordinary drain-then-fail behavior. Adding a queued error sentinel and an
adapter would recreate custom queue policy rather than remove it.

Source: [WHATWG Streams Standard, default controller and error
algorithm](https://streams.spec.whatwg.org/#rs-default-controller-class).

### Node built-ins

`events.on()` returns an async iterator, supports a close-event list and an
`AbortSignal`, and throws when the emitter emits `error`. It is nevertheless a
Node-only `EventEmitter` adapter and yields arrays of event arguments, so it
cannot back AUV's browser export. Node `Readable` has the same portability
problem and adds stream lifecycle/backpressure behavior that the transport
queue does not need.

Sources: [Node.js `events.on()`](https://nodejs.org/api/events.html#eventsonemitter-eventname-options)
and [Node.js readable async iterators](https://nodejs.org/api/stream.html#readableiteratoroptions).

## npm candidates

| Candidate | Graceful end | Drain then fail | Discard then fail | Browser + Node | Assessment |
|---|---:|---:|---:|---:|---|
| `@repeaterjs/repeater` 3.1.0 | Yes | Yes | With a clearable-buffer adapter | Likely; platform-neutral ESM/CJS core | Only serious replacement candidate; requires an adapter/spike |
| `async-channel` 0.2.0 | Yes | Yes, composed | Yes, composed | Platform-neutral source | Exact only through multi-call recipes; old `0.x` package |
| `event-iterator` 2.0.0 | Yes | Yes | No rejecting immediate close | Platform-neutral source | Covers ordinary failure only |
| `it-pushable` 3.2.4 | Yes | No | Yes | Platform-neutral source | `end(error)` deliberately clears the buffer |
| `ts-async-iterable-queue` 3.0.1 | Yes | Yes | Not reliably | Platform-neutral source | Immediate error does not clear its buffered push queue |
| `async-iterable-queue` 1.0.16 | Yes | No | No | No; imports `node:events` | Not suitable for browser export |
| `@borewit/async-queue` 0.1.2 | No | No | No | Platform-neutral source | A pending-value queue, not an `AsyncIterable` lifecycle primitive |
| `@n1ru4l/push-pull-async-iterable-iterator` 3.2.0 | Yes | No producer-facing API | Immediate iterator `throw` only | Platform-neutral source | Does not expose AUV's two producer termination modes |

### `@repeaterjs/repeater`: semantic match, API mismatch

Repeater's own error-handling guide explicitly says that `stop(error)` leaves
previously pushed values available and rejects the final `next()` only after
they are exhausted. The implementation of `stop` retains the push queue and
buffer; `finish` is the separate operation that clears them.

Sources:

- [Repeater error-handling guide](https://github.com/repeaterjs/repeater/blob/2b0e176487efad5b0d95d93066b0ec7680f8a0b3/docs/guides/05-error-handling.md#L10-L40)
- [`stop` retains queued values](https://github.com/repeaterjs/repeater/blob/2b0e176487efad5b0d95d93066b0ec7680f8a0b3/src/core.ts#L287-L317)
- [`finish` clears the buffer and pending pushes](https://github.com/repeaterjs/repeater/blob/2b0e176487efad5b0d95d93066b0ec7680f8a0b3/src/core.ts#L319-L345)
- [`throw(error)` takes the `finish` path when buffered](https://github.com/repeaterjs/repeater/blob/2b0e176487efad5b0d95d93066b0ec7680f8a0b3/src/core.ts#L567-L587)

That `throw(error)` path is not itself a producer-side abort primitive. Testing
the published 3.1.0 module confirms that the promise returned by `throw()`
rejects, but the next consumer `next()` resolves `{ done: true }`. To make an
abort reject the existing `responses` consumer, AUV would need to own a
clearable `RepeaterBuffer`, clear it, and then call `stop(abortError)`. Repeater's
built-in buffers do not expose `clear()` in their public interface.

The package is MIT licensed, has no runtime dependencies, exports a dedicated
`@repeaterjs/repeater/core` ESM/CJS subpath, and released version 3.1.0 in June
2026. Its core uses ECMAScript promises, arrays, weak maps, and symbols without
Node imports. Browser compatibility is an inference from that source shape,
not a documented browser test matrix; the repository CI currently tests Node
20/22/24 and Bun.

Sources:

- [3.1.0 release](https://github.com/repeaterjs/repeater/releases/tag/v3.1.0)
- [package exports and dependency metadata](https://github.com/repeaterjs/repeater/blob/2b0e176487efad5b0d95d93066b0ec7680f8a0b3/package.json)
- [CI runtime matrix](https://github.com/repeaterjs/repeater/blob/2b0e176487efad5b0d95d93066b0ec7680f8a0b3/.github/workflows/main.yml)

There are three integration costs:

1. The executor does not run until the first `next()`. AUV currently creates a
   queue, starts a request, and lets transport callbacks push independently of
   when user iteration begins. Capturing `push` and `stop` from a Repeater
   executor would make their availability depend on iteration start unless
   transport-listener registration moves into the executor or an adapter adds
   pre-start buffering.
2. Repeater's unbuffered mode caps pending push operations at 1024 and returns
   promises for backpressure. AUV's current queue has no such producer
   contract. A buffer choice is therefore a behavior decision, not just an
   import change. See [`MAX_QUEUE_LENGTH`](https://github.com/repeaterjs/repeater/blob/2b0e176487efad5b0d95d93066b0ec7680f8a0b3/src/core.ts#L175-L177),
   [`push`](https://github.com/repeaterjs/repeater/blob/2b0e176487efad5b0d95d93066b0ec7680f8a0b3/src/core.ts#L347-L415),
   and the [buffer APIs](https://github.com/repeaterjs/repeater/blob/2b0e176487efad5b0d95d93066b0ec7680f8a0b3/src/core.ts#L18-L111).
3. Immediate producer abort needs a custom clearable buffer plus `stop(error)`;
   calling the iterator's public `throw(error)` is not consumer-visible after
   that call settles.
4. The published `core.js` measured 8,764 bytes raw / 2,099 bytes gzip before
   application bundling. The current AUV TypeScript source is 1,577 bytes.
   Tree shaking may reduce Repeater, but it cannot make the executor adapter
   free. This measurement used the npm 3.1.0 tarball and `gzip -c`.

### Other near matches

`async-channel` can encode the two modes, but only as recipes. Its errors are
ordered channel items: ordinary failure is `throw(error)` followed by
`close()`, while abort requires `clear()`, `throw(error)`, then `close()`.
Its default buffer capacity is zero, sends return promises, and the current npm
release remains 0.2.0, making it more policy and lifecycle than AUV needs.

Source: [`async-channel` send, close, clear, and iterator
implementation](https://github.com/kyle1320/async-channel/blob/5732968f3638ae4b6f7a3d5117f48fc1f1b8bccd/src/index.ts#L46-L203).

`event-iterator` places `fail(error)` behind buffered values, which exactly
matches ordinary RPC failure. Its iterator `return()` clears the queue but
completes successfully; there is no producer operation that both clears and
rejects with the abort error.

Source: [`event-iterator` failure and iterator
implementation](https://github.com/rolftimmermans/event-iterator/blob/d7699d3d6e8bf3fa82c7cd42dc1a0a44e342b6d9/src/event-iterator.ts#L66-L136).

`it-pushable` explicitly documents and implements the opposite error ordering:
normal `end()` drains, while `end(error)` replaces the FIFO and makes the next
iteration throw. It remains a good immediate-abort primitive but cannot express
ordinary RPC failure without another ordered-error layer.

Sources: [`it-pushable` API contract](https://github.com/alanshaw/it-pushable/blob/0999892ccff90956054b8afd243a122b3170e564/src/index.ts#L45-L58)
and [`bufferError` implementation](https://github.com/alanshaw/it-pushable/blob/0999892ccff90956054b8afd243a122b3170e564/src/index.ts#L236-L269).

## GitHub Code Search findings

GitHub Code Search queries around `AsyncQueue implements AsyncIterable`,
`end(error)`, `close(error)`, `drain`, and `discard` found many local queue
implementations but no better maintained general package with AUV's exact two
failure modes. This is useful negative evidence: Signal Desktop and Z-Wave JS,
for example, each maintain their own small async queue rather than importing a
general queue package. Their queues do not satisfy AUV's full contract, so they
are evidence about ecosystem practice, not replacement candidates.

Sources:

- [Signal Desktop's local `AsyncQueue`](https://github.com/signalapp/Signal-Desktop/blob/de8fe1e7084fbab9c4e9c667c2d0ec0f208d1adc/ts/util/AsyncQueue.std.ts)
- [Z-Wave JS's local `AsyncQueue`](https://github.com/zwave-js/zwave-js/blob/b59ef1b58b17122368c828ce88bfb18404d466ee/packages/shared/src/AsyncQueue.ts)

One especially close local implementation has exactly AUV's
`close(error, { discard })` split: it clears buffered values only when
`discard` is true, otherwise `next()` consumes values before observing the
stored terminal error. It is internal application code, not an installable
general-purpose package, but independently confirms that AUV's state machine is
a coherent transport primitive rather than an accidental oddity.

Source: [Oxian's bounded async queue](https://github.com/AxionCompany/oxian-js/blob/18924c36997dffb22f882dc490287b1f5a769be4/src/transport/queue.ts#L1-L113).

## Recommendation

Do not replace `async-queue.ts` directly. Keep the local implementation and add
focused unit tests for its four terminal behaviors and iterator cleanup.

If dependency ownership is preferred, run one bounded Repeater spike before
deciding:

- adapt request/listener startup so no event can arrive before Repeater exposes
  `push` and `stop`;
- prove `stop(error)` and clear-buffer-then-`stop(abortError)` against the
  existing HTTP and gRPC streaming tests;
- run the actual browser project, not only Node tests;
- compare the built browser export and tree-shaking check with and without
  `@repeaterjs/repeater/core`.

Repeater is now the only candidate worth that experiment. The other packages
need at least as much custom policy as the existing queue while offering a less
direct API or weaker maintenance story.
