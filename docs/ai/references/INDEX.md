# AUV Reference Index

`docs/ai/references/` 下 **299** 篇 reference 的归类索引（不含本文件）。
文件仍保持扁平路径（避免破坏代码与文档互链）；本索引负责导航。

维护：新增 reference 时按命名规范落盘，并在本文件对应 lane 补一行。

## 命名规范

```text
YYYY-MM-DD-<topic-slug>-<doc-type>.md
```

## 文档类型

| 后缀 / 类型 | 用途 |
|---|---|
| `design` | 已接受或待批准的功能/边界设计 |
| `plan` / `implementation-plan` | 实现步骤与依赖顺序 |
| `handoff` | 切片完成后的交接说明 |
| `evidence` / `evidence-pack` | 可复现证据、benchmark、smoke 记录 |
| `closure` / `live-closure` / `design-closure` | 切片或 gate 的关闭记录 |
| `review` / `graduation-review` / `verdict` | 评审、毕业门、gate 裁定 |
| `roadmap` | 多切片路线图（不自动批准下游切片） |
| `matrix` / `taxonomy` / `inventory` | 对照表、分类、退役清单 |
| `freeze` / `acceptance` | 阶段冻结或验收 |
| `note` | 无标准后缀的 durable 笔记 |

## 快速入口

| 你想… | 从这里开始 |
|---|---|
| 理解核心术语 | [`TERMS_AND_CONCEPTS.md`](../../TERMS_AND_CONCEPTS.md) |
| 看 core lane 路线图 | [`2026-06-13-auv-core-lane-roadmap.md`](2026-06-13-auv-core-lane-roadmap.md) |
| 看 invoke / CLI 设计 | [`2026-06-11-auv-cli-invoke-driver-console-design.md`](2026-06-11-auv-cli-invoke-driver-console-design.md) |
| 看 inspect viewer 设计 | [`2026-05-19-trace-run-inspect-design.md`](2026-05-19-trace-run-inspect-design.md) |
| 看 scan / temporal 线 (S) | [`2026-07-05-auv-s1-bounded-contract-graduation-review.md`](2026-07-05-auv-s1-bounded-contract-graduation-review.md)（S1 bounded contracts）· [`2026-07-04-auv-s-line-graduation-review.md`](2026-07-04-auv-s-line-graduation-review.md)（状态审计）· [`2026-07-02-auv-scan-s0-charter.md`](2026-07-02-auv-scan-s0-charter.md) |
| 看设计系统 / viewer UI | [`../../design/README.md`](../../design/README.md) |
| 查 agent 写作规范 | 仓库根 [`AGENTS.md`](../../../AGENTS.md) |
| 看 Qodana 分层运营 | [`2026-07-03-auv-qodana-operating-model.md`](2026-07-03-auv-qodana-operating-model.md) |
| 查已归档 AX copilot | [`../../archive/verticals/ax-copilot/`](../../archive/verticals/ax-copilot/) |

## Lane 总览

| Lane | 状态 | 说明 | 篇数 |
|---|---|---|---:|
| `core/runtime` | Active | AUV core runtime、contract、graduation、query-readiness | 39 |
| `core/invoke-cli` | Active | invoke 路由、CLI handler、catalog | 14 |
| `core/api-mcp` | Active | session API、proto、MCP 前端 | 19 |
| `core/inspect-trace` | Active | run 录制、inspect viewer、trace | 6 |
| `core/driver-macos` | Active | auv-driver、macOS 输入/窗口/权限 | 20 |
| `core/view-parser` | Active | view-parser IR 与 inspect 消费 | 40 |
| `core/scenebridge` | Active | cross-app scene identity / grounding | 8 |
| `core/recognition` | Active | RecognitionResult、detector 边界 | 5 |
| `vertical/minecraft` | Paused vertical | Minecraft 3D spatial 探针；MC20 pause decision 已落地 | 58 |
| `vertical/osu` | Graduation candidate | osu benchmark；G-series 需单独 owner 批准 | 10 |
| `vertical/balatro` | Graduation candidate | 第三垂直消费探针 | 4 |
| `vertical/netease-music` | Product crate | 网易云音乐 app-local 命令 | 12 |
| `vertical/qqmusic` | Historical evidence | QQ 音乐早期探针与 GLM 证据 | 8 |
| `vertical/game-observe` | Observe-only | Steam/STS 等 observe-only fixture | 4 |
| `archive/skill-bundle-retirement` | Retired | SkillBundle / recipe 退役记录 | 5 |
| `archive/phase-history` | Historical | Phase 1–3 冻结与验收 | 5 |
| `archive/ax-copilot` | Archived vertical | macOS AX copilot；见 docs/archive/verticals/ax-copilot/ | 2 |
| `misc` | Mixed | 跨 lane 或尚未归入单一主题的笔记 | 38 |

## 按 Lane 列出

### `core/runtime` — Active

AUV core runtime、contract、graduation、query-readiness

#### design (3)

- [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-auv-core-c1-action-attempt-admission-design.md)
- [`2026-06-29-auv-core-a3-stage-status-triad-helper-design.md`](2026-06-29-auv-core-a3-stage-status-triad-helper-design.md)
- [`2026-06-29-auv-core-x1-third-vertical-scouting-design.md`](2026-06-29-auv-core-x1-third-vertical-scouting-design.md)

#### graduation-review (7)

- [`2026-06-27-auv-core-a-query-readiness-graduation-review.md`](2026-06-27-auv-core-a-query-readiness-graduation-review.md)
- [`2026-06-28-auv-core-a2-stage-quality-graduation-review.md`](2026-06-28-auv-core-a2-stage-quality-graduation-review.md)
- [`2026-06-30-auv-core-a5b-query-d2-falsifier-graduation-review.md`](2026-06-30-auv-core-a5b-query-d2-falsifier-graduation-review.md)
- [`2026-06-30-auv-core-x3-third-donor-graduation-review.md`](2026-06-30-auv-core-x3-third-donor-graduation-review.md)
- [`2026-06-30-auv-core-x5-post-x4-third-donor-graduation-review.md`](2026-06-30-auv-core-x5-post-x4-third-donor-graduation-review.md)
- [`2026-07-04-auv-s-line-graduation-review.md`](2026-07-04-auv-s-line-graduation-review.md) — S-line state-of-lane audit (`hold` substrate; S1 narrow graduation candidate)
- [`2026-07-05-auv-s1-bounded-contract-graduation-review.md`](2026-07-05-auv-s1-bounded-contract-graduation-review.md) — S1 bounded artifact/wire/IO graduation review (`60214d2`; frame + timeline only)

#### matrix (1)

- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)

#### note (15)

- [`2026-06-27-auv-core-b1-json-file-helper-extraction.md`](2026-06-27-auv-core-b1-json-file-helper-extraction.md)
- [`2026-06-27-auv-core-b2-dual-backend-query-compare-helper-extraction.md`](2026-06-27-auv-core-b2-dual-backend-query-compare-helper-extraction.md)
- [`2026-06-27-auv-core-query-readiness-helper-extraction-closeout.md`](2026-06-27-auv-core-query-readiness-helper-extraction-closeout.md)
- [`2026-06-27-auv-core-query-readiness-helper-extraction.md`](2026-06-27-auv-core-query-readiness-helper-extraction.md)
- [`2026-06-27-auv-core-spatial-result-consumption-admission-table.md`](2026-06-27-auv-core-spatial-result-consumption-admission-table.md)
- [`2026-06-27-auv-core-spatial-result-consumption-pattern.md`](2026-06-27-auv-core-spatial-result-consumption-pattern.md)
- [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- [`2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md`](2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md)
- [`2026-06-30-auv-core-a5a-prep-metric-partial-cross-donor-mapping.md`](2026-06-30-auv-core-a5a-prep-metric-partial-cross-donor-mapping.md)
- [`2026-06-30-auv-core-a5b-query-d1-query-backend-label-contract.md`](2026-06-30-auv-core-a5b-query-d1-query-backend-label-contract.md)
- [`2026-06-30-auv-core-a6-row-70-split-owner-decision.md`](2026-06-30-auv-core-a6-row-70-split-owner-decision.md)
- [`2026-06-30-auv-core-a7-extraction-boundary-owner-pause-checkpoint.md`](2026-06-30-auv-core-a7-extraction-boundary-owner-pause-checkpoint.md)
- [`2026-06-30-auv-core-c2-prep-admission-dispatch-read-side-vocabulary-alignment.md`](2026-06-30-auv-core-c2-prep-admission-dispatch-read-side-vocabulary-alignment.md)
- [`2026-06-30-auv-core-c3-d2-verification-outcome-read-side-projection.md`](2026-06-30-auv-core-c3-d2-verification-outcome-read-side-projection.md)
- [`2026-06-30-auv-core-c3-post-action-verification-outcome-boundary.md`](2026-06-30-auv-core-c3-post-action-verification-outcome-boundary.md)
- [`2026-07-05-auv-core-action-seam-audit-handoff.md`](2026-07-05-auv-core-action-seam-audit-handoff.md) — Core action seam read-only audit (L8a plan vs L8b reconciled effective; Slice 2 locks to L8b)
- [`2026-07-05-auv-core-action-seam-l8b-reconnect-handoff.md`](2026-07-05-auv-core-action-seam-l8b-reconnect-handoff.md) — L8b reconciled effective decision + `plan_delivery_mismatch` hard acceptance
- [`2026-07-05-auv-core-action-transition-lineage-read-handoff.md`](2026-07-05-auv-core-action-transition-lineage-read-handoff.md) — `ActionTransitionLineage` read-side projection (L8b effective, L8a comparator)
- [`2026-07-05-auv-core-l8-closeout-review.md`](2026-07-05-auv-core-l8-closeout-review.md) — L8 closeout: producer/read-model/compatibility/drift verdict (`close_for_core_seam_surface_gap_only`)
- [`2026-07-05-auv-core-l9-inspect-surface-handoff.md`](2026-07-05-auv-core-l9-inspect-surface-handoff.md) — L9 viewer surface for `action_transition_lineage` (mismatch, partial, verification)
- [`2026-07-06-auv-core-l9-r1-inspect-surface-closeout-handoff.md`](2026-07-06-auv-core-l9-r1-inspect-surface-closeout-handoff.md) — L9-R1 ATL consumption discipline (issue hard table, hint secondary, CLI seam-first)
- [`2026-07-06-auv-core-l9-r1-inspect-surface-closeout-landed.md`](2026-07-06-auv-core-l9-r1-inspect-surface-closeout-landed.md) — L9-R1 landed (merge gate G1–G7)
- [`2026-07-05-auv-core-app-command-pack-gate.md`](2026-07-05-auv-core-app-command-pack-gate.md) — App Command Pack entry gate (post L8+L9)
- [`2026-07-05-auv-core-surface-memory-lane-discipline.md`](2026-07-05-auv-core-surface-memory-lane-discipline.md) — S/Surface Memory independent lane discipline

#### review (5)

- [`2026-06-27-auv-core-a-query-readiness-falsifier-review.md`](2026-06-27-auv-core-a-query-readiness-falsifier-review.md)
- [`2026-06-28-auv-core-a2-full-chain-falsifier-review.md`](2026-06-28-auv-core-a2-full-chain-falsifier-review.md)
- [`2026-06-28-auv-core-c1-action-attempt-admission-review.md`](2026-06-28-auv-core-c1-action-attempt-admission-review.md)
- [`2026-06-30-auv-core-a5b-prep-backend-label-discipline-split-review.md`](2026-06-30-auv-core-a5b-prep-backend-label-discipline-split-review.md)
- [`2026-06-30-auv-core-d1-action-lease-ownership-boundary-review.md`](2026-06-30-auv-core-d1-action-lease-ownership-boundary-review.md)

#### roadmap (1)

- [`2026-06-13-auv-core-lane-roadmap.md`](2026-06-13-auv-core-lane-roadmap.md)

### `core/invoke-cli` — Active

invoke 路由、CLI handler、catalog

#### design (3)

- [`2026-06-11-auv-cli-invoke-driver-console-design.md`](2026-06-11-auv-cli-invoke-driver-console-design.md)
- [`2026-06-17-auv-cli-invoke-metadata-routing-design.md`](2026-06-17-auv-cli-invoke-metadata-routing-design.md)
- [`2026-07-03-cli-output-contract-design.md`](2026-07-03-cli-output-contract-design.md)

#### handoff (2)

- [`2026-06-14-c1-invoke-registry-handoff.md`](2026-06-14-c1-invoke-registry-handoff.md)
- [`2026-06-18-invoke-direct-command-implementations-handoff.md`](2026-06-18-invoke-direct-command-implementations-handoff.md)

#### implementation (1)

- [`2026-06-18-invoke-direct-command-implementations-plan.md`](2026-06-18-invoke-direct-command-implementations-plan.md)

#### implementation-plan (3)

- [`2026-06-11-auv-cli-invoke-driver-console-implementation-plan.md`](2026-06-11-auv-cli-invoke-driver-console-implementation-plan.md)
- [`2026-06-17-auv-cli-invoke-routing-implementation-plan.md`](2026-06-17-auv-cli-invoke-routing-implementation-plan.md)
- [`2026-07-03-cli-output-contract-implementation-plan.md`](2026-07-03-cli-output-contract-implementation-plan.md)

#### note (2)

- [`2026-06-10-auv-cli-invoke-catalog-removal.md`](2026-06-10-auv-cli-invoke-catalog-removal.md)
- [`2026-06-18-auv-cli-invoke-traced-wrapper-runtime-exit.md`](2026-06-18-auv-cli-invoke-traced-wrapper-runtime-exit.md)

#### plan (3)

- [`2026-06-13-auv-cli-invoke-handler-first-plan.md`](2026-06-13-auv-cli-invoke-handler-first-plan.md)
- [`2026-06-17-invoke-command-handler-binding-plan.md`](2026-06-17-invoke-command-handler-binding-plan.md)
- [`2026-06-18-auv-cli-invoke-traced-wrapper-runtime-exit-plan.md`](2026-06-18-auv-cli-invoke-traced-wrapper-runtime-exit-plan.md)

### `core/scenebridge` — Active (independent lane)

cross-app scene identity / grounding → command targets; not session API rhythm

#### design (1)

- [`2026-06-30-auv-scenebridge-a1-design-charter.md`](2026-06-30-auv-scenebridge-a1-design-charter.md)

#### review (2)

- [`2026-06-30-auv-scenebridge-a2-boundary-decision-review.md`](2026-06-30-auv-scenebridge-a2-boundary-decision-review.md)
- [`2026-06-30-auv-scenebridge-a3-prototype-boundary-review.md`](2026-06-30-auv-scenebridge-a3-prototype-boundary-review.md)

#### evidence-pack (1)

- [`2026-06-30-auv-scenebridge-a2-netease-sidebar-evidence-pack.md`](2026-06-30-auv-scenebridge-a2-netease-sidebar-evidence-pack.md)

#### handoff (1)

- [`2026-06-30-auv-scenebridge-a3-implementation-handoff.md`](2026-06-30-auv-scenebridge-a3-implementation-handoff.md)

#### closure (2)

- [`2026-06-30-auv-scenebridge-a4-closure.md`](2026-06-30-auv-scenebridge-a4-closure.md)
- [`2026-06-30-auv-scenebridge-a6-live-evidence-closure.md`](2026-06-30-auv-scenebridge-a6-live-evidence-closure.md)

#### charter (1)

- [`2026-06-30-auv-scenebridge-a5-inspect-identity-proof-charter.md`](2026-06-30-auv-scenebridge-a5-inspect-identity-proof-charter.md)

### `core/api-mcp` — Active

session API、proto、MCP 前端

#### design (2)

- [`2026-06-18-core-realtime-session-substrate-slice-design.md`](2026-06-18-core-realtime-session-substrate-slice-design.md)
- [`2026-06-30-auv-api-p4-session-proto-server-seam-design.md`](2026-06-30-auv-api-p4-session-proto-server-seam-design.md)

#### evidence-pack (1)

- [`2026-06-11-mcp-read-chain-evidence-pack.md`](2026-06-11-mcp-read-chain-evidence-pack.md)

#### handoff (7)

- [`2026-06-30-auv-api-s1-subprocess-smoke-handoff.md`](2026-06-30-auv-api-s1-subprocess-smoke-handoff.md)
- [`2026-06-14-c4-mcp-frontend-handoff.md`](2026-06-14-c4-mcp-frontend-handoff.md)
- [`2026-06-30-auv-api-p11-summary-durability-handoff.md`](2026-06-30-auv-api-p11-summary-durability-handoff.md)
- [`2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md`](2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md)
- [`2026-06-30-auv-api-p13-external-client-smoke-handoff.md`](2026-06-30-auv-api-p13-external-client-smoke-handoff.md)
- [`2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md`](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md)
- [`2026-06-30-auv-api-r2-invoke-operation-result-handoff.md`](2026-06-30-auv-api-r2-invoke-operation-result-handoff.md)

#### note (4)

- [`2026-06-30-auv-api-l1-session-api-operator-guide.md`](2026-06-30-auv-api-l1-session-api-operator-guide.md)
- [`2026-06-10-stateful-session-daemon-js-repl-v0.md`](2026-06-10-stateful-session-daemon-js-repl-v0.md)
- [`2026-06-11-mcp-frontend-surface-v0.md`](2026-06-11-mcp-frontend-surface-v0.md)
- [`2026-06-18-core-realtime-session-substrate-v0.md`](2026-06-18-core-realtime-session-substrate-v0.md)

#### review (5)

- [`2026-06-30-auv-api-p1-session-proto-boundary-review.md`](2026-06-30-auv-api-p1-session-proto-boundary-review.md)
- [`2026-06-30-auv-api-p12-identity-role-semantics-closeout.md`](2026-06-30-auv-api-p12-identity-role-semantics-closeout.md)
- [`2026-06-30-auv-api-r1-invoke-operation-result-persistence-decision-review.md`](2026-06-30-auv-api-r1-invoke-operation-result-persistence-decision-review.md)
- [`2026-06-30-auv-api-r2b-invoke-surface-parity-decision-review.md`](2026-06-30-auv-api-r2b-invoke-surface-parity-decision-review.md) <!-- review + freeze -->
- [`2026-06-30-auv-api-r2c-known-limits-plumbing-decision-review.md`](2026-06-30-auv-api-r2c-known-limits-plumbing-decision-review.md) <!-- review + freeze -->

### `core/inspect-trace` — Active

run 录制、inspect viewer、trace

#### design (2)

- [`2026-05-19-trace-run-inspect-design.md`](2026-05-19-trace-run-inspect-design.md)
- [`2026-05-21-live-inspect-recording-design.md`](2026-05-21-live-inspect-recording-design.md)

#### implementation-plan (1)

- [`2026-05-19-trace-run-inspect-implementation-plan.md`](2026-05-19-trace-run-inspect-implementation-plan.md)

#### note (2)

- [`2026-06-10-auv-tracing-driver-runtime-recording-split.md`](2026-06-10-auv-tracing-driver-runtime-recording-split.md)
- [`2026-06-18-recording-root-shim-removal-spec.md`](2026-06-18-recording-root-shim-removal-spec.md)

#### plan (1)

- [`2026-06-18-recording-root-shim-removal-plan.md`](2026-06-18-recording-root-shim-removal-plan.md)

### `core/driver-macos` — Active

auv-driver、macOS 输入/窗口/权限

#### design (12)

- [`2026-05-20-macos-driver-namespace-after-window-screen-design.md`](2026-05-20-macos-driver-namespace-after-window-screen-design.md)
- [`2026-05-20-macos-osascript-backend-design.md`](2026-05-20-macos-osascript-backend-design.md)
- [`2026-05-20-macos-swift-bridge-migration-design.md`](2026-05-20-macos-swift-bridge-migration-design.md)
- [`2026-05-20-window-screen-ocr-click-design.md`](2026-05-20-window-screen-ocr-click-design.md)
- [`2026-05-25-driver-platform-api-crates-design.md`](2026-05-25-driver-platform-api-crates-design.md)
- [`2026-05-26-macos-capture-fast-path-design.md`](2026-05-26-macos-capture-fast-path-design.md)
- [`2026-05-26-macos-no-steal-input-design.md`](2026-05-26-macos-no-steal-input-design.md)
- [`2026-06-04-auv-driver-command-bridge-design.md`](2026-06-04-auv-driver-command-bridge-design.md)
- [`2026-06-04-media-macos-now-playing-design.md`](2026-06-04-media-macos-now-playing-design.md)
- [`2026-06-05-auv-driver-foreground-input-design.md`](2026-06-05-auv-driver-foreground-input-design.md)
- [`2026-06-05-auv-driver-permission-probe-design.md`](2026-06-05-auv-driver-permission-probe-design.md)
- [`2026-06-05-window-management-api-design.md`](2026-06-05-window-management-api-design.md)

#### handoff (1)

- [`2026-05-26-macos-driver-legacy-typed-handoff.md`](2026-05-26-macos-driver-legacy-typed-handoff.md)

#### implementation (1)

- [`2026-06-18-auv-driver-windows-v0-implementation.md`](2026-06-18-auv-driver-windows-v0-implementation.md)

#### implementation-plan (2)

- [`2026-05-20-window-screen-ocr-click-implementation-plan.md`](2026-05-20-window-screen-ocr-click-implementation-plan.md)
- [`2026-06-16-auv-tracing-driver-extraction-implementation-plan.md`](2026-06-16-auv-tracing-driver-extraction-implementation-plan.md)

#### matrix (1)

- [`2026-06-04-auv-driver-command-migration-matrix.md`](2026-06-04-auv-driver-command-migration-matrix.md)

#### note (2)

- [`2026-06-05-window-management-api-v0.md`](2026-06-05-window-management-api-v0.md)
- [`2026-06-11-windows-driver-feasibility-and-delivery-paths.md`](2026-06-11-windows-driver-feasibility-and-delivery-paths.md)

#### roadmap (1)

- [`2026-05-26-driver-capture-input-interaction-roadmap.md`](2026-05-26-driver-capture-input-interaction-roadmap.md)

### `core/view-parser` — Active

view-parser IR 与 inspect 消费

#### note (14)

- [`2026-05-29-view-parser-anchor-reacquisition-v0.md`](2026-05-29-view-parser-anchor-reacquisition-v0.md)
- [`2026-05-29-view-parser-cli-rendering-v0.md`](2026-05-29-view-parser-cli-rendering-v0.md)
- [`2026-05-29-view-parser-contract-bridge-v0.md`](2026-05-29-view-parser-contract-bridge-v0.md)
- [`2026-05-29-view-parser-diagnostic-policy-v0.md`](2026-05-29-view-parser-diagnostic-policy-v0.md)
- [`2026-05-29-view-parser-example-placement-v0.md`](2026-05-29-view-parser-example-placement-v0.md)
- [`2026-05-29-view-parser-inspect-viewer-v0.md`](2026-05-29-view-parser-inspect-viewer-v0.md)
- [`2026-05-29-view-parser-ir-shapes-v0.md`](2026-05-29-view-parser-ir-shapes-v0.md)
- [`2026-05-29-view-parser-layer-contracts-v0.md`](2026-05-29-view-parser-layer-contracts-v0.md)
- [`2026-05-29-view-parser-merge-fixtures-v0.md`](2026-05-29-view-parser-merge-fixtures-v0.md)
- [`2026-05-29-view-parser-scroll-loop-v0.md`](2026-05-29-view-parser-scroll-loop-v0.md)
- [`2026-05-29-view-parser-spec-vs-pr9-divergence-triage.md`](2026-05-29-view-parser-spec-vs-pr9-divergence-triage.md)
- [`2026-05-29-view-parser-trace-layout-v0.md`](2026-05-29-view-parser-trace-layout-v0.md)
- [`2026-05-29-view-parser-v0-overview.md`](2026-05-29-view-parser-v0-overview.md)
- [`2026-05-29-view-parser-view-memory-v0.md`](2026-05-29-view-parser-view-memory-v0.md)

#### scan line (S) — note entry (26)

Single-viewport **2D temporal scan** / **S-line observation read-model v1 (hermetic)**; complements
[`scroll-scan` design](2026-05-21-scroll-scan-design.md) page-loop evidence. S1–S6b-1 landed in `crates/auv-scan`; S7 invoke frame producer landed in `crates/auv-cli-invoke` (`scan.frame`); whole-line substrate graduation **`hold`** — see [graduation review](2026-07-04-auv-s-line-graduation-review.md); S1 bounded artifact contracts documented — see [bounded contract review](2026-07-05-auv-s1-bounded-contract-graduation-review.md).

- [`2026-07-02-auv-scan-s0-charter.md`](2026-07-02-auv-scan-s0-charter.md) — design charter
- [`2026-07-02-auv-scan-s1-temporal-core-plan.md`](2026-07-02-auv-scan-s1-temporal-core-plan.md) — implementation plan (step 1 landed)
- [`2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md`](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md) — slice 1 handoff (`crates/auv-scan`, `scan-frame-v0`)
- [`2026-07-02-auv-scan-s1-s2-s4-producer-read-temporal-plan.md`](2026-07-02-auv-scan-s1-s2-s4-producer-read-temporal-plan.md) — S1-2/3/4 engineering plan (producer → reader → temporal outline)
- [`2026-07-02-auv-scan-s1-s2-s4-gan-spec.md`](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md) — S1-2/3/4 GAN spec (sprints, rubric, risks; producer Option A in-crate)
- [`2026-07-02-auv-scan-s1-slice2-producer-handoff.md`](2026-07-02-auv-scan-s1-slice2-producer-handoff.md) — slice 2 handoff (producer wiring)
- [`2026-07-02-auv-scan-s1-slice3-read-side-handoff.md`](2026-07-02-auv-scan-s1-slice3-read-side-handoff.md) — slice 3 handoff (crate-local reader)
- [`2026-07-02-auv-scan-s1-s4a-multi-frame-handoff.md`](2026-07-02-auv-scan-s1-s4a-multi-frame-handoff.md) — S1-4a handoff (two-frame artifacts + replay)
- [`2026-07-03-auv-scan-s1-s4b-motion-timeline-handoff.md`](2026-07-03-auv-scan-s1-s4b-motion-timeline-handoff.md) — S1-4b two-frame adjacent timeline (directory-level `scan-timeline-v0` wire; bounded contract per [2026-07-05 review](2026-07-05-auv-s1-bounded-contract-graduation-review.md))
- [`2026-07-03-auv-scan-s4-anchor-lifecycle-charter.md`](2026-07-03-auv-scan-s4-anchor-lifecycle-charter.md) — S4 lifecycle charter (evidence-first; docs-only)
- [`2026-07-02-auv-scan-s4-lifecycle-evaluator-handoff.md`](2026-07-02-auv-scan-s4-lifecycle-evaluator-handoff.md) — S4 evaluator v1 handoff (motion/association/coverage read-models)
- [`2026-07-03-auv-scan-s5-scene-state-charter.md`](2026-07-03-auv-scan-s5-scene-state-charter.md) — S5 scene state charter (L2 draft answers; docs-only)
- [`2026-07-03-auv-scan-s5-scene-state-handoff.md`](2026-07-03-auv-scan-s5-scene-state-handoff.md) — S5a scene product builder handoff
- [`2026-07-03-auv-scan-s6a-scene-state-inspect-handoff.md`](2026-07-03-auv-scan-s6a-scene-state-inspect-handoff.md) — S6a L3 inspect consumption handoff (memory-only projection)
- [`2026-07-03-auv-scan-s6b-scene-state-run-read-handoff.md`](2026-07-03-auv-scan-s6b-scene-state-run-read-handoff.md) — S6b-1 run-read text bridge (`inspect_run` + provisional `scan-scene-state-input-v0` staging)
- [`2026-07-03-s-line-streaming-observation-substrate.md`](2026-07-03-s-line-streaming-observation-substrate.md) — S0-S6 direction, A/B/S/M/G lane boundaries, and first acceptance gates
- [`2026-07-04-auv-s-line-graduation-review.md`](2026-07-04-auv-s-line-graduation-review.md) — S-line graduation review / state-of-lane audit
- [`2026-07-05-auv-s1-bounded-contract-graduation-review.md`](2026-07-05-auv-s1-bounded-contract-graduation-review.md) — S1 bounded contract graduation review (artifact/wire/IO only)
- [`2026-07-06-auv-scan-s7-invoke-frame-producer-handoff.md`](2026-07-06-auv-scan-s7-invoke-frame-producer-handoff.md) — S7 invoke `scan.frame` fixture producer (runtime artifact bridge; not lane graduation)
- [`2026-07-07-auv-scan-s8a-coverage-wire-handoff.md`](2026-07-07-auv-scan-s8a-coverage-wire-handoff.md) — S8a `scan-coverage-v0` crate-local wire/IO (`coverage_view_to_wire` projection only; S3 stage remains `partial`)
- [`2026-07-08-auv-scan-s8b-scene-coverage-consumer-handoff.md`](2026-07-08-auv-scan-s8b-scene-coverage-consumer-handoff.md) — S8b scene_state durable coverage consumer (`coverage_wire_to_view` inverse projection; whole-product parity; S3 substrate remains `partial`)
- [`2026-07-09-auv-scan-s8c-coverage-producer-handoff.md`](2026-07-09-auv-scan-s8c-coverage-producer-handoff.md) — S8c runtime coverage producer (`produce_coverage_from_fixture_dir` + `scan.coverage` invoke staging; S3 substrate remains `partial` until S8d inspect durable read)
- [`2026-07-10-auv-scan-s8d-inspect-coverage-handoff.md`](2026-07-10-auv-scan-s8d-inspect-coverage-handoff.md) — S8d inspect durable read (`scene_state_read` hydrates `scan-coverage-v0`; S8 fixture-first chain `landed proof`; S3 ledger substrate remains `partial`)
- [`2026-07-10-auv-scan-s9a-nframe-adjacent-timeline-handoff.md`](2026-07-10-auv-scan-s9a-nframe-adjacent-timeline-handoff.md) — S9a N-frame adjacent multi-segment timeline builder (`scan-timeline-v0` semantic revision; tracks row remains `hold`)
- [`2026-07-10-auv-scan-s9b-adjacent-tracks-wire-handoff.md`](2026-07-10-auv-scan-s9b-adjacent-tracks-wire-handoff.md) — S9b N-frame adjacent multi-segment tracks wire (`scan-tracks-v0`; tracks substrate row remains `hold`)

#### surface slam direction — roadmap entry (1)

Direction after pausing S-line implementation at S9b: build a **2D interactive
surface model** from video-stream evidence before considering 3DGS, SLAM
backends, or game telemetry as main lines.

- [`2026-07-05-auv-surface-slam-direction.md`](2026-07-05-auv-surface-slam-direction.md) — Surface SLAM direction (`2D video -> stable interactive surface model`; YOLO as one evidence channel; spatial grounding deferred)

### `core/recognition` — Active

RecognitionResult、detector 边界

#### evidence (1)

- [`2026-06-05-recognition-evidence-boundary-v0.md`](2026-06-05-recognition-evidence-boundary-v0.md)

#### handoff (1)

- [`2026-05-25-recognition-consumption-handoff.md`](2026-05-25-recognition-consumption-handoff.md)

#### note (2)

- [`2026-06-05-detector-manifest-recognitionresult-mapping-v0.md`](2026-06-05-detector-manifest-recognitionresult-mapping-v0.md)
- [`2026-06-10-game-recognition-recipe-consumer-seam.md`](2026-06-10-game-recognition-recipe-consumer-seam.md)

#### research (1)

- [`2026-05-24-maa-recognition-pipeline-research.md`](2026-05-24-maa-recognition-pipeline-research.md)

### `vertical/minecraft` — Paused vertical

Minecraft 3D spatial 探针；MC20 pause decision 已落地

#### closure (9)

- [`2026-06-16-minecraft-live-mc2-mc4-closure-plan.md`](2026-06-16-minecraft-live-mc2-mc4-closure-plan.md)
- [`2026-06-24-minecraft-mc6-dual-gate-closure-reference.md`](2026-06-24-minecraft-mc6-dual-gate-closure-reference.md)
- [`2026-06-26-minecraft-mc7-d12-normalized-result-artifacts-read-side-closure.md`](2026-06-26-minecraft-mc7-d12-normalized-result-artifacts-read-side-closure.md)
- [`2026-06-26-minecraft-mc7-live-real-source-closure-reference.md`](2026-06-26-minecraft-mc7-live-real-source-closure-reference.md)
- [`2026-06-26-minecraft-mc8-d1-d3-remote-adapter-closure.md`](2026-06-26-minecraft-mc8-d1-d3-remote-adapter-closure.md)
- [`2026-06-27-minecraft-mc9-d3-live-provider-status-and-fetch-closure.md`](2026-06-27-minecraft-mc9-d3-live-provider-status-and-fetch-closure.md)
- [`2026-06-27-minecraft-mc9-d3-real-provider-status-closure.md`](2026-06-27-minecraft-mc9-d3-real-provider-status-closure.md)
- [`2026-06-27-minecraft-mc9-d4-real-provider-artifact-fetch-closure.md`](2026-06-27-minecraft-mc9-d4-real-provider-artifact-fetch-closure.md)
- [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md)

#### design (18)

- [`2026-06-18-minecraft-mc6-spatial-dataset-measurement-design.md`](2026-06-18-minecraft-mc6-spatial-dataset-measurement-design.md)
- [`2026-06-18-minecraft-mc7-offline-3dgs-inspect-artifact-design.md`](2026-06-18-minecraft-mc7-offline-3dgs-inspect-artifact-design.md)
- [`2026-06-25-minecraft-mc7-d3-training-package-design.md`](2026-06-25-minecraft-mc7-d3-training-package-design.md)
- [`2026-06-25-minecraft-mc7-d5-training-launch-prep-design.md`](2026-06-25-minecraft-mc7-d5-training-launch-prep-design.md)
- [`2026-06-25-minecraft-mc7-d7-training-result-collection-design.md`](2026-06-25-minecraft-mc7-d7-training-result-collection-design.md)
- [`2026-06-25-minecraft-mc7-d8-trainer-lineage-inspect-consumer-design.md`](2026-06-25-minecraft-mc7-d8-trainer-lineage-inspect-consumer-design.md)
- [`2026-06-27-minecraft-mc10-result-semantic-validation-design.md`](2026-06-27-minecraft-mc10-result-semantic-validation-design.md)
- [`2026-06-27-minecraft-mc11-semantic-read-side-inspect-consumer-design.md`](2026-06-27-minecraft-mc11-semantic-read-side-inspect-consumer-design.md)
- [`2026-06-27-minecraft-mc12-spatial-query-contract-design.md`](2026-06-27-minecraft-mc12-spatial-query-contract-design.md)
- [`2026-06-27-minecraft-mc13-spatial-query-read-side-inspect-consumer-design.md`](2026-06-27-minecraft-mc13-spatial-query-read-side-inspect-consumer-design.md)
- [`2026-06-27-minecraft-mc14-spatial-query-action-facing-consumer-design.md`](2026-06-27-minecraft-mc14-spatial-query-action-facing-consumer-design.md)
- [`2026-06-27-minecraft-mc15-checkpoint-native-query-provider-seam-design.md`](2026-06-27-minecraft-mc15-checkpoint-native-query-provider-seam-design.md)
- [`2026-06-27-minecraft-mc17-d2-quality-baseline-report-design.md`](2026-06-27-minecraft-mc17-d2-quality-baseline-report-design.md)
- [`2026-06-27-minecraft-mc17-holdout-render-quality-design.md`](2026-06-27-minecraft-mc17-holdout-render-quality-design.md)
- [`2026-06-27-minecraft-mc18-closed-scene-toy-provider-design.md`](2026-06-27-minecraft-mc18-closed-scene-toy-provider-design.md)
- [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md`](2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md)
- [`2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`](2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md)
- [`2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md`](2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md)

#### design-closure (1)

- [`2026-06-15-minecraft-mc5-graduation-design-closure.md`](2026-06-15-minecraft-mc5-graduation-design-closure.md)

#### evidence (2)

- [`2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md)
- [`2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md)

#### handoff (3)

- [`2026-06-15-minecraft-mc2-closure-mc3-handoff.md`](2026-06-15-minecraft-mc2-closure-mc3-handoff.md)
- [`2026-06-15-minecraft-series-handoff.md`](2026-06-15-minecraft-series-handoff.md)
- [`2026-06-24-minecraft-mc6-to-mc7-handoff.md`](2026-06-24-minecraft-mc6-to-mc7-handoff.md)

#### live-closure (14)

- [`2026-06-26-minecraft-mc8-d4-adapter-live-closure.md`](2026-06-26-minecraft-mc8-d4-adapter-live-closure.md)
- [`2026-06-27-minecraft-mc10-semantic-validation-live-closure.md`](2026-06-27-minecraft-mc10-semantic-validation-live-closure.md)
- [`2026-06-27-minecraft-mc12-spatial-query-live-closure.md`](2026-06-27-minecraft-mc12-spatial-query-live-closure.md)
- [`2026-06-27-minecraft-mc13-spatial-query-read-side-live-closure.md`](2026-06-27-minecraft-mc13-spatial-query-read-side-live-closure.md)
- [`2026-06-27-minecraft-mc14-spatial-query-action-facing-live-closure.md`](2026-06-27-minecraft-mc14-spatial-query-action-facing-live-closure.md)
- [`2026-06-27-minecraft-mc15-checkpoint-native-query-provider-live-closure.md`](2026-06-27-minecraft-mc15-checkpoint-native-query-provider-live-closure.md)
- [`2026-06-27-minecraft-mc16-holdout-preview-render-inspect-live-closure.md`](2026-06-27-minecraft-mc16-holdout-preview-render-inspect-live-closure.md)
- [`2026-06-27-minecraft-mc17-d2-quality-baseline-live-closure.md`](2026-06-27-minecraft-mc17-d2-quality-baseline-live-closure.md)
- [`2026-06-27-minecraft-mc17-d3-quality-verdict-live-closure.md`](2026-06-27-minecraft-mc17-d3-quality-verdict-live-closure.md)
- [`2026-06-27-minecraft-mc18-closed-scene-toy-provider-live-closure.md`](2026-06-27-minecraft-mc18-closed-scene-toy-provider-live-closure.md)
- [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-live-closure.md`](2026-06-27-minecraft-mc19-query-to-live-click-wiring-live-closure.md)
- [`2026-06-27-minecraft-mc9-d5-real-provider-fetch-live-closure.md`](2026-06-27-minecraft-mc9-d5-real-provider-fetch-live-closure.md)
- [`2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`](2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md)
- [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md)

#### note (6)

- [`2026-06-14-auv-3d-minecraft-spatial-skill-p0.md`](2026-06-14-auv-3d-minecraft-spatial-skill-p0.md)
- [`2026-06-18-minecraft-mc6-run-preparation-exploration.md`](2026-06-18-minecraft-mc6-run-preparation-exploration.md)
- [`2026-06-24-minecraft-mc6-canonical-clean-rebuild-fail-record.md`](2026-06-24-minecraft-mc6-canonical-clean-rebuild-fail-record.md)
- [`2026-06-24-minecraft-mc6-canonical-staging-artifact.md`](2026-06-24-minecraft-mc6-canonical-staging-artifact.md)
- [`2026-06-24-minecraft-mc7-d2-accepted-only-scene-packet-inspect-reference.md`](2026-06-24-minecraft-mc7-d2-accepted-only-scene-packet-inspect-reference.md)
- [`2026-06-30-minecraft-mc20-final-closeout-pause-decision.md`](2026-06-30-minecraft-mc20-final-closeout-pause-decision.md)

#### review (1)

- [`2026-06-27-minecraft-mc16-holdout-preview-render-inspect-design.md`](2026-06-27-minecraft-mc16-holdout-preview-render-inspect-design.md)

#### verdict (4)

- [`2026-06-19-minecraft-mc6-texture-sweep-gate-verdict.md`](2026-06-19-minecraft-mc6-texture-sweep-gate-verdict.md)
- [`2026-06-26-minecraft-mc8-closure-gate-verdict.md`](2026-06-26-minecraft-mc8-closure-gate-verdict.md)
- [`2026-06-27-minecraft-mc17-d3-quality-verdict-design.md`](2026-06-27-minecraft-mc17-d3-quality-verdict-design.md)
- [`2026-06-27-minecraft-mc9-closure-gate-verdict.md`](2026-06-27-minecraft-mc9-closure-gate-verdict.md)

### `vertical/osu` — Graduation candidate

osu benchmark；G-series 需单独 owner 批准

#### design (2)

- [`2026-06-27-auv-second-vertical-consumption-probe-osu-design.md`](2026-06-27-auv-second-vertical-consumption-probe-osu-design.md)
- [`2026-06-28-osu-visual-truth-query-wired-live-action-design.md`](2026-06-28-osu-visual-truth-query-wired-live-action-design.md)

#### evidence (6)

- [`2026-06-13-osu-benchmark-p5-latency-evidence.md`](2026-06-13-osu-benchmark-p5-latency-evidence.md)
- [`2026-06-13-osu-benchmark-p6-dataset-evidence.md`](2026-06-13-osu-benchmark-p6-dataset-evidence.md)
- [`2026-06-13-osu-benchmark-p7-detection-eval-evidence.md`](2026-06-13-osu-benchmark-p7-detection-eval-evidence.md)
- [`2026-06-13-osu-benchmark-p8-bounded-demo-evidence.md`](2026-06-13-osu-benchmark-p8-bounded-demo-evidence.md)
- [`2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md`](2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md)
- [`2026-06-28-osu-wq1-witness-quality-evidence-design.md`](2026-06-28-osu-wq1-witness-quality-evidence-design.md)

#### live-closure (1)

- [`2026-06-28-osu-visual-truth-query-wired-live-action-live-closure.md`](2026-06-28-osu-visual-truth-query-wired-live-action-live-closure.md)

#### roadmap (1)

- [`2026-06-13-osu-benchmark-p4-plus-roadmap.md`](2026-06-13-osu-benchmark-p4-plus-roadmap.md)

### `vertical/balatro` — Graduation candidate

第三垂直消费探针

#### closure (1)

- [`2026-06-30-auv-core-x4-balatro-witness-lineage-closure-design.md`](2026-06-30-auv-core-x4-balatro-witness-lineage-closure-design.md)

#### design (1)

- [`2026-06-29-auv-core-x2-balatro-consumption-probe-design.md`](2026-06-29-auv-core-x2-balatro-consumption-probe-design.md)

#### evidence (2)

- [`2026-06-29-auv-core-x2-balatro-consumption-probe-evidence.md`](2026-06-29-auv-core-x2-balatro-consumption-probe-evidence.md)
- [`2026-06-30-auv-core-x4-balatro-witness-lineage-closure-evidence.md`](2026-06-30-auv-core-x4-balatro-witness-lineage-closure-evidence.md)

### `vertical/netease-music` — Product crate

网易云音乐 app-local 命令

#### design (4)

- [`2026-05-28-view-parser-ir-netease-playlist-example-design.md`](2026-05-28-view-parser-ir-netease-playlist-example-design.md)
- [`2026-05-29-netease-music-cli-design.md`](2026-05-29-netease-music-cli-design.md)
- [`2026-06-03-netease-cloud-music-domain-api-design.md`](2026-06-03-netease-cloud-music-domain-api-design.md)
- [`2026-07-03-cli-output-contract-design.md`](2026-07-03-cli-output-contract-design.md)

#### handoff (2)

- [`2026-07-05-auv-netease-music-acp-1-handoff.md`](2026-07-05-auv-netease-music-acp-1-handoff.md)
- [`2026-07-05-auv-netease-music-acp-2-handoff.md`](2026-07-05-auv-netease-music-acp-2-handoff.md)

#### implementation-plan (3)

- [`2026-05-28-view-parser-ir-netease-playlist-example-implementation-plan.md`](2026-05-28-view-parser-ir-netease-playlist-example-implementation-plan.md)
- [`2026-05-30-netease-music-cli-playlist-implementation-plan.md`](2026-05-30-netease-music-cli-playlist-implementation-plan.md)
- [`2026-07-03-cli-output-contract-implementation-plan.md`](2026-07-03-cli-output-contract-implementation-plan.md)

#### note (4)

- [`2026-05-19-netease-cloud-music-fixed-layout-baseline.md`](2026-05-19-netease-cloud-music-fixed-layout-baseline.md)
- [`2026-05-20-netease-v2-candidate-pass.md`](2026-05-20-netease-v2-candidate-pass.md)
- [`2026-05-29-netease-playlist-item-parsing-v0.md`](2026-05-29-netease-playlist-item-parsing-v0.md)
- [`2026-05-29-netease-sidebar-region-detection-v0.md`](2026-05-29-netease-sidebar-region-detection-v0.md)

### `vertical/qqmusic` — Historical evidence

QQ 音乐早期探针与 GLM 证据

#### evidence (2)

- [`2026-05-14-glm-air-qqmusic-search-evidence.md`](2026-05-14-glm-air-qqmusic-search-evidence.md)
- [`2026-05-15-glm-air-qqmusic-playback-evidence.md`](2026-05-15-glm-air-qqmusic-playback-evidence.md)

#### matrix (2)

- [`2026-05-15-qqmusic-playback-case-matrix.md`](2026-05-15-qqmusic-playback-case-matrix.md)
- [`2026-05-16-qqmusic-row-fallback-case-matrix.md`](2026-05-16-qqmusic-row-fallback-case-matrix.md)

#### note (4)

- [`2026-05-15-qqmusic-macos-capability-probe.md`](2026-05-15-qqmusic-macos-capability-probe.md)
- [`2026-05-15-qqmusic-playback-verification.md`](2026-05-15-qqmusic-playback-verification.md)
- [`2026-05-17-qqmusic-narrow-skill-coverage.md`](2026-05-17-qqmusic-narrow-skill-coverage.md)
- [`2026-05-22-qqmusic-search-candidate-shape.md`](2026-05-22-qqmusic-search-candidate-shape.md)

### `vertical/game-observe` — Observe-only

Steam/STS 等 observe-only fixture

#### closure (1)

- [`2026-06-14-c3-steam-core-lane-closure.md`](2026-06-14-c3-steam-core-lane-closure.md)

#### design (1)

- [`2026-06-09-steam-library-automation-design.md`](2026-06-09-steam-library-automation-design.md)

#### evidence (1)

- [`2026-06-10-sts-zero-ax-observe-probe-evidence.md`](2026-06-10-sts-zero-ax-observe-probe-evidence.md)

#### note (1)

- [`2026-06-06-game-slay-the-spire-observe-only-recognition-fixture-boundary.md`](2026-06-06-game-slay-the-spire-observe-only-recognition-fixture-boundary.md)

### `archive/skill-bundle-retirement` — Retired

SkillBundle / recipe 退役记录

#### design (1)

- [`2026-06-11-skill-recipe-removal-sequence-design.md`](2026-06-11-skill-recipe-removal-sequence-design.md)

#### inventory (1)

- [`2026-06-10-recipe-bundle-retirement-inventory.md`](2026-06-10-recipe-bundle-retirement-inventory.md)

#### note (3)

- [`2026-05-15-skill-contract-v0.md`](2026-05-15-skill-contract-v0.md)
- [`2026-05-17-auv-native-app-skill-tree.md`](2026-05-17-auv-native-app-skill-tree.md)
- [`2026-06-10-rust-orchestration-recipes-bundles-retirement.md`](2026-06-10-rust-orchestration-recipes-bundles-retirement.md)

### `archive/phase-history` — Historical

Phase 1–3 冻结与验收

#### acceptance (1)

- [`2026-05-22-phase-3-mainline-acceptance.md`](2026-05-22-phase-3-mainline-acceptance.md)

#### design (1)

- [`2026-05-21-phase-3-first-contract-consumer-design.md`](2026-05-21-phase-3-first-contract-consumer-design.md)

#### freeze (2)

- [`2026-05-18-phase-1-freeze.md`](2026-05-18-phase-1-freeze.md)
- [`2026-05-21-phase-2-press-presentation-freeze.md`](2026-05-21-phase-2-press-presentation-freeze.md)

#### note (1)

- [`2026-05-22-phase-3-mainline-audit.md`](2026-05-22-phase-3-mainline-audit.md)

### `archive/ax-copilot` — Archived vertical

macOS AX copilot；见 docs/archive/verticals/ax-copilot/

#### evidence-pack (1)

- [`2026-06-09-auv-macos-ax-copilot-mvp-evidence-pack.md`](2026-06-09-auv-macos-ax-copilot-mvp-evidence-pack.md)

#### note (1)

- [`2026-05-17-notes-ax-text-sample.md`](2026-05-17-notes-ax-text-sample.md)

### `misc` — Mixed

跨 lane 或尚未归入单一主题的笔记

#### closure (2)

- [`2026-05-21-repo-state-closure.md`](2026-05-21-repo-state-closure.md)
- [`2026-05-28-surface-analyze-closure.md`](2026-05-28-surface-analyze-closure.md)

#### design (7)

- [`2026-05-21-scroll-scan-design.md`](2026-05-21-scroll-scan-design.md)
- [`2026-06-02-background-scroll-policy-design.md`](2026-06-02-background-scroll-policy-design.md)
- [`2026-06-04-auv-inference-yolo-design.md`](2026-06-04-auv-inference-yolo-design.md)
- [`2026-06-04-ultralytics-inference-adapter-design.md`](2026-06-04-ultralytics-inference-adapter-design.md)
- [`2026-06-11-media-windows-now-playing-design.md`](2026-06-11-media-windows-now-playing-design.md)
- [`2026-06-11-runtime-legacy-retirement-design.md`](2026-06-11-runtime-legacy-retirement-design.md)
- [`2026-07-03-auv-qodana-operating-model.md`](2026-07-03-auv-qodana-operating-model.md)

#### evidence (1)

- [`2026-06-05-detection-evidence-manifest-v0.md`](2026-06-05-detection-evidence-manifest-v0.md)

#### handoff (4)

- [`2026-05-24-codex-handoff.md`](2026-05-24-codex-handoff.md)
- [`2026-05-28-pr8-surface-analyze-handoff.md`](2026-05-28-pr8-surface-analyze-handoff.md)
- [`2026-06-13-core-graduation-local-handoff.md`](2026-06-13-core-graduation-local-handoff.md)
- [`2026-06-14-c5-runtime-collapse-handoff.md`](2026-06-14-c5-runtime-collapse-handoff.md)

#### note (14)

- [`2026-05-13-auv-airi-desktop-reuse.md`](2026-05-13-auv-airi-desktop-reuse.md)
- [`2026-05-17-distillation-template-v0.md`](2026-05-17-distillation-template-v0.md)
- [`2026-05-18-app-probe-analyze-workflow.md`](2026-05-18-app-probe-analyze-workflow.md)
- [`2026-05-19-v2-docs-contract.md`](2026-05-19-v2-docs-contract.md)
- [`2026-05-20-cursor-warp-jitter-smoke.md`](2026-05-20-cursor-warp-jitter-smoke.md)
- [`2026-05-20-route-b-click-wrapper-smoke.md`](2026-05-20-route-b-click-wrapper-smoke.md)
- [`2026-05-21-dual-cursor-notes-demo-boundary.md`](2026-05-21-dual-cursor-notes-demo-boundary.md)
- [`2026-05-23-surface-selector-contract.md`](2026-05-23-surface-selector-contract.md)
- [`2026-05-25-auv-dream-architecture-rust-engineering.md`](2026-05-25-auv-dream-architecture-rust-engineering.md)
- [`2026-05-27-action-resolver-v0.md`](2026-05-27-action-resolver-v0.md)
- [`2026-05-28-surface-analyze-v0.md`](2026-05-28-surface-analyze-v0.md)
- [`2026-06-05-detectionset-candidate-adapter-boundary.md`](2026-06-05-detectionset-candidate-adapter-boundary.md)
- [`2026-06-11-frontend-convention-v0.md`](2026-06-11-frontend-convention-v0.md)
- [`2026-06-26-apple-music-windows-command-reference.md`](2026-06-26-apple-music-windows-command-reference.md)

#### plan (3)

- [`2026-06-14-c1-completion-plan.md`](2026-06-14-c1-completion-plan.md)
- [`2026-06-14-core-lane-short-term-plan-c1d-c3.md`](2026-06-14-core-lane-short-term-plan-c1d-c3.md)
- [`2026-06-18-auv-mc5-onward-execution-plan.md`](2026-06-18-auv-mc5-onward-execution-plan.md)

#### research (1)

- [`2026-05-25-projects-research-and-repl-api.md`](2026-05-25-projects-research-and-repl-api.md)

#### roadmap (1)

- [`2026-05-24-structured-observation-roadmap.md`](2026-05-24-structured-observation-roadmap.md)

#### setup (1)

- [`2026-05-12-auv-setup.md`](2026-05-12-auv-setup.md)

#### taxonomy (1)

- [`2026-06-10-observe-only-strategy-taxonomy-v0.md`](2026-06-10-observe-only-strategy-taxonomy-v0.md)

## Evidence 目录（附件包）

- [`evidence/2026-05-14-qqmusic-search-ocr-anchor/`](evidence/2026-05-14-qqmusic-search-ocr-anchor/)
- [`evidence/2026-05-15-qqmusic-play-visible-anchor/`](evidence/2026-05-15-qqmusic-play-visible-anchor/)
- [`evidence/2026-06-11-mcp-read-chain/`](evidence/2026-06-11-mcp-read-chain/)

## 相关目录

| 路径 | 用途 |
|---|---|
| [`docs/README.md`](../../README.md) | 文档体系总览 |
| [`docs/TERMS_AND_CONCEPTS.md`](../../TERMS_AND_CONCEPTS.md) | 共享词汇表 |
| [`docs/ai/explanations/`](../explanations/) | 教程、交互说明 |
| [`docs/design/`](../../design/) | vendored 设计系统 |
| [`docs/archive/verticals/`](../../archive/verticals/) | 已归档垂直证明 |
