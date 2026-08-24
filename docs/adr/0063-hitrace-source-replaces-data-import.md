---
status: accepted
---

# Hitrace Source 取代顶层 Data Import

KAT 的公开来源操作只保留 `kat bind` 与 `kat materialize`。顶层 `kat import` 连同 Hitrace、Deprecated Trace Streamer 两个变体整体删除，不保留 alias、兼容期或私有第三种用户入口。删除命令前，`kat-kernel` 必须先交付名为 `hitrace` 的 Source Entry，并用真实 `.htrace` fixture 证明 External Query、通用 Materialization 与 Materialized Query 都能读取既有 Hitrace 事实；迁移顺序不得造成用户可见的 Hitrace 能力空窗。

`kat-kernel/hitrace` 第一版只接收一个具名 `trace: pathlib.Path`，返回 DataFusion Source Schema。它复用现有 Rust Hitrace parser、时钟与调度事实实现，通过 DataFusion 官方 `FFI_SchemaProvider` 和 PyO3 进入 Bundled Python Host；Python Entry 只负责参数合同和返回 native provider，不重写解析器，也不让通用 `kat materialize` 识别 Hitrace。FFI exporter 使用 DataFusion Python 提供的 codec capsule，并返回名称精确为 `datafusion_schema_provider` 的 PyCapsule。生产端的 DataFusion/DataFusion FFI 版本与 Bundled Host 锁定版本保持一致，平台分别构建并安装 Linux、Windows native wheel。

第一版 Source 只发布旧 Hitrace 产品入口已经公开且经过验证的规范化事实：`clock_domain`、`clock_snapshot`，以及来源存在时的 `sched_switch`。Native Hook descriptor-derived tables、多个 capture 聚合、`capture_id`、时钟 identity 合并和新的 unsupported-content 查询合同都不随迁移自动公开；它们需要独立的消费证据和设计。Source 首次被解析时可以一次完成单文件解析并在本次 Source operation 内复用结果；是否使用临时 Parquet 是 native provider 的私有实现，不形成第二种 Materialized Source 或发布协议。

Deprecated Trace Streamer 的 Rust/SQLite 入口、测试 fixture 和唯一依赖它的两个预发布 PACK 一并删除。`kat-openharmony-critical-path` 与 `kat-openharmony-thread-cpu-time` 不迁移、不保留 alias；ADR-0058 与 ADR-0059 因此成为历史。Hitrace parser、domain facts、protobuf/codegen 和对应合同测试继续保留，不能因删除 CLI verb 而误删。研究文档和 proto 中对上游 Trace Streamer 的来源引用也继续保留。

这次迁移同时完成 ADR-0062 中尚未闭合的统一查询语义：Dataset Query、Workflow Run 与 Run Output Query 都可以解析所选 Dataset 中的 External 或 Materialized Binding，包括跨 PACK External Binding。调用方可用可重复 `--pack-dir` 提供精确 PACK candidates；Bundled PACK、Data Home PACK 与显式 candidates 继续服从同一唯一发现规则。用户显式选择 Dataset 并提交查询或 Run，即授权执行其中被实际解析且唯一发现的 PACK Source code，不增加 `--allow-external`。Workflow 所属 PACK 仍公开挂载为 `kat.pack`；其他 PACK 的 Source modules 使用按 PACK 隔离的 Runtime 私有 namespace，Source 及其内部 helper 必须用相对导入，避免多个 PACK 争用全局 `kat.pack`。

Materialized Binding 继续保存用于 REDO 的原始 Source argv 与绝对工作目录，但查询只使用已经物化的表。Materialized tables 完全遮蔽 External recipe；缺少的表明确不存在，不回退 Provider。只物化显式 `--table` 子集时，该子集就是当前 Source 的完整查询面。再次执行 `kat materialize` 可以重放保存的 recipe，也可以用本次显式 argv 在 `--replace` 授权下完整替换。Recipe 不是权威 provenance、版本锁或一致性保证，PACK/配置改变后的行为仍未定义。

KAT 管理的 Materialized Source 第一版每张表只接受 `<table>.parquet` 单文件。外部 Provider 可以直接复用 DataFusion/PyArrow 的多文件、远端 Parquet 或数据库能力，但 KAT 不为自身 Dataset 定义 fragment 目录、Schema 合并或第二套文件发现规则。后续出现真实多文件持久化需求时，直接评估成熟设施的 Dataset 行为。

本决定修订 ADR-0062 中“跨 PACK External 不执行”“Dataset/Output Query 只解析 Materialized”“Materialized 不保存 recipe”和“Materialized table 可为 fragment 目录”的条款，并完成其中顶层 `kat import` 的退场。ADR-0058 与 ADR-0059 的状态改为 `superseded by ADR-0063`；其他 Hitrace 领域合同继续有效，但其产品入口统一由 `kat-kernel/hitrace` Source 提供。
