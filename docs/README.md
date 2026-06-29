# AUV Documentation

AUV 文档按 **用途** 分目录，按 **lane + 文档类型** 在索引中二次归类。
canonical 项目契约仍是 [`AGENTS.md`](../AGENTS.md)。

## 目录结构

```text
docs/
├── README.md                 ← 你在这里
├── TERMS_AND_CONCEPTS.md     ← 共享词汇（改 contract 时同步更新）
├── ai/
│   ├── references/           ← durable 设计 / handoff / evidence 笔记
│   │   ├── INDEX.md          ← reference lane 索引（236 篇 + 本文件）
│   │   ├── evidence/         ← 证据包原始附件（json/png/txt）
│   │   └── YYYY-MM-DD-*.md   ← 扁平存放，避免破坏互链
│   └── explanations/         ← 教程、交互说明 HTML
├── design/                   ← vendored 设计系统 + viewer/cli mock
├── archive/verticals/        ← 已归档垂直证明（不再指导 active roadmap）
└── notes/<owner>/            ← 个人草稿（默认不提交）
```

## 写什么、放哪里

| 内容 | 落点 |
|---|---|
| 进行中的设计 / handoff / evidence | `docs/ai/references/YYYY-MM-DD-<slug>-<type>.md` |
| 教概念、walkthrough、交互 demo | `docs/ai/explanations/` |
| 新术语进入共享词汇 | `docs/TERMS_AND_CONCEPTS.md` |
| 已结束、不应再 bias roadmap 的垂直证明 | `docs/archive/verticals/<name>/` + 旧路径 tombstone |
| 个人探索、本地 log | `docs/notes/<owner>/`（需 owner 明确要求才提交） |
| UI token / viewer mock | `docs/design/` |

新增 `references` 后：在 [`ai/references/INDEX.md`](ai/references/INDEX.md) 对应 lane 补一行。

## Active roadmap vs 归档

| 类别 | 含义 | 索引 lane 前缀 |
|---|---|---|
| **Core（活跃）** | invoke、runtime、inspect、driver、view-parser | `core/*` |
| **Vertical（产品/探针）** | app-local crate 或 consumption probe | `vertical/*` |
| **Archive（历史）** | SkillBundle 退役、phase 冻结、AX copilot | `archive/*` 或 `docs/archive/` |

`AGENTS.md` 要求：`candidate-action` / macOS AX copilot **不得**作为 active product lane 扩展。

## 快速入口

| 你想… | 从这里开始 |
|---|---|
| 理解核心术语 | [`TERMS_AND_CONCEPTS.md`](TERMS_AND_CONCEPTS.md) |
| 浏览全部 reference 归类 | [`ai/references/INDEX.md`](ai/references/INDEX.md) |
| 看 core lane 路线图 | [`ai/references/2026-06-13-auv-core-lane-roadmap.md`](ai/references/2026-06-13-auv-core-lane-roadmap.md) |
| 看 invoke / CLI 设计 | [`ai/references/2026-06-11-auv-cli-invoke-driver-console-design.md`](ai/references/2026-06-11-auv-cli-invoke-driver-console-design.md) |
| 看 inspect viewer 设计 | [`ai/references/2026-05-19-trace-run-inspect-design.md`](ai/references/2026-05-19-trace-run-inspect-design.md) |
| 看设计系统 / viewer UI | [`design/README.md`](design/README.md) |
| 查 agent 写作规范 | 仓库根 [`AGENTS.md`](../AGENTS.md) |
| 查已归档 AX copilot | [`archive/verticals/ax-copilot/`](archive/verticals/ax-copilot/) |

## Reference 体量

- **236** 篇 reference markdown + **INDEX.md**（扁平存放）
- **3** 个 evidence 附件目录
- 完整归类见 [`ai/references/INDEX.md`](ai/references/INDEX.md)
