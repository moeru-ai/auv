export const noModNamesChecksInTestsInstructions = `
You are reviewing Rust tests in the AUV project.

Call report_findings exactly once. Report only tests that inspect Rust source text to assert the presence or absence of module, export, or type names. Use an empty findings array when there is no violation.
`.trim()

export const noModNamesChecksInTestsPrompt = `
Rust tests must not enforce implementation structure by searching source text for Rust module, export, function, constant, trait, or type names.

Report test code that reads or includes Rust source and then:

- searches for module declarations or exports such as mod, pub mod, pub use, or re-export lists
- asserts that a struct, enum, trait, type alias, function, constant, field, variant, method, or other symbol name is present or absent
- splits source text around a declaration name to inspect fields or implementation sections
- keeps a forbidden or required list of Rust identifiers and checks the source text against it
- claims an architectural contract from string matching against implementation declarations

Representative bad shapes include reading a crate root and asserting that a placeholder module declaration is absent, then checking a list of future contract type names; or including a module as text and searching for forbidden exported helpers. These examples are patterns only and must not depend on any concrete application, module, or identifier name.

Do not report:

- ordinary assertions over typed values, enum variants, serialized wire values, error codes, command names, or other runtime outputs
- compiler-enforced imports or calls through a public Rust API
- tests that parse domain text where a word happens to match a Rust identifier
- documentation examples that do not inspect source text
- checks of package metadata or feature membership that do not search Rust implementation source

Every finding suggestion must explain that Rust's compiler and public behavioral tests are the meaningful authorities for modules and types. Source-string checks of declaration or export names do not prove behavior, break on harmless refactors, and should be removed entirely. If a real public contract exists, recommend a typed test that imports and exercises that API; do not recommend another source-text or file-layout check.

Report the test function declaration line once per independent offending test. Return no finding when identifiers are asserted as runtime data rather than searched in Rust source text.
`.trim()
