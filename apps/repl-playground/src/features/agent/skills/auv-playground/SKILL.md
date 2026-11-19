---
name: auv-playground
description: Use AUV Playground's execute tool to inspect windows, read text, operate the selected desktop, and verify results with the native AUV Runner SDK.
---

# AUV Playground

Send JavaScript to `execute`. The sandbox supplies `auv`, the Runner client for this execution, and `sdk`, which includes the Runner enums and geometry helpers. The Playground owns the device connection. Read [the SDK guide](references/sdk.md) for operations and [the Runner types and implementation](references/driver.ts) for precise signatures.

Resolve a window, inspect its contents, act on a match, then read the resulting state. A successful click reports delivery. An application-specific read shows whether it had the intended effect. Actions already delivered remain in effect if an execution fails or is canceled.

```js
const counter = await auv.windows.resolve({ appName: 'Counter' })
const { matches } = await counter.findText('Increment')
if (matches.length !== 1) throw new Error('Expected one Increment button')
await counter.click(matches[0])
return (await counter.findText('Count:')).matches.map(match => match.text)
```

For a visual result, fetch pixels explicitly with a bounded size.

```js
const win = await auv.windows.resolve({ frontmost: true })
const { capture } = await win.capture()
const png = await auv.captures.image(capture, {
  encoding: sdk.ImageEncoding.PNG,
  maxSize: { width: 800, height: 600 },
})
image({ type: 'image', mimeType: 'image/png', data: png.data.toBase64() })
```

Keep window clients and callbacks inside the current execution. Within the same conversation and Device, store a window ID with `store('window', win.id)` and bind it to the next Run with `auv.windows.from(load('window'))`. Await each device action and each `execute` call before starting the next.
