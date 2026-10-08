# List windows

Lists the windows the Runner can see right now, front to back. Each window
carries its reference (`ref.windowId`), application name and bundle ID,
process ID, title and frame.

## Coordinates

`frame` is in logical screen coordinates: points, not pixels, with the
origin at the top-left of the primary display. Displays left of or above the
primary display have negative coordinates.

## What counts as a window

Menu bar items, status indicators and other system surfaces are windows too.
Select the one you want by application and title instead of taking the first
entry.

## Window references

A window reference belongs to the Device, not to a Run. It stays valid while
the window exists; an operation on a closed window fails with `NOT_FOUND`.
Every operation refreshes the window before it acts, so a stale frame in the
listing is never used for input.

## Examples

### Find an application's window

```ts
const windows = await device.windows.list()
const music = windows.find(w => w.window.applicationBundleId === 'com.netease.163music')
```

```rust
let windows = runner.windows().list().await?;
let music = windows.into_iter().find(|w| w.resource().app_bundle_id.as_deref() == Some("com.netease.163music"));
```

### From the command line

```sh
auv invoke window.list --wide
```
