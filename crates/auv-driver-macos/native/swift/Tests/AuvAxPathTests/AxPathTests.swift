// Characterizes the pure AX observed-path parse layer (`AuvAxPath`).
//
// Scope: this locks the OBSERVED-PATH PARSE contract only (root marker,
// non-negative integer segments, offset reporting, verb interpolation). The live
// tree-walk cases (out-of-range, role mismatch, stale tree,
// first-window-vs-app-root, empty children) require a real AX tree and are
// characterized by documentation in
// `docs/ai/references/driver/2026-07-19-ax-path-resolution-characterization.md`,
// not here — headless CI cannot grant Accessibility permission.

import Testing

import AuvAxPath

/// A path expected to parse, plus the child indices it should yield.
///
/// `testDescription` carries the original characterization label so it shows up
/// as the case's display name in `swift test` output.
private struct SuccessCase: CustomTestStringConvertible {
  let label: String
  let path: String
  let expected: [Int]

  var testDescription: String { label }
}

/// A path expected to fail, plus the operation verb it is parsed under and the
/// substrings its failure message and recovery hint must carry.
private struct FailureCase: CustomTestStringConvertible {
  let label: String
  let path: String
  let operation: String
  let retry: String
  let messageContains: String
  let recoveryContains: String

  var testDescription: String { label }
}

private let successCases: [SuccessCase] = [
  SuccessCase(label: "root-only path yields no child indices", path: "0", expected: []),
  SuccessCase(label: "valid multi-segment path", path: "0.1.2", expected: [1, 2]),
  SuccessCase(label: "zero child indices are valid", path: "0.0.0", expected: [0, 0]),
  // Swift String.split omits empty subsequences. These surprising cases are
  // current behavior, not an endorsement; AxPath.swift carries the deferral.
  SuccessCase(label: "leading empty segment is currently omitted", path: ".0", expected: []),
  SuccessCase(label: "repeated empty segment is currently omitted", path: "0..1", expected: [1]),
  SuccessCase(label: "trailing empty segment is currently omitted", path: "0.", expected: []),
]

private let failureCases: [FailureCase] = [
  // --- root-marker cases ---
  FailureCase(
    label: "non-zero first segment rejected",
    path: "1.2",
    operation: "action",
    retry: "the action",
    messageContains: "path must begin with 0; got 1.2",
    recoveryContains: "capture a fresh AX tree and retry the action"
  ),
  FailureCase(
    label: "empty path rejected as missing root",
    path: "",
    operation: "action",
    retry: "the action",
    messageContains: "path must begin with 0; got ",
    recoveryContains: "capture a fresh AX tree"
  ),
  // --- non-integer / negative segment cases ---
  FailureCase(
    label: "non-integer segment rejected with offset",
    path: "0.x",
    operation: "focus",
    retry: "the focus request",
    messageContains: "path segment x at offset 0 is not a non-negative integer",
    recoveryContains: "retry the focus request"
  ),
  FailureCase(
    label: "negative segment rejected with offset 1",
    path: "0.1.-1",
    operation: "inspection",
    retry: "the inspection",
    messageContains: "path segment -1 at offset 1 is not a non-negative integer",
    recoveryContains: "retry the inspection"
  ),
  // --- verb interpolation is per-operation ---
  FailureCase(
    label: "operation verb interpolated into message",
    path: "9",
    operation: "inspection",
    retry: "the inspection",
    messageContains: "AX inspection path must begin with 0",
    recoveryContains: "retry the inspection"
  ),
]

@Test("parses accepted observed paths", arguments: successCases)
func parsesAcceptedObservedPath(testCase: SuccessCase) throws {
  switch axObservedPathIndices(path: testCase.path, operation: "action", retry: "the action") {
  case .success(let indices):
    #expect(indices == testCase.expected, "\(testCase.label): expected \(testCase.expected), got \(indices)")
  case .failure(let failure):
    Issue.record("\(testCase.label): expected success, got failure \(failure.message)")
  }
}

@Test("rejects invalid observed paths with the operation's verb", arguments: failureCases)
func rejectsInvalidObservedPath(testCase: FailureCase) throws {
  switch axObservedPathIndices(path: testCase.path, operation: testCase.operation, retry: testCase.retry) {
  case .success(let indices):
    Issue.record("\(testCase.label): expected failure, got success \(indices)")
  case .failure(let failure):
    #expect(
      failure.message.contains(testCase.messageContains),
      "\(testCase.label): message should contain \"\(testCase.messageContains)\", got \"\(failure.message)\""
    )
    #expect(
      failure.recovery.contains(testCase.recoveryContains),
      "\(testCase.label): recovery should contain \"\(testCase.recoveryContains)\", got \"\(failure.recovery)\""
    )
  }
}
