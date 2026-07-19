// Characterization harness for the pure AX path parse layer.
//
// This file is compiled by `scripts/ci/ax-path-characterization.sh` together
// with the REAL `Sources/AuvMacosNative/AxPath.swift` in a single standalone
// `swiftc` invocation. It links nothing from the rest of `AuvMacosNative`, so
// it runs on headless CI (no Accessibility grant, no Rust static lib).
//
// It is intentionally NOT a SwiftPM target and NOT under `Sources/`, so:
//   - `build.rs` (which globs only `Sources/AuvMacosNative/*.swift`) never
//     compiles it into the shipped static library.
//   - SwiftPM's `AuvMacosNative` target never sees it.
//
// Scope: this locks the OBSERVED-PATH PARSE contract only (root marker,
// non-negative integer segments, offset reporting, verb interpolation). The
// live tree-walk cases (out-of-range, role mismatch, stale tree,
// first-window-vs-app-root, empty children) require a real AX tree and are
// characterized by documentation in
// `docs/ai/references/driver/2026-07-19-ax-path-resolution-characterization.md`,
// not here — headless CI cannot grant Accessibility permission.

import Foundation

@main
struct AxPathCharacterization {
  static var failures = 0

  static func check(_ condition: Bool, _ label: String) {
    if condition {
      print("ok   - \(label)")
    } else {
      print("FAIL - \(label)")
      failures += 1
    }
  }

  static func expectSuccess(_ path: String, _ expected: [Int], _ label: String) {
    switch axObservedPathIndices(path: path, operation: "action", retry: "the action") {
    case .success(let indices):
      check(indices == expected, "\(label): indices == \(expected) (got \(indices))")
    case .failure(let failure):
      check(false, "\(label): expected success, got failure \(failure.message)")
    }
  }

  static func expectFailure(
    _ path: String,
    operation: String,
    retry: String,
    messageContains: String,
    recoveryContains: String,
    _ label: String
  ) {
    switch axObservedPathIndices(path: path, operation: operation, retry: retry) {
    case .success(let indices):
      check(false, "\(label): expected failure, got success \(indices)")
    case .failure(let failure):
      check(failure.message.contains(messageContains), "\(label): message contains \"\(messageContains)\" (got \"\(failure.message)\")")
      check(failure.recovery.contains(recoveryContains), "\(label): recovery contains \"\(recoveryContains)\" (got \"\(failure.recovery)\")")
    }
  }

  static func main() {
    // --- success cases ---
    expectSuccess("0", [], "root-only path yields no child indices")
    expectSuccess("0.1.2", [1, 2], "valid multi-segment path")
    expectSuccess("0.0.0", [0, 0], "zero child indices are valid")

    // --- root-marker case ---
    expectFailure(
      "1.2", operation: "action", retry: "the action",
      messageContains: "path must begin with 0; got 1.2",
      recoveryContains: "capture a fresh AX tree and retry the action",
      "non-zero first segment rejected"
    )
    expectFailure(
      "", operation: "action", retry: "the action",
      messageContains: "path must begin with 0; got ",
      recoveryContains: "capture a fresh AX tree",
      "empty path rejected as missing root"
    )

    // --- non-integer / negative segment cases ---
    expectFailure(
      "0.x", operation: "focus", retry: "the focus request",
      messageContains: "path segment x at offset 0 is not a non-negative integer",
      recoveryContains: "retry the focus request",
      "non-integer segment rejected with offset"
    )
    expectFailure(
      "0.1.-1", operation: "inspection", retry: "the inspection",
      messageContains: "path segment -1 at offset 1 is not a non-negative integer",
      recoveryContains: "retry the inspection",
      "negative segment rejected with offset 1"
    )

    // --- verb interpolation is per-operation ---
    expectFailure(
      "9", operation: "inspection", retry: "the inspection",
      messageContains: "AX inspection path must begin with 0",
      recoveryContains: "retry the inspection",
      "operation verb interpolated into message"
    )

    if failures == 0 {
      print("AX path characterization: ALL PASS")
      exit(0)
    } else {
      print("AX path characterization: \(failures) FAILED")
      exit(1)
    }
  }
}
