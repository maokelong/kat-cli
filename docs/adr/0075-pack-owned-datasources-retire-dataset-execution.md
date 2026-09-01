---
status: accepted
---

# PACK 自有 Datasource 取代 Dataset 执行面

KAT 不再把来源接入统一收敛为平台 Dataset、Data Import 和 Workflow Runtime
中的隐式数据执行面。Datasource 是 PACK 顶层 `datasources/` 下的普通 Python
module，Datasource Provider 由 PACK owner 直接维护，Workflow 显式 import、构造和
调用。Provider 独占来源定位、配置解释、decode、Source query 和来源物化；生产
Workflow Runtime 不发现、注册、包装、启动或关闭 Provider，也不定义统一 Provider
基类、远端协议或生命周期。显式 Provider inspection 只读取 metadata declaration，
不改变生产调用边界。
跨来源组合只由 Workflow 把已经显式取得的 Table 或 Parquet Catalog 交给
`dp.DataFusionProvider` 完成。

Workflow Context 只通过受当前调用期 lease 约束的 `ctx.datasource_root` 暴露当前
PACK 的私有数据目录能力。`ctx.sql()`、`ctx.from_arrow()`、`ctx.convert_clock()`、
Dataset grant、Table Grant、`required_tables` 和 Runtime 私有 DataFusion execution
plane 全部退役。标准结构化 Workflow Output 精确为一个 `dp.Table`，或非空普通
`dict[str, dp.Table]`；DataFusion DataFrame、空 dict、混合值和兼容 adapter 均不接受。

KAT CLI 删除 `kat import`、`kat inspect --dataset`、`kat run --dataset`、Test Dataset
和 Output Query 的 `dataset.*` relation/状态。新 Run Manifest 不写来源 Dataset；读取
历史 Manifest 时只接受并忽略任意 JSON 形状的旧 `dataset` 字段，不解析路径、不验证
文件、不恢复 relation，也不公开 Dataset 状态。用户磁盘上已有的 Dataset 目录不自动
删除、不迁移，KAT 也不增加清理工具。

Hitrace 原生解析收窄为独立私有 distribution `kat-datasource` 的唯一公共模块
`kat_datasource.hitrace`。`decode(source, destination)` 要求 destination 尚不存在，
在同父目录的私有 staging 中流式生成平铺的具名 Parquet relations，全部 decode、
relation close 和校验成功后才以 no-replace 语义发布 destination。成功只返回不可变
`DecodeReport`，其中包含排序、去重的 `unsupported_plugins` 与
`unsupported_section_types`；格式损坏、I/O 错误或无法安全继续时抛出稳定
`DecodeError`。不增加成功 marker、长期 manifest、覆盖、恢复、合并或 cache 协议。

Hitrace 继续发布 descriptor-derived ftrace/native-hook relations、protobuf relation
mapping、`clock_domain` 与 `clock_snapshot` 普通来源 relations；这些 clock relations
不重新建立 UnifiedClock、Clock grant、Clock UDF 或平台换算执行面。旧根级规范化
`sched_switch` relation 退役，不影响 descriptor-derived
`trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format` relation。保留的
build-time protobuf relational plan、prost binding 与 generated typed emitter 继续由
直接合同测试覆盖合法 closure、拒绝边界和诊断位置；不恢复旧 Dataset/DataFusion
round-trip 测试栈。

两个仍消费 Trace Streamer SQLite 的 OpenHarmony Bundled PACK 各自拥有只读 SQLite
Provider。Workflow 接收绝对、已存在的普通文件路径，Provider 使用只读 URI、
`query_only`、authorizer 和具名参数；原分析含义与 Output 回归保持不变。这不是新的
SQLite Datasource type，也不形成跨 PACK Provider framework。

`kat-workflow` 与 `kat-datasource` 是同一 KAT 版本、彼此没有 package dependency 的
两个私有 wheel。Payload builder 把二者作为带预期版本和 SHA-256 的显式 artifact，
分别校验并以 `--no-deps` 安装；平台 Runtime lock 只锁第三方依赖。`kat-datasource`
不发布可执行文件、不修改 PATH，`kat-cli` 也不链接 native datasource crate。Ftrace
文本和 Trace Streamer SQLite 继续由具体 PACK Provider 负责，不增加通用 Decoder
trait、插件注册、自动来源识别或 RPC。

## 与既有决定的关系

下列决定已被本决定完整取代，其历史文件只用于说明旧架构：

| 旧决定 | 被取代的完整合同 |
| --- | --- |
| ADR-0009 | Dataset 路径身份、Run Dataset reference 与 `dataset.*` 查询 |
| ADR-0015 | `required_tables`、Table Grant 与 Dataset 准入 |
| ADR-0020 | Dataset Storage、marker、writer、publication、resolution 与 inspection |
| ADR-0021 | `kat import` 及一次 Import 选择一个 Datasource |
| ADR-0022 | 编译进 CLI 的封闭 Datasource type 集合 |
| ADR-0026 | 通过 `ctx.from_arrow()` 进入受管理 DataFrame execution plane |
| ADR-0033 | Dataset grant 下通过 `ctx.sql()` 派生且不修改 Dataset 的执行合同 |
| ADR-0051 | 通过 Workflow Context、Dataset clock evidence 与 UDF 执行时钟换算 |

下列决定只被局部取代；未列入“取代”栏的其余内容继续有效：

| 既有决定 | 本决定取代 | 继续有效 |
| --- | --- | --- |
| ADR-0001、ADR-0008 | Dataset 输入、Manifest reference、`dataset.*` 和 CLI 对 datasource crate 的依赖 | Runtime 执行、Run/Output 发布与后续只读查询的所有权边界 |
| ADR-0005、ADR-0032 | 三个旧 Context 数据方法与 DataFrame Output | 显式 Context、调用期 lease、命名 Output 和 all-or-fail 发布原则 |
| ADR-0013、ADR-0016、ADR-0074 | Import、Dataset inspection、Test Dataset 及过渡性 Dataset 参数 | 单一 Skill、PACK pytest、Workflow/Provider inspection 与 declaration |
| ADR-0024、ADR-0025 | Dataset mutation、根级规范化 `sched_switch` 及 Import result 形状 | Trace fact 的复用门槛，以及未知扩展与损坏数据的 fail-closed 区分 |
| ADR-0042、ADR-0048 | UnifiedClock/换算执行面、旧 ftrace 准入和规范化 `sched_switch` 数据链 | 原始 clock relations 和线程 CPU 时间问题中的来源语义边界 |
| ADR-0045 | Payload 只有一个 KAT wheel 的交付假设 | Pack Authoring API 与 Runtime 继续共用 `kat-workflow` wheel |
| ADR-0058、ADR-0059 | Trace Streamer Import、Dataset 与旧 Context/DataFrame 调用 | 两个 OpenHarmony Workflow 的分析问题、SQL 含义和 Output 合同 |
| ADR-0061 | Dataset Storage publication、marker 与根级规范化 relation | build-time plan、typed emitter、descriptor-derived relations 及 protobuf 语义 |
| ADR-0066 | 迁移期保留的 `ctx.sql() -> DataFrame` 兼容入口 | 显式 `dp.DataFusionProvider`、短命 Session 和 eager `dp.Table` |

## 验证

交付必须同时证明：保留的实际 Hitrace roots 能生成、编译并流式写入；compiler 对
canonical FQN、relation/name collision 和不支持的 reachable protobuf shape 仍在
构建期给出带 root、message 与 field path 的诊断；Python Hitrace API 与失败发布合同
通过；两个 wheel 的 metadata 互不依赖；Windows 与 Linux Payload 都完成
Hitrace decode → Provider/`dp.open()` → Workflow → Run Output → `kat query` NDJSON
全链路 smoke。任一平台不能替代另一平台，验证记录必须对应最终提交的精确 head。
