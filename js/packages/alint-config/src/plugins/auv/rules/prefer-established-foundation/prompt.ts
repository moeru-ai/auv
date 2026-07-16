export const establishedFoundationInstructions = `
You are reviewing one Rust source file.

Use the report_finding tool for each warning. Do not report anything when the file has no qualifying issue.
`.trim();

export const establishedFoundationPrompt = `
Task:
Warn about hand-rolled foundation code.

Foundation code means common infrastructure behavior that mature libraries, standard libraries, or a shared project utility would normally provide. Examples include structured output rendering, tabular or delimited text emission, stdout/stderr emission policy, pretty serialization, text wrapping or truncation, path and time handling, collection reshaping, escaping, sorting/grouping helpers, ad-hoc parsing, simple formatting protocols, retry/backoff loops, and other broadly reusable mechanics.

This rule is not limited to those examples. Look for any local helper or cluster of helpers that rebuilds a common capability inside a higher-level file instead of using an established dependency, the standard library, or a shared utility boundary.

This is a warning-level design smell, not a correctness error.

Report the declaration line of the local helper or orchestration function that most clearly owns the hand-rolled foundation behavior. Report multiple functions only when each one contributes a distinct piece of the private toolkit.

Do not key on names, string literals, specific crates, comments, or exact syntax. Infer the smell from responsibility:
- generic mechanics mixed into a higher-level workflow file
- local loops, builders, conditionals, or string assembly implementing reusable infrastructure
- helper clusters that would likely be needed by other files
- private formatting or transformation policy that duplicates an existing library-shaped capability
- output or serialization behavior that should pass through a shared renderer or serializer boundary

Treat private table/output toolkits as strong signals. Report local helpers that define columns, column width or truncation policy, cell rows, delimited output, pretty JSON, CSV-like output, stdout formatting, or generic text formatting when they live beside higher-level workflow logic instead of behind a shared rendering or utility boundary.

For private toolkit clusters, report the small helper declarations that encode the reusable mechanics, not only the top-level function that calls them. For example, report separate helpers for columns, row construction, display width policy, and generic geometry or primitive formatting when they are part of the local foundation layer.

Do not report:
- thin calls to a standard library or established dependency
- cohesive domain transformations whose rules are specific to the domain model
- adapters that prepare typed data for an already shared renderer, serializer, parser, or utility
- small one-off expressions where introducing a dependency or utility would add more complexity
- performance-sensitive or compatibility-sensitive code with a visible reason for being custom
- tests, fixtures, examples, or benchmarks

When suggesting a fix, name the kind of boundary to use, such as a standard library facility, dependency, shared formatter, shared serializer, shared parser, or reusable utility module. If uncertain, return no finding.
`.trim();
