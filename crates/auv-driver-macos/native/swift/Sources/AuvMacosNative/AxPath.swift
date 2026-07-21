// Pure, Accessibility-free parse layer for observed AX paths.
//
// This file deliberately imports nothing and touches no `AXUIElement`, Rust
// bridge, or system API. Keeping the path-string parsing separate from the
// live tree walk (`axResolveObservedPath` in `AxTree.swift`) lets a standalone
// `swiftc` harness compile and characterize this layer in CI without linking
// the full `AuvMacosNative` module — which cannot link under `swift test`
// because the generated `SwiftBridgeCore.swift` references Rust FFI symbols
// that only exist in the cargo-built static library.
//
// Symbols are `internal` (not `private`) so both `axResolveObservedPath` (same
// module) and the characterization harness (which compiles this exact source
// file directly) can reach them. Behavior is unchanged from the original
// `private` definitions extracted from `AxTree.swift`.

struct AxPathResolutionFailure: Error {
  let message: String
  let recovery: String
}

/// Parses a dotted observed AX path (e.g. `0.1.2`) into child indices.
///
/// The observed-path contract: the first segment must be the window/app root
/// marker `0`, and every following segment must be a non-negative integer child
/// index. `operation` and `retry` are interpolated into the failure message and
/// recovery hint so the caller (action / focus / inspection) reports the right
/// verb. Returns the parsed indices *after* the leading `0`.
func axObservedPathIndices(path: String, operation: String, retry: String) -> Result<[Int], AxPathResolutionFailure> {
  // TODO(ax-path-empty-segments): Swift's split currently omits leading,
  // repeated, and trailing empty segments, so `.0`, `0..1`, and `0.` are
  // accepted. Rejecting them is a behavior change; keep the current behavior
  // characterized until the owner approves a path-validation bug-fix slice.
  let segments = path.split(separator: ".", omittingEmptySubsequences: true).map(String.init)
  guard segments.first == "0" else {
    return .failure(AxPathResolutionFailure(
      message: "AX \(operation) path must begin with 0; got \(path)",
      recovery: "capture a fresh AX tree and retry \(retry)"
    ))
  }

  var indices: [Int] = []
  for (offset, segment) in segments.dropFirst().enumerated() {
    guard let index = Int(segment), index >= 0 else {
      return .failure(AxPathResolutionFailure(
        message: "AX \(operation) path segment \(segment) at offset \(offset) is not a non-negative integer",
        recovery: "capture a fresh AX tree and retry \(retry)"
      ))
    }
    indices.append(index)
  }
  return .success(indices)
}
