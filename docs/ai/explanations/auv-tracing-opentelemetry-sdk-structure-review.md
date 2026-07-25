# `auv-tracing` 与 OpenTelemetry Rust SDK 结构对照

> **历史审查（已于 2026-07-26 被取代）：** 本文描述的是重构前源码，不能作为
> 当前架构建议。authority、commit、revision、snapshot 与 read-side `RunStore`
> 已被删除；当前契约见
> [`2026-07-26-tracing-write-pipeline.md`](../references/inspect/2026-07-26-tracing-write-pipeline.md)。

## 目的与范围

本文对照 `auv-tracing` 与 OpenTelemetry Rust SDK 的概念、公开类型、扩展
trait、常量、函数 API、模块可见性和文件组织。目标是识别可以借鉴的结构，
同时保留 AUV 对持久化 run history、artifact、inspection 和 replay 的特有要求。

本次审查基于：

- AUV 工作区在 2026-07-25 的 `crates/auv-tracing` 源码；
- OpenTelemetry Rust 仓库提交
  [`0e78170d712e5046b8ed93b6f99b2b003af15cd7`](https://github.com/open-telemetry/opentelemetry-rust/tree/0e78170d712e5046b8ed93b6f99b2b003af15cd7)；
- AUV 的共享术语定义：
  [`docs/TERMS_AND_CONCEPTS.md`](../../TERMS_AND_CONCEPTS.md)。

这是一份结构审查，不是已批准的重构计划。表格中的「P0/P1/P2」表示建议的
设计优先级，不表示缺陷严重度，也不构成实现授权。

## 结论摘要

`auv-tracing` 不是在 OpenTelemetry SDK 上增加几个 DTO。它同时包含三类职责：

1. producer instrumentation：`Context`、span、event、artifact emission；
2. canonical run data：authority、commit、revision、snapshot、artifact bytes；
3. lossy projection：把 canonical facts 投影到 Rust `tracing`、OpenTelemetry
   或其他外部观测系统。

OpenTelemetry SDK 主要处理 instrumentation、processor、aggregation 和 export，
没有 AUV 的 canonical authority、event-sourced history、artifact publication 和
snapshot reduction。因此，`RunStore` 不能改造成 `Exporter`，`RunSnapshot` 也不能
改造成 `SpanData`。

当前 AUV 的概念边界大体成立。更需要调整的是源码和 public API 如何呈现这些
边界：

- `lib.rs` 的全面 glob re-export 隐藏了符号所属领域；
- `dispatch.rs` 包含多套可以独立说明的并发与恢复策略；
- `history.rs` 同时拥有 durable model 和 reducer；
- `DispatchErrorReporter`、`BoxFuture` 等共享或调度概念的文件归属不准确；
- 已经存在 `auv-tracing-otel` 独立 adapter crate，core 内的具体
  `RustTracingProjector` 可以按同一原则重新评估归属。

## 1. 概念对应关系

### 1.1 Producer instrumentation 与上下文

| AUV 概念 | OpenTelemetry 最接近的概念 | 关系 | 主要差异 | 判断与建议 |
|---|---|---|---|---|
| `Run` / `RunId` | Trace / `TraceId` | 仅相关性近似 | AUV Run 是相关、持久化、检查和未来 replay 的范围；V1 没有 start、end、status 或 seal，项目术语明确规定它不是 OTel trace。 | 保留区别；文档和代码不应把 Run 重命名成 Trace。 |
| `Context` | `opentelemetry::Context` + active `SpanContext` | 直接但有所扩展 | AUV `Context` 还捕获 `Dispatch`、Run、Authority 和可选 span；OTel Context 是可携带任意值的传播容器。 | 概念合理；只需要改善 namespace，不需要新增一层 pass-through context。 |
| `Span` | OTel SDK `Span` | 近似 | 两者都是一次 instrumentation scope 的 handle。AUV `Span` 通过 drop 产生独立的 start/end facts；OTel span 结束后形成 exporter-facing `SpanData`。 | 保留当前生命周期模型。 |
| `SpanSpec` | tracer/span builder + instrumentation scope | 近似 | AUV 用 associated const 固定 namespaced name，并返回已经验证的 `Attributes`；OTel builder 接受更通用的名称、kind、attributes、links 等。 | 保留强类型 producer contract；不需要为了对齐 OTel 引入 `Tracer` pass-through 层。 |
| `EventPayload` / `EventSchema` | span `Event` 或 `SdkLogRecord` | 近似但不等价 | AUV event 可以只属于 Run，有 schema name/version 和 canonical JSON payload；OTel span event 必须属于 span，log 是另一种 signal。 | 不要强制映射成 OTel span event 或 log；映射属于 projector。 |
| `SpanLink` | OTel `Link` | 有限近似 | AUV 当前只表达一个传播而来的 remote span identity；OTel 可以有多个 link，每个 link 可带完整 context 和 attributes。 | 当前切片保持有限模型；出现具体多-link consumer 后再设计。 |
| `Attributes` | `KeyValue` / `Value` | 直接近似 | AUV 对数量、字符串、浮点数和 compact JSON 大小有 canonical bounds；OTel 更偏向可配置 telemetry limits。 | 保留 AUV 的 validated value objects。 |
| `RemoteContext` | `SpanContext` / remote `Context` | 近似 | AUV 传播 Run、Authority 和 Span，并检查 authority mismatch；OTel 传播 trace context 和可选 baggage。 | 当前边界合理；不要引入未获批准的 baggage。 |
| `TextMapReader` / `TextMapWriter` | `Extractor` / `Injector` / `TextMapPropagator` | 结构近似 | AUV carrier trait 很窄，协议字段和版本由实现隐藏。 | trait 保持 public，字段常量保持 internal。 |

相关源码：

- AUV [`context.rs`](../../../crates/auv-tracing/src/context.rs)、
  [`event.rs`](../../../crates/auv-tracing/src/event.rs) 和
  [`propagation.rs`](../../../crates/auv-tracing/src/propagation.rs)；
- OTel [`trace/provider.rs`](https://github.com/open-telemetry/opentelemetry-rust/blob/0e78170d712e5046b8ed93b6f99b2b003af15cd7/opentelemetry-sdk/src/trace/provider.rs)、
  [`trace/tracer.rs`](https://github.com/open-telemetry/opentelemetry-rust/blob/0e78170d712e5046b8ed93b6f99b2b003af15cd7/opentelemetry-sdk/src/trace/tracer.rs) 和
  [`trace/span.rs`](https://github.com/open-telemetry/opentelemetry-rust/blob/0e78170d712e5046b8ed93b6f99b2b003af15cd7/opentelemetry-sdk/src/trace/span.rs)。

### 1.2 调度、持久化与投影

| AUV 概念 | OpenTelemetry 最接近的概念 | 关系 | 主要差异 | 判断与建议 |
|---|---|---|---|---|
| `Dispatch` | `SdkTracerProvider` + processors/pipeline | 结构近似 | 两者都组合下游、调度和 flush。AUV 还协调唯一 authority、per-run ordering、artifact publication 和非权威 projection。 | 公共概念保留；内部按不变量拆分。 |
| `RunStore` | 没有直接对应；`Exporter` 只是表面相似 | AUV 独有 | Exporter 发送 telemetry，不拥有 canonical truth、读取、订阅、revision、snapshot、artifact bytes 或 recovery。 | 绝不能重命名或收缩成 Exporter。 |
| `AuthorityId` | 没有直接对应 | AUV 独有 | 它防止一个传播的 Run 被多个 store 静默分裂；OTel provider/exporter 没有 canonical authority 身份。 | 保持 core contract。 |
| `RunMutation` / `RunFact` | OTel SDK 内部 span/log/metric data | 很弱的结构近似 | AUV mutation 经过 authority 验证后成为 durable fact；OTel signal data 是处理或 export 输入。 | 保持 AUV 的双阶段模型。 |
| `RunCommit` / `RunRevision` / `IdempotencyKey` | 没有直接对应 | AUV 独有 | OTel export batch 不是原子历史提交，也没有 authority revision 和幂等 replay。 | 保持 history/store contract。 |
| `RunSnapshot` / reducer | 没有直接对应 | AUV 独有 | `SpanData` 和 `ResourceMetrics` 不是从 durable history 归约出的 read model。 | 将 model 与 reducer 分文件，但不拆散概念。 |
| `Artifact` / `ArtifactUri` | 没有直接对应 | AUV 独有 | OTel 不拥有大对象字节、摘要、MIME、原子发布和读取验证。 | 保持独立 artifact domain，不能把 bytes 塞入 event。 |
| `TelemetryItem` | `SpanData` / `LogBatch` / `ResourceMetrics` | 下游 DTO 近似 | AUV DTO 刻意删除 event payload 和 artifact bytes，并携带 authority revision。 | 保持受控、明确有损的 DTO。 |
| `TelemetryProjector` | `SpanExporter`，部分行为也接近 processor | 近似 | OTel exporter 面向 primary telemetry pipeline；AUV projector 消费 canonical facts 的有损映射。 | 保留 `Projector` 名称，避免与 authority store 混淆。 |
| `TaskSpawner` | batch processor worker/runtime | 结构近似 | OTel 默认 processor 多把 worker 隐藏在内部；AUV 暴露 runtime-neutral IO scheduling 注入点。 | 外部确有 runtime adapter，trait 应保持 public；默认实现可 internal。 |
| `Dispatch::flush` | provider/processor `force_flush` | 直接近似 | AUV flush 还覆盖 authority ticket、artifact 和 projector barrier。 | 保持 public，并补充明确 lifecycle 文档；是否需要 shutdown 应单独设计。 |

OTel 的 [`SpanExporter` 与 `SpanData`](https://github.com/open-telemetry/opentelemetry-rust/blob/0e78170d712e5046b8ed93b6f99b2b003af15cd7/opentelemetry-sdk/src/trace/export.rs)
明确是 export port 和 export DTO；其
[`SpanProcessor`](https://github.com/open-telemetry/opentelemetry-rust/blob/0e78170d712e5046b8ed93b6f99b2b003af15cd7/opentelemetry-sdk/src/trace/span_processor.rs)
是 span lifecycle hook。这两个边界都不能替代 AUV 的
[`RunStore`](../../../crates/auv-tracing/src/store.rs) 和
[`history`](../../../crates/auv-tracing/src/history.rs)。

### 1.3 OpenTelemetry 有、AUV 当前没有的概念

| OTel 概念 | AUV 是否需要对应概念 | 原因 |
|---|---|---|
| Sampling / `ShouldSample` | 不应进入 canonical recording core | AUV durable facts 不能因 telemetry sampling 丢失；projector 或下游 SDK 可以独立 sampling。 |
| `SpanKind` | 当前不需要 | AUV operation/app/driver 的领域类型不应被压成 client/server/producer/consumer。出现明确外部映射需求时由 projector 决定。 |
| Span `Status` | 当前不需要 | AUV 的 direct result、activation、verification 和 failure 是不同边界，不应合并成通用 span status。 |
| `Resource` | 暂无直接对应 | OTel Resource 描述 telemetry producer。AUV Device/Session 尚未成为 V1 run identity，不应只因 OTel 有 Resource 就新增。 |
| `InstrumentationScope` | 部分由 namespaced names 承担 | AUV 当前通过 namespaced span/event names 保持 producer vocabulary；是否需要独立 producer identity 需要具体 consumer。 |
| Baggage | 当前不需要 | 任意传播 metadata 会扩大 wire 和信任边界；当前传播协议有意只传 Run/Authority/Span。 |
| Metrics aggregation | 不属于 `auv-tracing` canonical history | 需要 metrics 时可由 OTel projector/SDK 或 viewer projection 承担。 |

## 2. Struct、enum、trait、常量与 API 设计

### 2.1 Struct 与 enum 家族

| AUV public 类型家族 | OTel 对照 | 当前判断 | 改进或拆分建议 |
|---|---|---|---|
| `Context`、`Span`、`ContextGuard`、`WithContext<F>`、`Instrumented<F>` | provider/tracer/span 和 active context | public 合理；guard/future 是可命名返回类型 | 归入 `context` 或 instrumentation namespace。`context.rs` 内部 cohesive，暂不因文件长度拆分。 |
| `SpanSpec`、`EventPayload`、`EventSchema`、`JsonPayload` | span builder、event/log API | public 合理；AUV 的 schema/version/canonical JSON 更严格 | 保留强类型 API。严格 JSON codec 可继续由 `event` 深模块隐藏。 |
| `NewArtifact<R>`、`ArtifactEmission`、`ArtifactUri`、artifact read errors | OTel 无直接对应 | public 合理，但一个文件覆盖 producer、read、codec 和 receipt | 建议用私有 `model`、`read`、`emission`、`json` 子模块，公共概念仍为 `artifact`。 |
| UUID IDs | `TraceId`、`SpanId` | public newtype 合理 | 建议归入 `identity` 或 `value` namespace；保持 UUIDv7 和非 nil 验证。 |
| `RunRevision`、`PageLimit`、`Timestamp` | sequence/time/limits | public validated value 合理 | 保持 opaque field + constructor。不要改成裸整数。 |
| `AttributeKey`、`AttributeValue`、`Attributes` | `Key`、`Value`、`KeyValue` | public 合理 | 建议归入 `attributes`；保持 canonical bounds。若 public enum 将来会扩展，需要先决定 sealed V1 或 `non_exhaustive` 策略。 |
| `BoundedString`、`FiniteF64` | OTel value 内部 scalar | 当前因 `AttributeValue` public variants 而成为 public API | 若保持 tuple variants，它们必须可见；若未来封闭 variants，可收窄并只通过 constructors/getters 暴露。 |
| `NamespacedName` | OTel `Key` 或 instrumentation name | 外部通常使用 `SpanName`、`EventName` 等 wrapper | 可考虑 `pub(crate)`，但必须先验证 serialization 和公共类型签名没有泄漏。不要单独进行兼容 shim。 |
| `NonEmptyVec<T>` | 无直接对应 | 当前被 `auv-tracing-inspect` protocol 使用，必须 public | 移入 contract/value namespace，停止 root-level 暴露；不能直接改成 `pub(crate)`。 |
| `SpanStarted`、`SpanEnded`、`EventOccurred`、artifact facts | `SpanData`/log data 仅表面相似 | canonical cross-store contract，应 public | 放入 `history::model`，字段继续 private，使用 validated constructors/getters。 |
| `RunMutation`、`RunFact`、`RunCommitRequest`、`RunCommit` | 无直接对应 | canonical wire/history contract，应 public | 保持明确分层；新增 variant 必须按 V1 contract/version 边界评估。 |
| `SpanSnapshot`、`RunSnapshot`、`ReduceError` | 无直接对应 | public read/reduction contract 合理 | 放入 `history` facade；`IncrementalReducer` 继续 `pub(crate)`。 |
| `DispatchBuilder`、`Dispatch`、dispatch errors | `SdkTracerProviderBuilder`、provider errors | public lifecycle root 合理 | `Dispatch::builder()` 可提高 discoverability；`configure()` 可作为高频便捷 API 保留。无需同时引入新的 Provider 类型。 |
| `TelemetryItem`、`TelemetryRoutePolicy`、`TelemetryError` | export data/config/error | public projection contract 合理 | 明确 enum 的扩展策略；它面对外部 projector，新增 variant 会影响 exhaustive match。 |
| `StoreArtifactRequest`、`RunCommitPage`、store errors/results | exporter/reader DTO 仅结构近似 | store implementor 和 inspect client 需要 public | 只从 `store` namespace 暴露。`StoreArtifactRequest::new` 参数较多，但没有证据支持先加浅 builder。 |
| `MemoryRunStore`、`FileRunStore` | in-memory exporter、具体 exporter backend | concrete implementation | `MemoryRunStore` 可暂留 feature-gated；filesystem backend 长期归属 `auv-tracing-store-local`。 |
| `RustTracingProjector` | tracing appender/adapter | concrete adapter | core feature 可视为过渡；仓库已有独立 `auv-tracing-otel`，Rust `tracing` adapter 也应评估独立 crate。 |

### 2.2 Trait 设计

| AUV trait | OTel 对照 | 接口判断 | 建议 |
|---|---|---|---|
| `SpanSpec` | tracer/span builder 输入 | 窄且类型化 | 保持 public；associated `NAME` 与 validated attributes 适合 AUV。 |
| `EventPayload` | event/log record producer API | 窄且类型化 | 保持 public；`NAME` + `VERSION` 是 AUV schema contract。 |
| `RunStore` | 没有直接对应 | 深 port：一个接口隐藏 authority write、read、subscription、artifact 和 recovery | 当前不要为了模仿 exporter/reader 而拆 trait。只有出现独立 read-only/write-only implementor 压力时，再评估 reader/writer 分离。 |
| `TelemetryProjector` | `SpanExporter` | 窄 extension port | 保持 public。`project` 与 `flush` 足够表达现有下游；不要把 retry 或 exporter config 拉进 core。 |
| `TaskSpawner` | processor runtime/worker | 真实外部调度边界 | 保持 public；`DispatchTask` 和 `TaskSpawnError` 同属 `dispatch::runtime` 或 `dispatch` facade。 |
| `DispatchErrorReporter` | OTel internal logging/error reporting | 真实诊断边界，但当前文件归属错误 | 从 `telemetry.rs` 移到 `dispatch`；它报告所有 dispatch failure，不只 telemetry failure。 |
| `TextMapReader` / `TextMapWriter` | `Extractor` / `Injector` | 窄 carrier port | 保持 public；`inject` implementation 和字段列表继续 private。 |

`RunStore` 和 `TelemetryProjector` 都因为需要 `dyn Trait` 而返回 boxed future。
当前通用 [`BoxFuture`](../../../crates/auv-tracing/src/store.rs) 定义在 `store.rs`，
但也被 telemetry 和 dispatch 使用。这是文件 ownership 泄漏：

- 不建议复制出多个同形 alias；
- 可把共享 erased-future primitive 移到一个很小的 internal support 文件；
- 因它出现在 public trait signature，可在 crate root 或明确的 `port` namespace
  做一次精确 re-export；
- 名称是否改为 `PortFuture` 需要作为 public API 变更单独批准。

### 2.3 常量、限制值与关联常量

| AUV 设计 | OTel 对照 | 当前判断 | 建议 |
|---|---|---|---|
| `SpanSpec::NAME` | instrumentation scope/span name | associated const 表达稳定 schema | 保持。 |
| `EventPayload::{NAME, VERSION}` | OTel 没有强制 event schema version | AUV 特有且有价值 | 保持；version validation 留在 `EventSchema`。 |
| attribute、JSON、page、artifact byte limits | OTel `SpanLimits` 等可配置 limits | AUV 的多数限制属于 canonical V1 contract，而不是 deployment tuning | 继续 private，避免调用方把它们当可配置 SDK knob。若确有预检需求，可在 owning type 上选择性公开 `MAX` associated const。 |
| propagation field names/version | OTel propagator 内部 header names | protocol implementation detail | 继续 private；carrier implementor只需按回调处理 name。 |
| file frame magic/version/chunk size | exporter/backend protocol constants | filesystem backend detail | 继续 private，并随 `FileRunStore` 移到实现 crate。 |
| stable error code strings | OTel typed SDK error + internal codes | AUV wire/diagnostic contract | 放在产生该错误的 owning module，避免建立全局 error-code 常量仓库。 |

需要区分两类 limit：

- canonical limit：改变后会改变 accepted wire/data contract；
- operational limit：worker 数量、batch 大小、timeout、backoff 等部署或调度策略。

前者应由 validated type 隐藏，后者才可能进入 builder/config。不要把两类限制放进
一个类似 OTel `Config` 的宽结构。

### 2.4 函数与宏 API

| AUV API | OTel 对照 | 判断 | 建议 |
|---|---|---|---|
| `configure()` | provider builder entrypoint | 高频配置入口合理 | 可保留，并考虑增加 `Dispatch::builder()`；两者应返回同一 builder，不新增 wrapper service。 |
| `dispatcher::set_global_default` / `with_default` | `opentelemetry::global` provider API | namespace 合理 | 保持 `dispatcher` 为显式公共 namespace；不要把所有函数继续平铺。 |
| `start_span` / `emit_event` / `emit_artifact` | tracer/event recording API | 高频 producer API | 可以精确重导出到 crate root，并保留同名宏。 |
| `Context::inject` / `extract` / `from_remote` | propagator inject/extract | 生命周期完整 | `extract` 应从 `propagation` namespace 暴露；`inject` implementation 继续 internal，由 `Context::inject` 调用。 |
| `read_artifact_bytes` / `read_json_artifact` | 无直接对应 | typed consumer helpers 有价值 | 放在 `artifact` namespace，不应成为大量 root 函数之一。 |
| `reduce_commits` | 无直接对应 | deterministic public reducer API | 从 `history` namespace 暴露；incremental reducer 保留 internal。 |
| `artifact_identity_conflict_error_code()` | typed error classification | public helper略显机械 | 当前 wire code consumer 仍需要它，可留在 `store`；未来若 error enum 能直接表达 identity conflict，再评估删除 helper。 |

## 3. Module 暴露与 public API 管理

OpenTelemetry SDK 根模块公开稳定领域：`trace`、`logs`、`metrics`、`resource`、
`propagation`。每个 signal 的 `mod.rs` 声明私有实现文件，并精确 re-export 公共
类型。例如：

- [`trace/mod.rs`](https://github.com/open-telemetry/opentelemetry-rust/blob/0e78170d712e5046b8ed93b6f99b2b003af15cd7/opentelemetry-sdk/src/trace/mod.rs)；
- [`logs/mod.rs`](https://github.com/open-telemetry/opentelemetry-rust/blob/0e78170d712e5046b8ed93b6f99b2b003af15cd7/opentelemetry-sdk/src/logs/mod.rs)；
- [`metrics/mod.rs`](https://github.com/open-telemetry/opentelemetry-rust/blob/0e78170d712e5046b8ed93b6f99b2b003af15cd7/opentelemetry-sdk/src/metrics/mod.rs)；
- [`metrics/internal/`](https://github.com/open-telemetry/opentelemetry-rust/tree/0e78170d712e5046b8ed93b6f99b2b003af15cd7/opentelemetry-sdk/src/metrics/internal)。

OTel 也存在不应照抄的历史不一致，例如 metrics facade 仍有 glob re-export，
trace 的 simple/batch processor 仍集中在一个大文件。AUV 更适合参考 trace/logs 的
精确 facade 和 metrics 的 private algorithm directory。

| AUV 当前模块 | 当前问题 | 建议 public surface | 建议 internal 拆分 | 优先级 |
|---|---|---|---|---|
| `lib.rs` | 九组 `pub use ...::*` 将所有领域压平成 root API | 公开 `context`、`artifact`、`history`、`store`、`dispatch`、`telemetry`、`propagation` 等领域 namespace；root 只精确重导出高频 producer API 和少量通用 IDs/attributes | `macros` 继续 private，宏通过 `macro_export` 留在 root | P0 |
| `context.rs` | 文件同时有 current context、guard、future instrumentation 和 span lifecycle，但共享同一 scope invariant | `pub mod context`，精确公开 Context/Span/spec/guards | clock、TLS frame、span state、drop guards 全部 private；当前不必拆文件 | P1 |
| `event.rs` | public event contract 和严格 JSON serializer 同文件 | `pub mod event`，公开 schema/payload trait/JSON payload/errors | serializer/visitor 可保持 private；只有导航压力持续增加时才进 `event/json.rs` | P1 |
| `artifact.rs` | construct、bounded JSON、read verification、emission receipt、URI 混合 | `pub mod artifact` | `model.rs`、`json.rs`、`read.rs`、`emission.rs`，只在 `mod.rs` 精确 re-export | P1 |
| `history.rs` | durable wire model 与 reducer/parentage validation 同文件 | `pub mod history` | `model.rs` + `reducer.rs`；`IncrementalReducer` 保持 `pub(crate)` | P0 |
| `dispatch.rs` | 公开 builder/错误与 ticket、lane、cursor、artifact worker、projection、flush 状态机同文件 | `pub mod dispatch`，公开 Dispatch/Builder/errors/TaskSpawner | `progress.rs`、`lane.rs`、`cursor.rs`、`artifact.rs`、`projection.rs` 均 private 或 `pub(crate)` | P0 |
| `telemetry.rs` | projection contract 清楚，但混入 `DispatchErrorReporter` | `pub mod telemetry`，公开 item/projector/policy/error | filtering/routing helper private；将 reporter 移到 `dispatch` | P0 |
| `rust_tracing.rs` | concrete adapter 位于 core feature | 过渡期可由 `telemetry` 精确 re-export | 长期评估独立 `auv-tracing-tracing` crate；状态机继续 private | P1 |
| `store.rs` | trait、DTO、errors 与 shared `BoxFuture` 同文件，具体 backends 也由 core feature 暴露 | `pub mod store`，公开 RunStore/request/result/page/errors/read streams | `memory.rs`/`file.rs` 实现文件 private；shared future primitive 移出 store ownership | P0 |
| `store/memory.rs` | reference/test backend 与 production use 尚未完全区分 | feature-gated `store::MemoryRunStore` | 实现状态 private；若成为独立生产 backend，再评估单独 crate | P1 |
| `store/file.rs` | 包含 filesystem layout、locking、frame、durability、recovery 等 OS 知识 | core 最终只依赖 `RunStore` port | 移至 `auv-tracing-store-local`，全部 backend helpers private | P1 |
| `value.rs` | identity、names、attributes、paging、time、artifact integrity 都落在泛化的 `value` 文件 | 公开稳定 value families，但不为每个 newtype 建 public module | 建议 `identity.rs`、`attributes.rs`、`value.rs`；`NonEmptyVec` 放 contract/value namespace | P1 |
| `propagation.rs` | 内部边界清楚，只是被 root flatten | `pub mod propagation`，公开 carrier traits、extract、error、opaque remote context | inject、field list、version/error builders private | P0 |
| `macros.rs` | 无问题 | macros 继续在 crate root | source module private | 保留 |

### 3.1 建议的目标树

下面的树只表达责任边界，不要求一次性创建所有文件：

```text
src/
├── lib.rs
├── context.rs
├── event.rs
├── propagation.rs
├── identity.rs
├── attributes.rs
├── value.rs
├── history/
│   ├── mod.rs
│   ├── model.rs
│   └── reducer.rs
├── artifact/
│   ├── mod.rs
│   ├── model.rs
│   ├── json.rs
│   ├── read.rs
│   └── emission.rs
├── dispatch/
│   ├── mod.rs
│   ├── progress.rs
│   ├── lane.rs
│   ├── cursor.rs
│   ├── artifact.rs
│   └── projection.rs
├── store/
│   ├── mod.rs
│   ├── memory.rs
│   └── file.rs          # 过渡位置；长期属于 store-local crate
├── telemetry/
│   ├── mod.rs
│   └── rust_tracing.rs  # 过渡位置；长期评估 adapter crate
└── macros.rs
```

这个结构不建议继续拆出 `span_started.rs`、`span_ended.rs`、
`artifact_uri.rs` 等单类型文件。文件应该隐藏一项稳定决策，而不是只让每个文件
更短。

### 3.2 Public API facade 示例

下面只展示方向，不是可直接应用的 patch：

```rust
pub mod artifact;
pub mod context;
pub mod dispatch;
pub mod event;
pub mod history;
pub mod propagation;
pub mod store;
pub mod telemetry;

mod attributes;
mod identity;
mod macros;
mod value;

// 高频 producer API 可以保留 root ergonomics。
pub use context::{Context, Span, SpanSpec, emit_event, start_span};
pub use artifact::{emit_artifact, emit_json_artifact};
pub use identity::{ArtifactId, AuthorityId, EventId, RunId, SpanId};
pub use attributes::{AttributeKey, AttributeValue, Attributes};

// dispatcher 继续作为明确 namespace，而不是散落的 global functions。
pub use dispatch::dispatcher;
```

`identity`、`attributes` 和 `value` 是否公开为模块，需要与 root ergonomics 一起
决定。关键约束是停止新增 glob export，而不是立即删除所有已有 root path。
移除或弃用现有 path 属于公共 API 迁移，需要 owner 单独批准；不应在内部文件
拆分时顺带完成，也不应默认添加长期 compatibility shim。

## 4. 建议顺序

| 顺序 | Slice | 是否改变行为/public API | 原因 |
|---|---|---|---|
| 1 | 将 `history.rs` 拆成 `history/model.rs` 与 `history/reducer.rs` | 不改变行为；可保持现有 re-export | 两部分因不同原因变化，且已有明确 `pub(crate)` reducer seam。 |
| 2 | 将 `dispatch.rs` 按 progress/lane/cursor/artifact/projection 拆成私有子模块 | 不改变行为和 public API | 减少理解并发状态机时需要同时加载的策略数量。 |
| 3 | 把 `DispatchErrorReporter` 移回 dispatch ownership，把 shared future primitive 移出 store ownership | 可以保持 public path，具体迁移方式需批准 | 修复明显的信息归属泄漏。 |
| 4 | 决定 public namespace 和精确 re-export 清单 | public API 设计变更 | 需要先决定 root ergonomics、breaking boundary 和是否发布兼容窗口。 |
| 5 | 拆 `artifact` 和 `value` 内部文件 | 不一定改变 public API | 价值低于 history/dispatch，应结合下一次真实修改进行。 |
| 6 | 将 FileRunStore、Rust tracing adapter 移到实现 crate | package/feature 变更 | 与现有 `auv-tracing-otel` 和设计文档中的 capability matrix 对齐。 |

## 5. 保留项与暂不实现项

以下部分不建议仅为了模仿 OpenTelemetry 而改变：

- 不新增 AUV `Tracer` 或 `TracerProvider` pass-through 层；
- 不把 `RunStore` 改名为 exporter；
- 不把 canonical history 变成 export DTO；
- 不把 event canonical JSON 改成无版本 attributes；
- 不给 canonical recording 增加 sampling；
- 不把 direct result、activation 和 verification 合并成 span status；
- 不拆分 `RunStore` reader/writer trait，除非出现明确的独立实现压力；
- 不把每个 newtype 拆成一个文件；
- 不因这份审查直接新增 shutdown、Resource、Baggage、多 SpanLink 或新的
  telemetry variant。

这些候选项都需要具体 consumer、失败案例或 owner-approved slice 才能重新打开。

## 6. 总体判断

`auv-tracing` 在概念上比 OpenTelemetry SDK 多出一层 canonical run-data system：
authority、commit、revision、idempotency、snapshot、artifact 和 recovery。多出的概念
并不是冗余；它们服务 AUV 的 recording、inspection 和 replay 目标。

OpenTelemetry SDK 对 AUV 最有价值的结构经验有三项：

1. 用稳定领域目录管理 public API，用 `mod.rs` 做精确 facade；
2. 把 provider/producer、processing、data 和 output port 分成不同责任；
3. 将复杂算法和具体 backend 放在 private submodule 或 implementation crate，
   不让它们污染常用 instrumentation API。

按这些原则看，AUV 最先需要调整的是 `history`、`dispatch` 和 crate-root facade，
而不是修改 Run、RunStore、Artifact 或 Projection 的核心语义。
