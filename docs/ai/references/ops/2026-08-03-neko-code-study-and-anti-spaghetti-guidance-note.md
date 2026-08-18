# Neko 代码学习与反史山指导

状态：当前工程开发参考，不是新的公共 API，也不替代根目录 `AGENTS.md`。

这份文档记录对 Neko 最近提交和两块代码的阅读结论：

- `0ca2aef1 refactor(auv-game-*,auv-*): better structure, prepare for repo split`
- `141337f7 chore(deps,alint-config): updated alint to 0.1.5, migrated api`
- `24257d7a test: stage device run runner grpc smoke`
- `js/packages/alint-config/`
- `supported/games/auv-game-balatro/`

目标不是模仿某个人的写法，而是吸收已经落地并能验证的边界，防止新的
3DGS、游戏观测和推理代码继续堆进一个巨型入口。

## 一句话结论

好的拆分不是“多建几个文件”，而是让每个模块拥有一个可说清楚的决定：
它负责什么、输入是什么、输出是什么、失败如何表达、证据在哪里、谁可以
复用它。没有独立决定的薄包装不要为了看起来整齐而保留；有稳定契约的行为
不要继续塞进 CLI 或一个万能 helper。

## 每次开始写代码前

先问五个问题：

1. 这个行为的 owner 是谁，仓库里是否已经有可复用的 typed contract、
   parser、driver、artifact 或错误类型？
2. 这个函数是否拥有策略、不变量、错误分类、外部 IO、生命周期或真实的
   多处复用？如果只是转发、字段复制或一行映射，不要造新边界。
3. 输入、输出、版本、失败状态和证据来源是否可单独验证？未知就保留
   `unknown` / `unread` / `not_confirmed`，不要用默认值遮住缺证据。
4. 测试是否调用稳定公共行为并断言结果、artifact、trace 或用户可观察
   状态，而不是源码布局、模块名字或文件列表？
5. 这次是否只改一个责任面，并在完成后做窄测试和 diff 检查？

3DGS 当前额外遵守：

```text
capture -> observation packet -> black-box hypothesis -> candidate patch
  -> independent confirmation -> dataset/training -> quality witness
```

telemetry、world state、raycast 和矩阵只能做事后 oracle；单视角 Prompt
只能产出 hypothesis；训练退出码为 0 也不等于空间质量通过。

## 读到的有效模式

### 1. 根模块只做装配和公共出口

`supported/games/auv-game-balatro/src/lib.rs:7-32` 主要声明模块，
`34-102` 负责有意识地 re-export 公共类型和操作。业务行为不在根模块里
横向展开。

这给新模块的约束是：

- 先找到真正的 owning module，再从根模块导出必要的公共类型；
- 不在 `lib.rs` 复制业务逻辑、解析逻辑或运行时状态；
- `pub` 是跨边界承诺，不是为了方便测试而默认公开。

### 2. `model` 是状态和证据模型，不是万能上下文

`src/model.rs:7-232` 把 schema version、阶段、区域、slot、frame、检测
证据、读取状态、缓存提示和诊断组织成可序列化的领域状态。

值得保留的性质：

- `SlotId`、`ObjectZone` 这种类型表达了领域身份，避免在调用点散落裸
  字符串；
- `FrameRef`、`ObjectEvidence` 和 `diagnostics` 保留了来源和不确定性，
  不把模型猜测伪装成事实；
- `ReadingStatus`、`confidence`、`CacheHint` 把“未读、缓存、需刷新”
  和“读到了什么”分开。

不要把截图句柄、窗口驱动、鼠标点击、模型加载器和业务策略全塞进这个
状态结构。状态模型越像一个万能 `Context`，后续越容易变成史山的共享
垃圾桶。

### 3. 观测管线先保留原始证据，再生成结构化状态

`src/observation.rs:27-118` 的主线是：读取图像和尺寸，加载 detector，
取得检测集，保留 raw evidence，再生成 hand/joker/store/button 等 typed
state，并附带 diagnostics。

这里最重要的不是具体 detector，而是单向数据流：

```text
原始输入 -> 检测结果 -> 领域状态 -> 诊断/输出
```

实现新观测时：

- 不要让下游重新猜测上游已经知道的身份；
- 不要丢掉原始检测、来源 frame 或失败原因；
- 归一化和排序必须有明确规则，不能在多个 action 文件各写一份；
- 观测质量不足时输出“不确定/未读/诊断”，不要静默填一个看似完整的
  结果。

3DGS 对应的边界也是如此：capture、信号包、假设、候选记忆、confirmed
memory、训练数据和质量报告不能被一个 Prompt 或一个 `observe` 函数
隐式串成不可审计的链。

### 4. 每个动作模块拥有自己的结果和确认语义

例如 `src/store_buy.rs:1-158`、`src/cards_clear.rs` 和同目录的
`blind_action.rs`、`pack_choose.rs` 等，分别组织 request、click/selection、
outcome、confirmation、failure reason 和 tracing event。

可复用的形状是：

```text
request -> resolve target -> deliver input -> reread state -> confirmation -> result/artifact
```

动作成功不能只等于“点击 API 返回 Ok”。如果语义确认没有发生，结果应
明确是 selection-only、submitted-but-not-confirmed 或带具体失败原因的
状态。这个规则对 3DGS 也成立：Prompt 产出 hypothesis 不等于 memory
confirmed，训练命令返回 0 不等于空间质量通过。

### 5. 评估和证据按阶段分层

Balatro 的 card detection 相关模块把 producer、semantic validation、
spatial query、quality 和 eval witness 分开，并通过 manifest 保留来源
关系。这个方向比“一个函数同时检测、解释、评分、写报告”更容易定位坏点。

新评估链应明确：

- producer 生成了什么原始产物；
- semantic 层验证什么结构；
- spatial/query 层回答什么查询；
- quality 层如何打分、哪些项不计分；
- witness 如何把输入、版本、来源和结论绑定起来。

## JavaScript / alint 的工程化启示

### 1. 规则本身也是可复用边界

`js/packages/alint-config/src/plugins/auv/agents/judge/agent.ts:19-49`
把模型调用和结构化 finding 解析集中在一个 judge 边界；各规则只提供
自己的 instructions/prompt，并把 finding 变成统一 report。新增规则前先
确认是否能复用这个边界，不要每条规则另写一套模型调用、解析和错误处理。

`config.ts:5-76` 还把规则按 Rust 生产代码、测试契约、side-by-side 测试、
app/game 测试组织和 runtime ownership 分组。规则分组表达的是责任范围，
不是随意按文件名堆配置。

### 2. alint 中值得当作默认审查问题的几条线

当前规则直接对应这些反模式：

- `no-unearned-function-boundary`：只给一行映射、构造或转发起名字的薄
  helper，不拥有策略、校验、不变量、错误契约、外部边界或复用价值；
- `prefer-established-foundation`：已有 typed contract、driver、parser、
  artifact 或公共 helper 时，不在局部复制一份形状相同的替代品；
- `no-private-schema-toolkit`：几个局部字符串抽取/归一化 helper 拼成
  私有 schema parser，却没有成为真正可复用的 owning boundary；
- `require-side-by-side-unit-tests`：保留的 Rust 单测和生产文件按同目录
  `<stem>.rs` / `<stem>_test.rs` 组织，避免生产模块被测试实现淹没；
- `restrict-non-runtime-unit-tests`、`no-source-files-compare-in-tests`、
  `no-mod-names-checks-in-tests`：测试公共、typed、可观察行为，不测试
  文件布局、源码字符串或模块名字是否出现。

这些规则是辅助审查，不是免死金牌。模型规则可能误报，最终仍以代码
owner、编译器、行为测试和真实证据为准。

## 明确不能照抄的地方

### `cli.rs` 仍是警戒信号

当前 `supported/games/auv-game-balatro/src/cli.rs` 约 4,490 行。近期模块
化已经把很多领域类型和动作移出 CLI，这是有效进展；但 CLI 仍承担参数
定义、路由、观测调度、窗口捕获、坐标投影、输入提交、重读确认、输出和
setup 等多个职责。

因此它是“正在改善但尚未完成”的样本，不是新代码的落点。后续添加一个
新操作时，先判断它属于 observation、target resolution、driver delivery、
verification、artifact 或 presentation 哪个 owner；如果只是把更多函数
追加到 `cli.rs`，应先停下来拆出真正的边界。

### 模块数量本身不是质量

`lib.rs` 的 re-export 列表已经很长。继续拆分只有在新模块拥有独立协议、
状态机、错误策略、持久化或外部边界时才值得；仅仅把一段表达式搬到
`helpers.rs`，会制造跳转成本而不减少复杂度。

### 可用结果不等于已证实能力

模型下载成功、截图读成功、命令返回成功、训练进程退出码为 0，都只能
证明对应阶段完成。不要把单帧 detector、单视角 Prompt hypothesis 或
“有一个 PLY 文件”写成产品级空间记忆支持。

## 本仓库新代码的执行规则

1. **先找 owner**：写代码前搜索既有类型、错误、artifact、driver 和
   parser；说明为什么现有边界不够。
2. **一条数据流一个契约**：定义输入、输出、版本、失败状态和证据来源；
   不用隐式全局状态或万能上下文串层。
3. **把阶段结果分开**：observation、inference、decision、action、
   verification、persistence、evaluation 不在一个函数里互相冒充。
4. **保留不确定性**：`unknown`、`unread`、`not_confirmed`、diagnostic
   和 known limits 都是结果，不要用默认值掩盖缺证据。
5. **只保留有决定的函数**：函数如果只是单次转发、字段复制或一行映射，
   优先内联；如果拥有策略、不变量、错误分类、外部 IO、生命周期或多处
   真正复用，才建立边界。
6. **测试行为和证据**：优先调用稳定公共 API，断言结果、artifact、trace、
   replay input 或用户可观察状态；不要断言源码布局。
7. **小步落盘**：每个 slice 只改一个责任面；完成后做格式化、编译、窄
   测试、diff 检查，再进入下一面。
8. **延迟要显式**：暂不实现的字段、fallback、持久化或跨引擎支持，在
   类型或调用点留下 `TODO:` / `NOTICE:` 和解锁条件，不要让下一位维护者
   误以为是遗漏。

## 对 3DGS 的直接套用

3DGS 这条线先保持 app/game-specific，不把未经真实 capture 验证的经验
提升为 AUV core。建议维持如下 owner：

```text
capture / timestamp binding
  -> observation packet
  -> black-box hypothesis
  -> candidate memory patch
  -> independent confirmation
  -> dataset / training backend
  -> reprojection and spatial-quality witness
```

其中：

- 黑盒 Prompt 只能看允许的截图、窗口信息、时间和输入历史；telemetry、
  world state、raycast 和矩阵只能作为事后 oracle，不能泄漏成答案；
- 单视角输出只能是 hypothesis；confirmed memory 需要独立观测和校验；
- 3DGS 是 appearance/reprojection backend，不是 memory truth；
- 训练、查询和质量验证要有各自的 artifact/manifest，不要把训练命令塞
  进 Prompt 或观测模块。

## 当前决策记录

- 结论：**保持为跨切面开发参考和 hook 注入文档**，不新增 AUV core API。
- 采用：模块 owner、typed state、分阶段结果、原始证据保留、行为测试、
  统一 alint judge。
- 拒绝：继续向 `cli.rs` 追加万能 helper、为每个局部数据格式复制 parser、
  用源码布局测试替代行为测试、把模型/命令成功写成能力证明。
- 触发更新：Neko 的 `js` 规则、Balatro 观测/动作/评估边界发生结构性
  变化，或 3DGS 实际 capture 验证推翻当前假设时，先更新本文件和 hook
  注入入口，再继续扩展实现。
