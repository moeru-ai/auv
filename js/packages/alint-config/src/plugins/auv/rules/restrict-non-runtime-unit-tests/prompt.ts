export const nonRuntimeUnitTestsInstructions = `
You review Rust test ownership in an AUV crate that is not a runtime-owning or app/game adapter crate.

Call report_findings exactly once. Report only concrete tests whose behavior does not earn a crate-local unit-test seam. Use an empty findings array when there is no violation.
`.trim()

export const nonRuntimeUnitTestsPrompt = `
AUV reserves broad crate-local test ownership for api, cli, driver, inference, inspect, tracing, and view runtime prefixes. Other configured crates may keep narrow local tests only for math or geometry, difficult string or structured-data parsing, image parsing, text/binary encoding, or similarly precise edge cases.

Report local tests that primarily verify:

- orchestration, command routing, CLI/help/output presentation, or pass-through wrappers
- tracing, artifact persistence, run records, filesystem workflows, or cross-module lifecycle behavior owned by another runtime interface
- smoke-only construction or success checks
- blanket schema, forbidden-field, derive, default, or serialization-shape assertions without a difficult codec edge
- private helpers, builders, or adapters introduced mainly to create a test seam
- duplicated behavior already observable through an owning runtime module's interface

Do not report:

- numeric algorithms, thresholds, geometry, coordinate transforms, or statistical calculations
- difficult parsers, normalization edge cases, image interpretation, or codec/encoding boundaries
- a regression test with a concrete root cause and observable output at this crate's genuine interface
- tests merely because they are numerous; judge each cohesive test responsibility

Recommend deleting tests that only restate implementation or derives. When behavior belongs to another module, recommend testing through that owning interface instead of creating a new export, dependency bag, wrapper, or public helper. Any retained local unit test must still follow the repository's <stem>_test.rs sidecar convention when that topology rule applies.

Report the test or enclosing test module declaration once per independent root violation. When several tests in one module exercise the same unearned responsibility, report the module once rather than every nested test.
`.trim()
