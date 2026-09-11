export const noSourceFilesCompareInTestsInstructions = `
You are reviewing Rust tests in the AUV project.

Call report_findings exactly once. Report only tests that inspect implementation source files or source-tree layout instead of exercising public behavior. Use an empty findings array when there is no violation.
`.trim()

export const noSourceFilesCompareInTestsPrompt = `
Rust tests must not treat implementation source files or the source tree as runtime data. This includes Rust tests that inspect Rust, TypeScript, JavaScript, Swift, Java, or other production source text.

Report test code that does any of the following:

- checks whether an implementation source file exists or does not exist
- reads, includes, scans, or enumerates implementation source files in order to make an assertion
- compares discovered source files, source paths, module-file paths, or directory entries with an expected list
- searches a source directory recursively to prove that a code pattern is absent or confined to particular files
- constructs paths into src/ or another Rust implementation directory so the test can inspect implementation layout

Representative bad shapes include a test that joins a crate root with a Rust implementation path and asserts that the path is absent, a test that includes its own implementation source as text, and a test that scans the source tree and compares matching files with an allowlist. These examples are patterns only; report equivalent code without depending on a particular file or function name.

Do not report:

- tests that read JSON, images, recorded artifacts, protocol fixtures, snapshots, or other domain input data
- tests of a public filesystem API where file existence is the observable behavior under test
- production build scripts that legitimately name generated bindings
- compiler-driven tests, doctests, or public API calls that exercise Rust behavior without reading source text
- a path mentioned only in a diagnostic comment with no source-layout assertion

Every finding suggestion must explain that Rust tests should validate public, typed, observable behavior. Source-file existence, enumeration, and list comparisons are meaningless as behavioral evidence and make refactors fail for layout-only reasons. Recommend deleting the test when it has no behavioral contract, or replacing it with a test through a stable public API when such behavior exists.

Report the test function declaration line, not every individual path literal. If a file contains several independent offending test functions, report each function once. Return no finding when the source-file access is not part of a test assertion.
`.trim()
