# AUV and computer-use components: source differences, upstream changes, and direction

> NOTICE: This historical review describes the recorded revisions. The [Wayland background input research](2026-09-13-wayland-background-input-research.md) records the 2026-09-13 decision, later implementation PRs, and evidence limits. Candidate rows do not authorize implementation.

Research date: 2026-09-09. Classification: docs-only research. This document records a source review and candidate follow-ups. It is not an approved implementation plan.

The [improvement candidates](2026-09-09-computer-use-improvement-candidates.md) connect reference implementations to individual AUV gaps, trigger flows, exact source lines, and acceptance criteria. That document also covers scripting, Windows UIA, and multiple devices.

## Conclusions and evidence limits

The AUV typed operations → driver → InputActionResult → run/artifact path has value. Its separation of input delivery from semantic verification also has value. This review found no evidence that requires replacement of Rust/Swift, gRPC, or the driver/tracing boundaries.

For general OS automation, an advanced design does not establish implementation leadership. Peekaboo and CUA have more complete implementations for target identity, partial delivery, concurrent resource lifecycles, and independent receiver evidence. Sources: [AUV result contract][a-input], [Peekaboo outcome][p-outcome], and [CUA behavior evidence][c-tests].

The strongest findings are **one click-count contract mismatch, one CI test-selection gap, and capability differences between entry points for the same operation**. Three other correctness risks need reproduction first: delivery and retry, target identity, and capture geometry. These risks are not reproduced production incidents. They do not establish a count of incorrect architectural decisions or a completion percentage.

Evidence labels have these meanings:

- **Source fact:** code branches, data flow, or test configuration directly establish the finding. This label does not mean GUI reproduction.
- **Inferred risk:** source code indicates a possible race or failure condition. Independent receiver evidence or fault injection remains necessary.
- **Capability gap or intentional deferral:** a public contract or production consumer is absent. This label does not automatically mean a bug.
- **Upstream test evidence:** the review examined upstream tests or records. This review did not repeat those tests. They do not prove AUV behavior.

Three research agents reviewed Peekaboo, CUA, and KWWK separately. The lead reviewed AUV and supplementary projects. Another agent examined counterevidence for the CI and entry-point findings.

This review changed no production code, ran no builds, and operated no desktop applications. It included no comparison benchmark for success rates, latency, or throughput.

## Revisions and research scope

| Project | Fixed revision | Scope |
| --- | --- | --- |
| AUV | `bae42bf9905614b19347566d5d41b3a9998e8a35` | Local code at review time. Driver, invoke, Runner, MCP, tracing, CI, and accepted contracts. |
| Peekaboo | `6cd38c0d319ab8db52b05d22409072efc4a20021` | Native macOS services, CLI/MCP, tests, and recent commits. |
| CUA | `467c103be28384502cdd77b9edd5ea46da0b8ded` | Latest native cua-driver. Follow-up review includes the September 9 commit. Cloud and agent layers remain separate. |
| KWWK main | `acc65213baa725b31153609e651f166fdaf79272` | Coding-agent host/SDK. This scope does not represent the entire KWWK desktop ecosystem. |
| KWWKComputerUseCore | `5201e300ceb58f2aaf501b20477aff5a912efb9a` | The independent macOS automation core. AUV already cites its code. |

The main recent-change period is **2026-08-10 through 2026-09-09**. Related work from July and early August has a separate earlier-work label. The user checkout of CUA remained at `02fdd98`. The latest review used a temporary research checkout and did not change the user branch.

This report adds to the [earlier capability table](2026-09-09-computer-use-framework-comparison-note.md) and corrects two scope distinctions. KWWK main and its independent native core are different comparison subjects. Platform availability for default local invoke/MCP and a selected Runner also differs.

## Comparison of code boundaries

| Technical boundary | AUV implementation at review time | Peekaboo strengths | CUA strengths | KWWK native core strengths and limits |
| --- | --- | --- | --- | --- |
| Native input | Typed paths, attempts, and disturbance. macOS uses compatibility click by default. Keyboard input includes target preconditions and partial progress. | Each event uses one transport. Outcomes distinguish refusal, partial delivery, and unknown effects. | New click paths avoid duplicate delivery. Typed effects and evidence constrain outcomes. Background routes refuse targets without sufficient proof. | Compact Swift targeted events and AX actions. Some mechanisms come from CUA, so they are not independent evidence. |
| Observation before action | AX paths include child indexes and an expected role. A consistent public snapshot/action contract remains absent. | Snapshot, process generation, and window receipt. Actions require fresh identity evidence. | Tokens have runtime/PID/window scope. Fresh AX ancestry and stale-reference refusal constrain actions. | Actions consume the latest session snapshot and match against a new tree. Score-based matches lack complete ambiguity refusal. |
| Coordinates and captures | ScreenPoint/WindowPoint exist. macOS capture results can contain the caller frame from an earlier observation. | Raster extent, density, and target evidence constrain capture. Unproven geometry causes refusal. | Capture requires fresh owner/layer/frame evidence afterward. One rebuild follows a change. | Coordinate actions detect window-frame changes. This is not a general cross-platform geometry layer. |
| Operation lifecycle | Mouse-motion mutex, stream-disconnect stop, and clipboard locks exist. Synchronous native-call cancellation and held input remain limited. | Held input has an owner, watchdog, and release logic. Composite operations are atomic. Capture has ownership rules. | Each PID has a mutation lease. SDK shutdown waits for calls, clears tokens, and stops recording. | session.finish/deinit restores the monitor. Drag requires explicit release. This is not a complete cancellation pattern. |
| Semantic effects | verified=false by default. App-owned independent verification is available. No general predicate service exists. | Exact readback covers limited nonsecure text controls. Other actions remain unverified. | confirmed/partial/refused/unknown have constraints. The independent verifier does not treat an incomplete AX tree as proof of absence. | Actions return a settled snapshot. A stable image does not prove completion of user intent. |
| Entry-point reuse | Registry, typed inputs, and recording are shared. Direct invoke/MCP and selected Runner behavior still differs. | Swift services serve CLI/MCP/Bridge. | Multiple language SDKs and the daemon share a typed native runtime. | Compact Swift client and session facade. |
| Platforms and acceptance | Code exists for three platforms. Platforms and entry points are not equivalent. CI omits many owning unit suites. | Detailed macOS behavior for native windows, menus, dialogs, and input boundaries. | Toolkit fixtures, external oracles, and explicit refusal. Wayland adaptation differs by compositor. | macOS-specific. No CUA-style behavior matrix across platforms. |

Sources: [AUV input][a-input], [AUV keyboard][a-keyboard-session], [AUV geometry][a-geometry], [Peekaboo pointer][p-pointer], [Peekaboo actions][p-actions], and [CUA token][c-token]. Other sources: [CUA output][c-output], [CUA testkit][c-tests], [KWWK client][k-client], and [KWWK core][k-core]. The detailed reviews describe the limits of these comparisons.

## Specific differences that need attention first

### 1. Default compatibility click needs new evidence for its old workaround

**Source fact:** AUV `postCompatibilityMouseEvent` sends the same event through SkyLight and then public `postToPid`. The default route uses ChromiumCompatible. The compatibility function limits the request to at most two target clicks, but invoke accepts 1–255. A triple-click request cannot retain its declared count in this native branch. Sources: [delivery][a-pointer], [count limit][a-count], and [entry-point rules and routing][a-runner-click].

**Upstream difference:** Peekaboo selected one transport in its August 9 commit [f2edb8be](https://github.com/openclaw/Peekaboo/commit/f2edb8be4fa8fc6d1cc66f6ce1fbbcda506b5c53). CUA corrected related duplicate delivery in its September 9 commit [467c103](https://github.com/trycua/cua/commit/467c103be28384502cdd77b9edd5ea46da0b8ded). CUA used an independent AppKit receiver to make sure that event counts matched requests. Current CUA foreground input uses public transport. A specific background route uses SkyLight and falls back to public transport only for an absent symbol. Source: [current CUA pointer][c-pointer].

**Inferred risk:** AUV can deliver duplicate events to some toolkits. This review did not reproduce that behavior live. Other CUA helpers still use Both, so the correction does not cover every repository path. Some applications also depend on the existing AUV offscreen primer. This finding does not justify removal of all compatibility logic.

**Direction:** a toolkit-specific workaround is unsuitable as a universal default without supporting evidence. This is an implementation choice, not evidence against CGEvent or native APIs. An independent AppKit/Electron counter is the proposed first experiment. Its evidence covers count=1/2/3, the actual recipient window, and transport. That evidence determines the conditional policy.

### 2. Fallback can replay all text after partial delivery

**Source fact:** Swift TypeText creates and sends events one character at a time. A later allocation failure returns an ordinary error without the delivered character count. Foreground Rust code can then paste the entire original text with explicit clipboard-fallback permission. Sources: [character delivery][a-keyboard] and [fallback][a-fallback].

**Inferred risk:** If an error follows delivery of a prefix, fallback can duplicate text, especially with `replace_existing=false`. Fault injection is necessary to establish occurrence and frequency. Source code alone does not establish a routine failure.

Peekaboo explicitly handles input prefixes in its August 18 commit [86b7d102](https://github.com/openclaw/Peekaboo/commit/86b7d10298d4f903b9122e252ec4a4c88b0b3de3). It blocks ordinary fallback replay after partial delivery and distinguishes retry safety. Sources: [pointer error classification][p-pointer] and [outcome][p-outcome].

The [accepted AUV keyboard contract][a-keyboard-contract] already permits explicit foreground clipboard fallback. It also records that internal TypeText progress is not measurable. AUV reports completed actions and repetitions, and background input refuses this fallback.

The missing failure distinction is no delivery, partial delivery, or unknown delivery. This finding does not reject all fallback or mean that partial progress is entirely absent.

### 3. An existing PID/window does not prove the original recipient

AUV keyboard input fixes a recipient and requires fresh PID/window ownership evidence before each repetition. This is not blind global input. However, PID-directed keyboard events still have process scope, and background input uses `require_window_focus=false`. AUV lacks same-process window-conflict detection and per-PID coordination across proof and dispatch. Sources: [keyboard][a-keyboard-session] and [native checks][a-window-check].

The AUV AX path has another specific limit. It starts from `axFirstWindow`, follows child indexes, and primarily requires the expected role at the final node. A replacement node with the same role or a different first window can satisfy this condition. Source: [AX resolution][a-ax]. These are inferred risks, not reproduced incorrect clicks.

CUA refuses competing same-PID windows for GenericKey/InsertText in its August 5 commit [1b2cb5a7](https://github.com/trycua/cua/commit/1b2cb5a706c3e5d636b683ab15336dbf35e579e0). It also uses a [per-PID mutation lease][c-lease]. Its [token registry][c-token] invalidates old references for a window after a new snapshot. Peekaboo strengthened process generation and mutation receipts in August. KWWK core captures again and matches signatures, but its highest-score match does not guarantee an unambiguous target. Sources: [CUA conditions][c-background] and [KWWK resolution][k-core].

Target identity, stale-reference refusal, and validity throughout delivery are useful reference concepts. **The retired candidate_promotion/stability remains retired. A general token system without a consumer is outside this candidate scope.**

### 4. New capture pixels can contain an old window frame

AUV Swift finds SCWindow again and uses its dimensions for capture. The Rust result uses the earlier caller `window.frame` and derives scale from its old width. If the window moves or changes size between resolution and capture, pixels and coordinate metadata can describe different observations. Sources: [Swift capture][a-capture] and [Rust result][a-capture-result].

CUA [post-capture validation][c-capture] requires consistent owner/layer/frame evidence. It rebuilds once after a change, then refuses another change. Peekaboo strengthened raster extent and density rules for popups/sheets in its September 5 commit [620563ac](https://github.com/openclaw/Peekaboo/commit/620563ac2f98a391405af681bc6db085c8b2f2d3). Source: [geometry code][p-capture].

These are not identical bugs in the same backend. That Peekaboo correction applies to classic capture. This AUV path uses ScreenCaptureKit. The shared lesson is a contract that ties images, coordinates, and window identity to one consistent observation.

### 5. Multiple clients, held input, and cancellation need lifecycle contracts

AUV already has a motion-sequence lock, stops later samples after stream disconnect, and uses a clipboard lock across processes. Source: [motion lifecycle][a-motion]. It therefore has concurrency controls. However, cancellation of an MCP future cannot interrupt a synchronous native call. Independent key/button down/up remains explicitly deferred until reliable release semantics exist. Sources: [cancellation boundary][a-cancel] and [hold deferral][a-hold].

Capture has a specific related limit. A Swift semaphore timeout does not establish that the underlying callback completed. A later capture-backend fallback therefore introduces a risk of overlapping operations. Source: [capture][a-capture]. This is not a measured hang or throughput failure.

Peekaboo added or strengthened held-input watchdogs, atomic focus+typing/modifier clicks, and SCK ownership coordination in August. CUA strengthened shutdown drain, worker join, and recording cleanup on September 4. Source: [CUA runtime][c-runtime]. Resource-scoped coordination is the useful pattern, rather than one global lock.

KWWK session finish is also relevant, but its drag handle requires explicit release. It does not prove cancellation safety. Sources: [KWWK session][k-session] and [drag][k-drag].

### 6. Existing driver capabilities do not consistently reach every entry point

Source call relationships:

```text
invoke（未选择 runner） → InvokeCommand.invoke
MCP command_adapter   → InvokeCommand.invoke
invoke（选择 runner）   → auv_cli_invoke::runner::invoke → selected services
```

These paths describe normal execution. dry-run has additional exceptions. Sources: [invoke dispatch][a-invoke] and [MCP adapter][a-mcp].

For example, direct `screen.captureRegion` returns a platform restriction outside macOS. The selected Runner branch calls CaptureService for the selected platform. Sources: [direct][a-screen] and [selected][a-runner-screen]. This does not mean that all invoke commands are macOS-only. Windows/Linux capture in a driver also does not establish equivalent availability through default MCP.

This is a source fact about capability exposure. The current [ownership exception][a-ownership] permits retention of the invoke crate. It does not establish explicit acceptance of every behavior difference. The proposed work connects a real command to a shared execution path. This finding does not justify deletion of gRPC, mandatory daemon routing for local calls, or a new auv-runtime crate.

### 7. CI dependency compilation does not run unit tests owned by each dependency

`cargo metadata --no-deps --format-version 1` reports 41 workspace members, with only auv-cli as a default member. CI runs bare `cargo test` on three platforms. Sources: [Cargo.toml][a-cargo] and [workflow][a-ci]. [Cargo package selection](https://doc.rust-lang.org/cargo/commands/cargo-test.html#package-selection) does not automatically select the unit suites owned by driver, tracing, and invoke.

Dependencies still compile. The [CLI integration suite][a-integrated] indirectly exercises invoke, recording, and MCP. It is incorrect to describe the other 40 crates as entirely untested. default-members is also reasonable for convenient local CLI use. The CI gap is the absence of explicit selection for the owning packages that need tests.

CUA has [external receiver oracles and evidence requirements][c-tests]. Its records include SHA, fixture, focus, cursor, occlusion, permitted refusals, screenshots, and video. A matrix such as “122/122” can include cases that correctly refuse operations. It does not mean that all 122 features execute. This review did not repeat that matrix. AUV needs a small set of key behavior tests that establish the actual recipient, event count, and state.

## Recent upstream changes relevant to AUV

The table groups commits by technical subject. Each commit does not represent a separate missing capability. Priorities are research recommendations, not implementation authorization.

| Upstream date | Change and commit | Specific relevance to AUV | Assessment |
| --- | --- | --- | --- |
| CUA 09-09 | [467c103](https://github.com/trycua/cua/commit/467c103be28384502cdd77b9edd5ea46da0b8ded) corrects duplicate delivery in a specific mouse route. An independent receiver supplies evidence. | AUV retains two posts. The count clamp also needs investigation. | Highest priority for reproduction. |
| Peekaboo 08-18 | [86b7d102](https://github.com/openclaw/Peekaboo/commit/86b7d10298d4f903b9122e252ec4a4c88b0b3de3) prohibits ordinary replay after partial dispatch. | Boundary between internal TypeText progress and fallback that pastes the entire text. | High priority for fault injection. |
| Peekaboo 08-20, 08-26 | [a146c035](https://github.com/openclaw/Peekaboo/commit/a146c035), [cc4c714c](https://github.com/openclaw/Peekaboo/commit/cc4c714c): generation-bound observation/mutation. | Process/window/snapshot invalidation and fresh evidence for dynamic targets. | High-priority correctness boundary. |
| Peekaboo 08-16, 08-22 | [aef709fe](https://github.com/openclaw/Peekaboo/commit/aef709fea3d18f7ca989c3ab6c1a60428f2f3e51), [7a1d13aa](https://github.com/openclaw/Peekaboo/commit/7a1d13aab2bfa05a99c0896632aeb6fa08023c1d): held input and atomic composite operations. | AUV drag/hold needs ownership, cancellation, and guaranteed release before public availability. | Requires a real consumer first. |
| Peekaboo 08-11, 08-27 | [f2773c36](https://github.com/openclaw/Peekaboo/commit/f2773c3628feffadc559eeb3667dbb04f0ff98d4), [715caa24](https://github.com/openclaw/Peekaboo/commit/715caa24bbe1d9accaeff283d3e8b1a96a7338b4): SCK ownership with continued concurrency for classic capture. | Timeout does not mean native work completed. Coordination differs by backend. | Stronger lifecycle rules. |
| Peekaboo 09-05, CUA 09-02 | [620563ac](https://github.com/openclaw/Peekaboo/commit/620563ac2f98a391405af681bc6db085c8b2f2d3), [808c014](https://github.com/trycua/cua/commit/808c0142dc7c8c84cde3a0d1fc5118194898a7a3): capture geometry/metadata and capture-only. | AUV risks new pixels with an old frame. Copying new tools is unnecessary. | High priority for contract evidence. |
| Peekaboo 08-26 | [1d2b6614](https://github.com/openclaw/Peekaboo/commit/1d2b6614bfeeef9c3a38d3b80448db664f06a144): readback for limited background text. | AUV retains delivery/semantic separation. Specific app results can consume verification. | Connect individual semantic operations. |
| CUA 08-30 | [99f27ee](https://github.com/trycua/cua/commit/99f27eeb96481a155fe10f6dee6a131cc0de8b9e): stronger cross-platform behavior evidence. | CI needs owning suites and independent receivers. Refusal also needs observable evidence. | Foundational priority. |
| CUA 08-27, 09-07 | [9596fb3](https://github.com/trycua/cua/commit/9596fb334f3eeec541979ccf5f0ef9ef360da0c6), [c5a15f3](https://github.com/trycua/cua/commit/c5a15f3df3b29ffbe774de9f33d632fe75afec75): KWin identity and conditional Hyprland input. | The AUV portal/GNOME scope is narrower. Compositor identity adapters need separate maintenance and tests. | Platform extension, not an incorrect direction. |
| CUA 09-04 | [aabb208](https://github.com/trycua/cua/commit/aabb2082c170289256f0c8d9db4cce094c778578): SDK shutdown cleanup. | Alignment of frontend-owned lifecycle with native termination. | Useful reference without a copied runtime crate. |
| CUA 08-17, 08-25 | [3f791b2c](https://github.com/trycua/cua/commit/3f791b2cfec23d690cd34e6d275b6cbe1a8acc05), [85d77792](https://github.com/trycua/cua/commit/85d77792e2f400a88f4b77c1218e388945c4b01c): browser profile integration and debugging cleanup. | Exact native window ↔ CDP page identity can extend browser/app operations. | Adjacent capability, not an OS requirement. |
| KWWK main 08-19, 08-22 | [9bfed81](https://github.com/EYHN/kwwk/commit/9bfed818295a0203b79d3ae5a80f0ec424d82d21), [562f1e0](https://github.com/EYHN/kwwk/commit/562f1e0f46567f41e76ba36c6279ead083c1758e): caller wait and task lifecycle separation. Truncated previews retain complete output. | Reference for long tasks and artifact producers/consumers. AUV already has durable artifacts. | Host-layer reference, not a native deficit. |
| KWWK main 09-08 | [052cd0c](https://github.com/EYHN/kwwk/commit/052cd0cc5f30a23784451f280b0e1475f0c2d726): beforeRunEnd and renewed cancellation evidence. | Lifecycle design for an embedding host. | No agent loop is necessary for parity. |
| KWWK native core | Zero commits in the last 30 days. Zero runtime implementation changes after the revision that AUV cites. | No recent native update awaits adoption. | No invented gap. |

Earlier implementations fall **outside the recent 30-day period**:

- CUA [07-31 typed effects](https://github.com/trycua/cua/commit/8e0a92e3dbf20134be9922f9e0dc847addcc92fa).
- CUA [08-02 snapshot refs](https://github.com/trycua/cua/commit/d8ae6df643df5049505a327b88abc2644a25b209).
- CUA [08-05 exact target](https://github.com/trycua/cua/commit/1b2cb5a706c3e5d636b683ab15336dbf35e579e0).
- CUA [08-05 capture validation](https://github.com/trycua/cua/commit/bc90373362cb7c521b1ff03f94457d5de618095c).
- Peekaboo [08-09 single transport](https://github.com/openclaw/Peekaboo/commit/f2edb8be4fa8fc6d1cc66f6ce1fbbcda506b5c53).

## Correct comparison subjects and technical origins for KWWK

KWWK main is a coding agent. Its computer-use commit [87e87e7](https://github.com/EYHN/kwwk/commit/87e87e7e627bc07385b6e14ba6b10b7c80c134ba) belongs to `origin/eyhn/feat/background-computer-use`. It is not an ancestor of main. The evidence therefore does not establish removal from main.

The independent [kwwk-computer-use-core](https://github.com/EYHN/kwwk-computer-use-core) is the desktop core. AUV native source already cites its May 22 revision `eddd9e5`. Source: [origin comment][a-source]. Subsequent history contains only MIT attribution in [2b8da82](https://github.com/EYHN/kwwk-computer-use-core/commit/2b8da82e1232e15191b1d3d838378e0806cdf22a) and its later merge. No runtime code changed.

Its strengths are the existing client/session composition, actions after observations, drag, and foreground restoration boundaries. These are not new mechanisms from the last month that AUV missed. Some core mechanisms also come from CUA, as its [attribution][k-license] records. AUV, KWWK core, and old CUA are not three unrelated reliability experiments.

Improvements to skills indexes, LLM image providers, and agent loops in KWWK main do not establish AUV OS API gaps. They also do not justify restoration of SkillBundle. Later output-related commits on August 22 removed task_read search and artifact GC. A capability in an intermediate commit is not necessarily a current capability.

## Other relevant components

These projects contain relevant modules. Stars, advertised success rates, and tool counts do not determine their order. The supplementary review covers only the listed source files and official documents. Its depth is less than the three main reviews.

| Project / fixed revision | Component most relevant to AUV | Strength | Applicability limits |
| --- | --- | --- | --- |
| [oh-my-pi][omp-doc] / `a33cc268`, 09-08 | Rust pi-natives desktop and JS computer worker. | [Frame-bound coordinates][omp-frame] reject absent captures, out-of-bounds points, and changed window dimensions. The [AX registry][omp-ax] has generation. The [worker supervisor][omp-worker] owns restart and state invalidation. | A useful native API for scripts. Current documents state that prebuilt Wayland lacks PipeWire capture. Four declared backends do not establish equivalent behavior. |
| [terminator][terminator-readme] / `73a381c0`, 06-02 | Rust Windows UIA Locator/selector. | [Locator][terminator] combines scope, search, and wait predicates such as Exists/Visible/Enabled/Focused. | The current README states Windows-only, despite old macOS claims elsewhere. Default locator timeout is 0. Automatic waits do not apply to every call. |
| [computer-use-mcp][zavora] / `8a140ecb`, 09-08 | Rust N-API and TS tool registry / doctor. | A [single registry][zavora] constrains declarations, schemas, and executors. [doctor][zavora-doctor] provides environment capability checks. These support entry-point consistency. | Sixty-four tools do not establish 64 capabilities with cross-platform evidence. A cached target is not process-generation identity. |
| [Microsoft UFO][ufo] / `364eb796`, 09-02 | Automator receiver/command dispatch. | One workflow combines GUI actions and native application APIs. This supports the direction of AUV app-owned typed operations. | Primarily a Windows app-automation reference. The complete agent and multi-device system is outside the base AUV driver scope. |
| [KWWKComputerUseCore][k-client] / `5201e300`, 08-05 | Native Swift client/session/AX action. | The most direct independent macOS component reference. AUV already shares a source lineage. | Not a recent-update target. Settled snapshots and score-based matches still need stronger effect and ambiguity boundaries. |
| [libei](https://libinput.pages.freedesktop.org/libei/) | Low-level component for Wayland input. | Standard interface for research into portal/compositor/native-input permissions and lifecycles. | Infrastructure, not a complete automation framework. It does not establish arbitrary background-window input. |

CUA [native-window ↔ CDP binding][c-browser] also merits separate module research. It establishes unique bounds/cardinality, refuses ambiguity, and limits title heuristics to read-only use. These properties have more value than a connection to the Chrome debugging port alone.

## Directions to retain or correct

| Decision | Specific subject | Conclusion supported by evidence |
| --- | --- | --- |
| Retain | Typed InputActionResult, delivery/verification separation, disturbance, and run artifacts. | The abstraction fits the problem. Effect/refusal/unknown can grow incrementally. Animation or submitted input does not prove success. |
| Retain | Rust + Swift native calls, capability-oriented drivers, and frontend-owned Run lifecycle. | No matched performance/reliability benchmark supports a language change, gRPC removal, or an aggregate runtime crate. |
| Retain | Wayland portal, refusal for unknown window origins, and the historical Linux X11 deferral. | These are explicit scope and platform limits. The [capture boundary at review time][a-linux] reports them honestly. They do not establish an incorrect direction. |
| Retain | Deferred held input without guaranteed release. Retired candidate-action/SkillBundle. | The contract deferrals are reasonable. Expansion requires a real consumer and an approved slice. |
| Correct | Default historical Chromium compatibility recipe without receiver regressions for upstream corrections. | This specific implementation choice needs new evidence. The count-clamp mismatch is a source fact. |
| Strengthen | Target/process/window/snapshot identity, partial retry, and capture metadata. | Existing primitives need stricter invariants and executable evidence. |
| Connect | Consistent availability and results for one typed operation through CLI/MCP/selected Runner. | This is current core-convergence work. It takes priority over another peripheral system. |
| Add evidence | CI package selection and a few key native fixtures. Observable failures and refusals. | A reliable baseline comes before support or leadership claims. |

AUV has a clear product scope for application operations that support recording, inspection, and reuse. That scope differs from the most complete collection of general OS primitives. Each claim needs separate evidence.

Current evidence supports continued work within the AUV product scope. It **does not support a claim of overall leadership**. It also does not support conversion of gap counts into development weeks or a percentage of lag.

## Candidate next steps and acceptance evidence

These are candidates only. This review implemented none of them and did not expand the roadmap. The preferred sequence starts with test-only reproduction. A narrow correction follows evidence that establishes the behavior.

| Order | Candidate slice | Smallest meaningful acceptance evidence |
| --- | --- | --- |
| 1 | Explicit CI selection of owning test packages. | The platform-specific list selects and runs expected driver/common/invoke/tracing tests. CLI integration remains. Feature selection avoids indiscriminate activation of all features. |
| 2 | macOS click count and single-transport routing. | An independent AppKit/Electron receiver records down/up/count/target. Actual consumption matches requests for 1, 2, and 3 clicks. Applications that require the primer retain behavior. |
| 3 | Retry classification after partial TypeText delivery. | Fault injection at character N establishes no replay of the entire partially delivered request. Results distinguish no delivery, partial delivery, and unknown delivery. |
| 4 | Target identity for one existing observe→action consumer. | Cases cover two same-PID windows, window closure/recreation, and same-role node replacement. Unproven targets produce explicit refusal with original evidence. |
| 5 | Consistent geometry for window capture. | Cases cover movement/resize between resolution and capture, and scale changes. Results contain metadata from one observation or bounded retry/refusal. |
| 6 | One existing driver capability across entry points. | Examples include captureRegion or general scroll. CLI/MCP/selected Runner evidence establishes the same typed contract and recording. |
| 7 | Later drag/hold, general semantic actions, and predicates. | A real consumer and cancellation/release/verification boundaries come first. Platform availability follows that contract. |

The existing AUV event-recorder injection point works without a GUI and primarily covers key combinations. It does not establish Swift pointer-post or per-character TypeText behavior. This review added no false regression tests that merely repeat `min/max` expressions. Upstream fixture success did not count as AUV success.

## Research reproducibility

The review examined repository revisions, recent git logs, relevant git show/blame output, source files, and tests. Branch containment and merge-base established the KWWK feature-branch relationship. cargo metadata established the AUV workspace and default-members. CI shell commands and official Cargo rules supplied the package-selection interpretation.

The supplementary review read official source/README files at fixed GitHub revisions. It did not install or run external code.

The three local target paths were `~/Git/github.com/openclaw/peekaboo`, `~/Git/github.com/trycua/cua`, and `~/Git/github.com/EYHN/kwwk`. The additional core and updated CUA sources used temporary research directories. Existing user branches did not change. Document checks cover only Markdown paths, format, and diff. They do not establish runtime behavior.

[a-pointer]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift#L251
[a-count]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift#L356
[a-runner-click]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli-invoke/src/runner.rs#L535
[a-keyboard]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Keyboard.swift#L104
[a-keyboard-session]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/src/session.rs#L563
[a-fallback]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/src/session.rs#L629
[a-keyboard-contract]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/docs/ai/references/invoke-cli/2026-09-08-targeted-keyboard-contract.md
[a-window-check]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Window.swift#L574
[a-ax]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/AxTree.swift#L138
[a-capture]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Capture.swift#L65
[a-capture-result]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/src/session.rs#L1822
[a-input]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-common/src/input.rs#L509
[a-hold]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-common/src/input.rs#L250
[a-invoke]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/src/commands/invoke.rs#L76
[a-mcp]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/src/commands/mcp.rs#L253
[a-cancel]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/src/commands/mcp.rs#L318
[a-screen]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli-invoke/src/commands/screen.rs#L45
[a-runner-screen]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli-invoke/src/runner.rs#L159
[a-ownership]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/docs/ai/references/invoke-cli/2026-08-04-core-cli-command-ownership-design.md#L220
[a-cargo]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/Cargo.toml#L45
[a-ci]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/.github/workflows/check.yml#L43
[a-integrated]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/tests/integrated.rs
[a-linux]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-linux/src/window.rs#L34
[a-motion]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/src/runner/local_driver.rs#L824
[a-source]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift#L608
[a-geometry]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-common/src/geometry.rs#L24
[p-pointer]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/WindowRoutedPointerDriver.swift#L559
[p-receipt]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Strategy/DesktopOperationSnapshotReceiptValidator.swift
[p-actions]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/UIAutomationService+ElementActions.swift#L30
[p-literal]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/ExactLiteralTypingEffectConfirmation.swift#L39
[p-outcome]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooFoundation/Sources/PeekabooFoundation/DesktopActionOutcome.swift#L8
[p-held]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/ExactWindowHeldPointerLifecycle.swift
[p-capture]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/Capture/LegacyWindowCaptureGeometry.swift#L37
[c-pointer]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L259
[c-background]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-core/src/background_input.rs#L181
[c-lease]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/platform-macos/src/background_mutation.rs
[c-token]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-core/src/element_token.rs
[c-capture]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/platform-macos/src/capture.rs#L556
[c-output]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-contract/src/outputs.rs#L408
[c-verify]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-contract/src/verification.rs
[c-tests]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-testkit/src/e2e.rs#L525
[c-browser]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-core/src/browser/binding.rs
[c-runtime]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-sdk/src/runtime.rs#L223
[k-client]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseClient.swift#L89
[k-core]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseCore.swift#L197
[k-session]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseSession.swift
[k-drag]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseActions.swift#L389
[k-license]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/LICENSES/cua-driver-MIT.txt
[omp-frame]: https://github.com/can1357/oh-my-pi/blob/a33cc26824e3c91edd9fa42d681f10dceb4ac2f0/crates/pi-natives/src/desktop/frame.rs
[omp-ax]: https://github.com/can1357/oh-my-pi/blob/a33cc26824e3c91edd9fa42d681f10dceb4ac2f0/crates/pi-natives/src/desktop/ax.rs
[omp-worker]: https://github.com/can1357/oh-my-pi/blob/a33cc26824e3c91edd9fa42d681f10dceb4ac2f0/packages/coding-agent/src/tools/computer/supervisor.ts
[omp-doc]: https://github.com/can1357/oh-my-pi/blob/a33cc26824e3c91edd9fa42d681f10dceb4ac2f0/docs/computer-use.md
[terminator]: https://github.com/mediar-ai/terminator/blob/73a381c0c1c33eda55f2c0ecb1d918bf5ec7561a/crates/terminator/src/locator.rs
[terminator-readme]: https://github.com/mediar-ai/terminator/blob/73a381c0c1c33eda55f2c0ecb1d918bf5ec7561a/README.md
[zavora]: https://github.com/zavora-ai/computer-use-mcp/blob/8a140ecbf6437e1e2f5c033abdd5b8c1789d2878/src/registry/registry.ts
[zavora-doctor]: https://github.com/zavora-ai/computer-use-mcp/blob/8a140ecbf6437e1e2f5c033abdd5b8c1789d2878/src/session/doctor.ts
[ufo]: https://github.com/microsoft/UFO/blob/364eb7969d392e857299ceaf14bd6057e5b00078/ufo/automator/puppeteer.py

The [background input, AX tree, and media review](2026-09-09-background-ax-and-media-gap-review.md) further separates existing risks. It also records new capability candidates MEDIA-1～3. Most BG/AX groups overlap with earlier candidates, so their counts do not add to a bug total.
