# Orca Computer Use and AUV: Capability and Implementation Comparison

> Research snapshot: 2026-08-09. **Orca** refers specifically to
> [`stablyai/orca`](https://github.com/stablyai/orca), which describes itself as
> an ADE for AI coding agents. It does not refer to other workflow or terminal
> agent projects with the same name. External source links are pinned to commit
> [`04f7123`](https://github.com/stablyai/orca/tree/04f7123d26921795a3e582a2e0713bcb0f2b1076)
> to avoid drift on `main`. Capability status distinguishes among *publicly
> implemented*, *current macOS evidence in AUV*, and *unproven or intentionally
> omitted*. It does not represent a roadmap commitment.

## Summary

Orca Computer Use is a productized desktop-control surface for general-purpose
coding agents. Its unified `orca computer` CLI reads the accessibility tree and
a screenshot, performs semantic actions against short-lived element indexes,
and returns a fresh snapshot. Providers are available for macOS, Linux, and
Windows. AUV's current core is narrower and more auditable: macOS-first typed
drivers and operations, explicit input-delivery paths and disturbance metadata,
run tracing, and durable artifacts. AUV also keeps input delivery separate from
semantic verification.

The comparison is therefore more specific than a feature count. Orca currently
provides the stronger **cross-platform desktop-operation surface that any CLI
agent can consume directly**. AUV provides stronger **contractual guarantees
around delivery facts, fallback, disturbance, artifacts and run recording, and
app-owned verification**. AUV should not adopt Orca's broad CLI as a shared
runtime or create a parallel action-result schema.

## Capability matrix for currently verifiable surfaces

| Capability | Orca Computer Use (public implementation) | Current verifiable AUV surface | Key difference and evidence level |
|---|---|---|---|
| Product boundary | A general-purpose local desktop CLI for coding agents within an ADE, also distributed through skills and MCP. | A typed operation, driver, run, and artifact core designed for inspectable and replayable application use; not an agent IDE. | The frontend goals differ. Orca's worktree and agent orchestration should not be classified as Computer Use capabilities or missing AUV features. Orca [README](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/README.md#L188-L242); AUV [terminology](../../../TERMS_AND_CONCEPTS.md#runtime-responsibility). |
| Supported desktop platforms | Native Swift helper on macOS, AT-SPI/Python on Linux, and UI Automation/PowerShell on Windows, exposed through a unified provider capability response. | Live invoke commands are explicitly macOS-only. `auv-driver` includes Linux and Windows adapters in its local selector, but this table does not treat an enum as behavioral evidence. | Orca has productized cross-platform coverage. AUV's currently verified core lane is macOS. Orca [provider source](https://github.com/stablyai/orca/tree/04f7123d26921795a3e582a2e0713bcb0f2b1076/native); AUV [`LocalDriver`](../../../../crates/auv-driver/src/lib.rs). |
| Running app and window discovery | `list-apps` and `list-windows`, with selection by bundle ID, name, PID, window ID, or window index. | `window.list`, normalized `WindowSelector`, and `app.activate`, but no public app-list invoke command. | Orca better supports agents that must discover a target from scratch. AUV assumes that callers usually provide a target to a typed operation. Orca [documentation](https://www.onorca.dev/docs/cli/computer-use#selecting-an-app); AUV [window command](../../../../crates/auv-cli-invoke/src/commands/window.rs). |
| Accessibility-tree observation | Each `get-app-state` response includes tree text, element frames, focus, windows, and truncation state. Element indexes are valid only for the latest snapshot. Before a macOS action, Orca also checks the element signature against a fresh snapshot. | The macOS driver contains AX-tree and native-tree modules. The current invoke path primarily exposes window or display capture plus OCR, not an equivalent public command that obtains a tree and acts on an index. | Orca exposes a more complete agent-consumption contract. AUV's AX support is a driver capability, not a unified Computer Use ABI. Orca [guide](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/skill-guides/computer-use.md#L38-L49) and [snapshot/signature gate](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L422-L445). |
| Image observation | Returns a screenshot of the target window by default. The JSON CLI writes the image to a restricted temporary path and returns that path. `--no-screenshot` disables capture. | `display.capture`, `screen.captureRegion`, and `window.capture` emit PNG artifacts. Capture results also carry coordinate and backend metadata. | Orca optimizes the payload for one agent-loop iteration. AUV optimizes durable evidence that can be inspected within a run. Orca [documentation](https://www.onorca.dev/docs/cli/computer-use#screenshots); AUV [capture-frame terminology](../../../TERMS_AND_CONCEPTS.md#capture-frame). |
| OCR and visual targeting | Provider capabilities declare `ocr: false` on macOS, Linux, and Windows. The agent must interpret screenshots or read the AX tree. | macOS invoke commands include `screen/window.findText`, `waitForText`, and `clickText`, projecting native OCR results into click points. | AUV has stronger evidence for OCR anchors on macOS. Orca does not claim OCR as a Computer Use capability. Orca [macOS capabilities](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L478-L520); AUV [screen commands](../../../../crates/auv-cli-invoke/src/commands/screen.rs). |
| Semantic click and secondary actions | `click --element-index` tries `AXPress`, `AXConfirm`, or `AXOpen`; right-click tries `AXShowMenu`. Explicit `perform-secondary-action` accepts only actions advertised by the element. | The input-path model includes `AxPress`, `AxFocus`, `AxSetValue`, and `AxScroll`. Existing public text-click commands use an OCR point plus typed input. | Both prefer semantic actions. Orca exposes them through one index-based CLI, while AUV emphasizes driver-level path facts. Orca [implementation](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L728-L800); AUV [`InputDeliveryPath`](../../../../crates/auv-driver-common/src/input.rs). |
| Raw pointer input, drag, and scroll | Coordinate fallback for clicks, plus drag and scroll. Coordinates are window-local and transformed using the screenshot scale. | Window-relative typed click and scroll traits, plus invoke commands for mouse motion, keys, text, and paste. | AUV has stronger coordinate types and delivery-path modeling. Orca provides a unified cross-platform command that agents can call immediately. Orca [guide](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/skill-guides/computer-use.md#L90-L117); AUV [`WindowInput`](../../../../crates/auv-driver-common/src/input.rs). |
| Text and value input | `set-value` writes through AX and reads the value back. Otherwise, `type-text`, `paste-text`, keys, and hotkeys use focus-dependent synthetic input. | `input.focusText`, `axFocusText`, `typeText`, `pasteText`, and `key`. An input action may set semantic `verified` to true only after explicit read-back. | The semantics are aligned. AUV makes unverified delivery a project-level invariant, while Orca returns verified or unverified metadata with the action in this API. Orca [set-value and typing](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L803-L834); AUV [result invariant](../../../../crates/auv-driver-common/src/input.rs). |
| Fallback and foreground disturbance | When semantic AX click is unavailable, Orca uses a synthetic click at the element frame. It provides `restore-window` and documents focus and occlusion limitations. | `InputActionResult` records the selected path, all attempts, and mouse, focus, and clipboard disturbance. Input modes explicitly distinguish background-only, background-preferred, and foreground-preferred behavior. | AUV's fact model is more detailed. Orca exposes `path` and `fallbackReason` and documents operational constraints, but the reviewed surface has no equivalent disturbance triple. Orca [fallback](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L739-L774); AUV [`InputActionResult`](../../../../crates/auv-driver-common/src/input.rs). |
| Post-action verification | Every action returns the current snapshot, and the documentation prescribes a snapshot → act → snapshot loop. `set-value` supports value read-back; other synthetic actions remain unverified. | Input delivery and application-semantic verification are separate. The latter is represented by an app-owned typed result, event, or artifact. Successful dispatch does not imply semantic success. | Both reject the assumption that a successful click means the task succeeded. AUV makes this boundary stricter and suitable for durable inspection. Orca [loop](https://www.onorca.dev/docs/cli/computer-use#snapshot--act--snapshot); AUV [semantic verification](../../../TERMS_AND_CONCEPTS.md#semantic-verification). |
| Permissions and sensitive data | Exposes capability and permission status. macOS requires Accessibility and Screen Recording permissions. Orca blocks several password-manager apps and supports stdin to avoid shell history, although Linux and Windows action payloads are still written briefly to operation files. | macOS probes Screen Recording, ScreenCaptureKit, Accessibility, and Automation. Run records and artifacts use the tracing store. | Orca has more productized warnings and denylists for agent operations. AUV can learn from its secret-transport warnings, but temporary files should not be treated as a security boundary. Orca [guide](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/skill-guides/computer-use.md#L20-L31) and [Linux/Windows caveat](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/skill-guides/computer-use.md#L80-L88); AUV [permission command](../../../../crates/auv-cli-invoke/src/commands/app.rs). |
| Agent integration | Distributes a small discovery stub. The agent obtains the complete, version-matched guide at runtime with `orca skills get <topic>`. Commands recommend `--json`; MCP registration is also available. | The CLI and built-in MCP decode into the same command-local typed input. Each frontend owns its run context. | Orca's thin-stub plus live-versioned-guide model is a useful distribution pattern. AUV's typed command routing and run ownership already align with its own architecture. Orca [skills documentation](https://www.onorca.dev/docs/cli/skills#hybrid-stubs-vs-the-live-guide); AUV [CLI invoke boundary](../../../TERMS_AND_CONCEPTS.md#cli-invoke-boundary). |
| Tracing, artifacts, and inspection | Screenshots are temporary CLI outputs. This review found no public contract that represents Computer Use as an append-only run record with artifacts and a reader. | `auv-tracing` persists span, event, and artifact metadata and bodies; inspect provides the read side. Captures and operations can emit artifact receipts. | The presence of Orca screenshots is not evidence of an inspectable run record equivalent to AUV's. AUV [artifact and inspect terminology](../../../TERMS_AND_CONCEPTS.md#artifact). |
| Replay | The reviewed material shows that an agent can repeat the CLI loop. It does not establish a durable UI replay contract for Computer Use. | The project mission and its artifact/run model provide a basis for future replay. This is not evidence that general UI replay is already implemented. | Neither project can claim complete, general-purpose desktop replay from the current evidence. |

## Implementation structure visible in source

```text
Orca agent / `orca computer` CLI
  -> TypeScript RPC methods + single-flight Node sidecar
  -> provider capability handshake
  -> macOS: authenticated Unix-socket Swift helper
     Linux: per-operation JSON file -> Python AT-SPI/GDK adapter
     Windows: per-operation JSON file -> PowerShell UIAutomation/Win32 adapter
  -> snapshot (AX tree + optional PNG) / semantic action -> synthetic fallback
  -> next snapshot returned to the agent

AUV typed command or app operation
  -> `auv-driver` capability (macOS Swift through `swift-bridge`)
  -> `InputActionResult` + direct app-owned result
  -> tracing event/artifact receipt (frontend owns the run context)
  -> separate app-owned semantic verification when required
```

The direct evidence is distributed across the implementation. Orca defines its
RPC command table in
[`computer.ts`](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/src/main/runtime/rpc/methods/computer.ts).
The sidecar serializes calls, terminates after a timeout, and restarts on the
next request ([client](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/src/main/computer/sidecar-client.ts#L118-L313)).
The macOS helper advertises capabilities during its handshake and communicates
over an authenticated socket
([client](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/src/main/computer/macos-native-provider-client.ts#L28-L219)).
Linux uses PyGObject's `Atspi` and `Gdk`; Windows uses .NET
`UIAutomationClient` and Win32 `SendInput`. The providers do not share a single
cross-platform GUI library ([Linux](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-linux/runtime.py#L1-L43),
[Windows](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-windows/runtime.ps1#L1-L136)).

The macOS action handler includes the current post-action snapshot in its
response, with one bounded allowance for a window change. Re-observation is
therefore enforced in the implementation as well as recommended in the guide.
On Linux and Windows, the TypeScript client caches a short-lived snapshot to
associate an index with an action request. See the macOS
[response construction](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L245-L258)
and the [script-provider cache](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/src/main/computer/desktop-script-provider-client.ts).

The evidence does not support a claim that Orca Computer Use contains a visual
model or a visual-agent loop. Providers publicly report `ocr: false`, and the
source exposes OS accessibility and screenshot primitives. The calling CLI
agent decides how to interpret screenshots and when to repeat the loop. This is
an inference from source, not an Orca product claim about visual models.

## Small slices AUV could adopt

These are comparison findings, not approved implementation work.

1. **Capability negotiation with explicit degradation reasons.** Orca's
   provider handshake separates apps, windows, observation, and actions so that
   the frontend can reject unsupported operations before dispatch. AUV could,
   with owner approval, add queries for *implemented* capabilities at the
   existing driver descriptor or Protobuf boundary and reuse attempts and
   fallback facts from `InputActionResult` in direct results. This should not
   introduce a parallel action schema.

2. **A fresh-observation-token constraint.** Orca limits element indexes to the
   most recent snapshot and requires another observation after navigation,
   scrolling, or repainting. If an AX or recognition candidate-to-action slice
   is approved for AUV, the consumer could validate its source artifact,
   surface identity, and freshness before using a candidate. The validation
   belongs in the existing evidence and artifact contract, not in a CLI-local
   cache.

3. **Version-matched agent guides.** Orca's stub does not duplicate flags; the
   complete guide comes from the running version. If an agent-facing
   distribution surface is approved for AUV, this model could reduce drift
   between skill documentation and the binary. The guide would need to state
   that delivery is not semantic success and that run and artifact evidence is
   mandatory.

4. **Parts that do not fit AUV's current core boundary.** Per-operation JSON
   files and PowerShell or Python adapters provide practical Linux and Windows
   delivery for Orca, but they are not an appropriate AUV core boundary for
   secrets or durable evidence. AUV's typed Rust driver with `swift-bridge` is
   the current macOS lane. Cross-platform expansion requires an owner-approved
   producer and consumer plus corresponding evidence; Orca's coverage of three
   operating systems is not sufficient reason to expand AUV's scope.

## Evidence boundaries that remain open

- This review did not run Orca on all three operating systems. In this note,
  “available” means supported by source and official documentation, not
  independently verified with a live probe.
- Orca's temporary screenshot export does not prove that no recording exists.
  It means only that this review found no public append-only run, artifact, and
  reader contract equivalent to AUV's.
- The existence of AUV Linux and Windows crates is not evidence for every
  capability in this table. A platform matrix would require a separate live
  evidence pack for each driver.
