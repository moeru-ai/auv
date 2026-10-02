# RustRover MCP 工具使用清单（给 Antigravity / Gemini）

> 目标：让 agent 充分使用 RustRover MCP 的代码智能能力，而不是只拿它当 terminal 用。
> 连接：Windows 本机 `127.0.0.1:64522`，项目路径 `F:/auv`。开工前先做 Preflight。

---

## 0. Preflight（每次开工第一步）

用 `list_directory_tree` 列项目根目录。通了 = MCP 在线且项目路径正确；不通 = 按 §5 Fallback 处理，**不许空转重试超过 2 次**。

---

## 1. 代码智能（MCP 的核心价值，优先用）

| 工具 | 用途 | 何时用 |
|---|---|---|
| `search_symbol` | 按标识符片段做**语义**查找（类/方法/字段）| 找定义、找实现，**替代盲目 grep** |
| `get_symbol_info` | 取指定位置符号的信息 | 理解陌生代码、确认类型签名 |
| `analyze_calls` | 方法/函数的 Call Hierarchy | 改函数前做影响面分析 |
| `search_regex` / `search_text` | 正则/文本搜索，带坐标 | 找用法、找字符串字面量 |
| `search_file` | 按 glob 找文件 | 定位模块文件 |
| `get_project_modules` | 列项目模块及类型 | 初次了解仓库结构 |

## 2. 验证与验收（**强制走 MCP**，见 §4）

| 工具 | 用途 | 何时用 |
|---|---|---|
| `build_project` | 构建项目/指定文件，返回构建错误 | 实现后第一道门 |
| `get_file_problems` | IntelliJ inspections 查单文件错误/警告 | 验收"0 错误 0 警告"，**必须贴诊断原文** |
| `lint_files` | 批量 lint 多个文件 | 改了多个文件时 |
| `get_run_configurations` | 列可用的运行配置 | 跑测试前先看有什么配置 |
| `execute_run_configuration` | 按配置运行（含测试）| 跑 `cargo test` 等，替代手敲 terminal |

## 3. 编辑（推荐走 MCP，diff 精确）

| 工具 | 用途 | 何时用 |
|---|---|---|
| `apply_patch` | 按 Codex/unified diff 格式打补丁 | 改代码首选，比 heredoc/sed 精确 |
| `create_new_file` | 建新文件并可预填内容 | 新增模块/脚本 |
| `reformat_file` | 按 IDE 规则格式化 | 改完代码后（替代手跑 rustfmt，效果等价按项目配置）|
| `rename_refactoring` | 安全重命名符号 | 重命名函数/类型/变量，**禁止手 sed 全局替换** |

## 4. 自带 terminal / VCS（走 Antigravity 内置，不走 MCP）

| 工具 | 用途 |
|---|---|
| `execute_terminal_command` | git 操作、跑脚本、cargo 命令（无 run 配置时）|
| `git_status` / `get_repositories` | 提交前查工作区干净度（也可用 terminal `git status`）|

**Terminal 在 Windows PowerShell 下的坑**（已知）：
- 没有 `head`，用 `Select-Object -First N`。
- `execute_terminal_command` 按空格切分命令，复杂命令加 `executeInShell=true`。

## 5. 不用 / 慎用的

- **Database 整组**（`execute_sql_query` 等）：AUV Rust 工作用不到，忽略。
- `open_file_in_editor` / `get_all_open_file_paths`：编辑器状态类，headless agent 不需要。
- `Inspection Kts` 整组（`run_inspection_kts` 等）：自定义检查脚本，常规任务用不到；除非 brief 点名。
- `generate_psi_tree`：只支持 Java/Kotlin，Rust 用不上。

---

## 强制规则（MCP vs 自带的分工）

1. **凡涉及"实现/修改 Rust 代码"**：探索用 §1，编辑用 §3（`apply_patch` 优先）。
2. **凡 brief 写了"静态分析验收"**：必须用 §2 的 `build_project` + `get_file_problems`/`lint_files`，结项报告里**贴 MCP 返回的诊断原文**（0 错误也要贴"no problems found"类原文）。**不接受只用 terminal `cargo check` 糊弄。**
3. **测试**：优先 `execute_run_configuration`；无合适配置时才用 terminal `cargo test`，并注明。
4. **git/脚本/文档**：走自带 terminal，不占用 MCP。

## Fallback

MCP 不通（Preflight 失败或工具连续报错）：书面报备一次，改用自带 terminal 继续；所有"验证"类结论降级标注为"terminal-only，未经 IDE 审计"。**不许静默降级，不许无限重试。**
