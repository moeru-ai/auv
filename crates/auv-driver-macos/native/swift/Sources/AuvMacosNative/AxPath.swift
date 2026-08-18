// Pure, Accessibility-free parse layer for observed AX paths, covered by
// `swift test` through the `AuvAxPath` SwiftPM target (`Tests/AuvAxPathTests`).
//
// This file deliberately imports nothing and touches no `AXUIElement`, Rust
// bridge, or system API. Keeping the path-string parsing separate from the live
// tree walk (`axResolveObservedPath` in `AxTree.swift`) is what makes it
// testable on headless CI: it needs no Accessibility (TCC) grant and links no
// cargo-built Rust static library, unlike the rest of `AuvMacosNative`.
//
// NOTICE: `Package.swift` keeps `AuvAxPath` and `AuvMacosNative` pointed at this
// same directory, because `build.rs` globs `Sources/AuvMacosNative/*.swift` into
// one flat `swiftc -module-name AuvMacosNative` call. Symbols are `public` so
// the test target can import them across the SwiftPM module boundary; `public`
// is inert in that flat single-module compile, where `axResolveObservedPath`
// reaches them as same-module symbols. Behavior is unchanged from the original
// `private` definitions extracted from `AxTree.swift`.

public struct AxPathResolutionFailure: Error {
  public let message: String
  public let recovery: String

  // NOTICE: A `public` struct's memberwise initializer is `internal` by default,
  // so `axResolveObservedPath` in `AxTree.swift` — a different SwiftPM module —
  // needs this spelled out. Same members, same order as the synthesized one.
  public init(message: String, recovery: String) {
    self.message = message
    self.recovery = recovery
  }
}

/// Parses a dotted observed AX path (e.g. `0.1.2`) into child indices.
///
/// The observed-path contract: the first segment must be the window/app root
/// marker `0`, and every following segment must be a non-negative integer child
/// index. `operation` and `retry` are interpolated into the failure message and
/// recovery hint so the caller (action / focus / inspection) reports the right
/// verb. Returns the parsed indices *after* the leading `0`.
public func axObservedPathIndices(path: String, operation: String, retry: String) -> Result<[Int], AxPathResolutionFailure> {
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
