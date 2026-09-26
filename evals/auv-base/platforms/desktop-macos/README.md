# macOS desktop evaluations

- `tasks/keyboard.rs`: CLI and persistent-driver keyboard delivery.
- `tasks/focus_without_raise.rs`: activation, key-window focus, and restoration.
- `tasks/native/`: Swift controllers for macOS focus and input-source state.
- `cases/`: input scenarios and receiver checks in Rust.
- `test-objects/appkit/`: native Swift receivers.

Electron and web receivers are shared from `../desktop-universal/test-objects/`.

`test-objects/appkit/` identifies the native application technology. Its current
`keyboard.swift` and `focus.swift` receivers cover keyboard input and focus;
future approved evaluations can add other test areas under the same directory.
See [test object scope and naming](../../README.md#test-object-scope-and-naming).

See the [suite README](../../README.md) for prerequisites and `just eval` commands.
