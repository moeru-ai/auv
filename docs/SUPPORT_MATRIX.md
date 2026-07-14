# AUV Support Matrix

This matrix describes the highest evidence level currently justified for public
AUV claims. It is not a roadmap. An empty cell means AUV does not make a public
claim for that platform and capability.

Update this document in the same change that alters a public capability or its
evidence. The matrix is a current claim, not a historical release note.

## Evidence Levels

| Level | Meaning |
| --- | --- |
| `contract` | A typed public contract or documented interface exists. It makes no compilation, behavior, or platform claim. |
| `compiles` | A current CI gate or reproducible command compiles the relevant crate or target. Compilation does not prove runtime behavior. |
| `tested` | Automated tests exercise the behavior with fixtures, fakes, hermetic transports, or deterministic local resources. Such tests are not desktop or app validation unless they drive the named environment. |
| `live-validated` | A dated evidence record proves the stated boundary on a named real OS, app, or device, including known limits. One successful probe is not automatically product support. |
| `supported` | The public setup path is documented, an automated regression gate exists, current live closure exists for that public path, known limits are documented, and the project explicitly maintains that surface. |

The levels are ordered by evidence strength. A matrix cell reports the highest
proven level, not a target. `not claimed` means no public level is asserted.

## Product Surface

| Capability | Public surface | macOS | Windows | Linux | Evidence | Known limit |
| --- | --- | --- | --- | --- | --- | --- |
| Product CLI installation and command discovery | `auv`, `auv invoke --help` | `tested` | `not claimed` | `tested` | [install smoke](../scripts/ci/install-smoke.sh), [CI matrix](../.github/workflows/check.yml), [CLI parser tests](../crates/auv-cli/src/cli.rs) | Git-source installation is tested against a local, commit-pinned `file://` repository snapshot; it does not exercise the public top-level GitHub URL. The check job currently runs on macOS and Ubuntu, not Windows. |
| MCP invoke and run inspection | `auv mcp serve` | `tested` | `not claimed` | `tested` | [MCP tools and transport tests](../src/mcp.rs), [product MCP bootstrap](../crates/auv-cli/src/mcp.rs) | Tests use a hermetic stdio transport and deterministic commands; no remote-session claim is made. |
| Durable runs, artifacts, and inspect composition | `auv invoke`, `auv inspect`, `auv inspect serve` | `tested` | `not claimed` | `tested` | [recorded invoke tests](../crates/auv-cli-invoke/src/recorded.rs), [inspect composition tests](../crates/auv-cli/src/inspect/goldens.rs), [terms](TERMS_AND_CONCEPTS.md#artifact) | The durable record is local-first; remote reporting and replay are separate, unfinished surfaces. |
| Screenshot and capture primitives | registry commands such as `display.capture` and `window.capture` | `tested` | `not claimed` | `not claimed` | [macOS capture tests](../crates/auv-driver-macos/src/session.rs), [capture contract](TERMS_AND_CONCEPTS.md#capture-contract) | Other platform crates exist, but current public CI does not run Windows and no current Linux desktop closure is recorded. |
| OCR and recognition primitives | registry commands such as `screen.findText` | `tested` | `not claimed` | `not claimed` | [macOS OCR decoder tests](../crates/auv-driver-macos/src/native/ocr.rs), [core registry test](../crates/auv-cli-invoke/src/lib.rs) | OCR engine availability and output quality remain target-environment dependent. |
| Accessibility-tree observation | `window.captureAxTree` | `tested` | `not claimed` | `not claimed` | [macOS accessibility tests](../crates/auv-driver-macos/src/accessibility.rs), [core registry test](../crates/auv-cli-invoke/src/lib.rs) | No current cross-platform desktop/app closure is claimed. |
| Accessibility focus and actions | commands such as `input.focusText` and `input.pressButton` | `tested` | `not claimed` | `not claimed` | [macOS accessibility tests](../crates/auv-driver-macos/src/accessibility.rs), [core registry test](../crates/auv-cli-invoke/src/lib.rs) | A delivered action is not semantic success without a separate verification result. |
| Keyboard and pointer delivery | registry `input.*` commands | `tested` | `not claimed` | `not claimed` | [macOS input-policy tests](../crates/auv-driver-macos/src/session.rs), [core registry test](../crates/auv-cli-invoke/src/lib.rs) | Current evidence is policy/fixture-oriented, not a blanket app-level live claim. |
| Background or virtual input | driver input-delivery policy | `tested` | `not claimed` | `not claimed` | [macOS delivery-policy tests](../crates/auv-driver-macos/src/session.rs), [input-mode contract](TERMS_AND_CONCEPTS.md#input-mode), [scroll-delivery contract](TERMS_AND_CONCEPTS.md#scroll-delivery-strategy) | Do not infer background delivery on Windows or Linux; those drivers retain foreground-only or explicitly rejected paths. |
| Scroll scan | `scan.frame`, `scan.coverage`, persisted scan artifacts | `tested` | `not claimed` | `not claimed` | [scan command tests](../crates/auv-cli-invoke/src/commands/scan.rs), [scroll-scan terms](TERMS_AND_CONCEPTS.md#scroll-scan) | The public matrix does not claim a real app-list closure on any platform. |
| App-local recorded operation: TextEdit document write | `auv invoke app.textedit.document.write` | `tested` | `not claimed` | `not claimed` | [fixture parity and mismatch tests](../crates/auv-cli/tests/textedit_document_write_parity.rs), [product integration](../crates/auv-cli/src/integrations/textedit/mod.rs), [live closure record](ai/references/apps/textedit/2026-07-13-textedit-document-write-live-closure.md) | Fixture-backed tests establish recorded CLI/MCP/inspect parity. The live macOS closure is blocked because the WindowServer `.optionOnScreenOnly` snapshot does not expose the AX-visible TextEdit window; no successful focus, paste, or post-write verification occurred. This surface is not `live-validated` or `supported`. |
| JS/TS and Python bindings | none | `not claimed` | `not claimed` | `not claimed` | [README planning statement](../README.md) | JS/TS and Python bindings are planned, not shipped. Native macOS Swift interop is an internal implementation boundary, not a public language binding. |

## Reading This Matrix

- A platform-specific crate or symbol is not a platform-support claim by itself.
- Archived vertical evidence is historical context, not current product support.
- Fixture evidence can justify `tested`; it cannot justify `live-validated`.
- Before raising a cell to `supported`, add the public setup path, automated
  regression gate, current live closure, and a documented maintenance boundary.
