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

Output Query 沿用前置重构后的单 Run Output 合同：Runtime 只注册该 Run 的
`output.*`，由 DataFusion 原生 writer 把对象行直接写入候选 NDJSON；公共成功结果只
返回 `format`、`path` 和有序 `columns`。查询数据不进入 Runtime Response 或 Rust，
也不再有内联 `rows`、Dataset 状态、KAT 自定义 Arrow 标量投影或类型白名单。旧
compact positional JSON 会同时保留已删除的 Dataset 状态，并让 Runtime 与 CLI 承担
已经不需要的数据转码，因此在 Dataset 执行面退役后不再维持该合同。

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
trait、插件注册、自动来源识别或 RPC。原生扩展具有平台相关构建与验证生命周期，且
CLI 已没有其 Rust 调用者；拆成第二个私有 wheel 可以保持依赖边界并由各原生 Payload
builder 验证，而两个 artifact 仍随同一 KAT 版本原子装配和发布。

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
| ADR-0038 | Dataset 状态、内联 columns/positional rows 与 compact Query Result |
| ADR-0039 | Query Result 的自定义 64 位整数投影 |
| ADR-0040 | Query Result 的自定义非有限浮点拒绝规则 |
| ADR-0041 | Query Result 的自定义 Decimal 投影 |
| ADR-0044 | Query Result 的自定义 UTC 纳秒 Timestamp 投影 |
| ADR-0046 | Query Result 的自定义 Arrow 字符串投影 |
| ADR-0051 | 通过 Workflow Context、Dataset clock evidence 与 UDF 执行时钟换算 |

下列决定只被局部取代；未列入“取代”栏的其余内容继续有效：

| 既有决定 | 本决定取代 | 继续有效 |
| --- | --- | --- |
| ADR-0001、ADR-0008 | Dataset 输入、Manifest reference、`dataset.*`、CLI 对 datasource crate 的依赖和查询数据经 Runtime Response 内联返回 | Runtime 执行、Run/Output 发布与后续只读查询的所有权边界 |
| ADR-0002 | Payload 只安装一个 KAT wheel，以及默认 Dataset/Data Import 目标 | 同版本原子发布、原生平台矩阵、私有 Host、Data Home 其他目录与 Skill Assembly 边界 |
| ADR-0003 | PACK 内 Test Dataset 和已删除操作的 discovery 表述 | External PACK 部署单位、生产/测试源码共同版本化、统一 Authoring API 与无持久 registry |
| ADR-0004 | `kat inspect --dataset` 启动边界与 Import → Run → Query 验收链 | 强制 Bundled Python Host、isolated 启动参数、CPython/平台矩阵及无系统 Python fallback |
| ADR-0005、ADR-0032 | 三个旧 Context 数据方法、DataFrame Output 与 Run/Manifest Dataset path | 显式 Context、调用期 lease、命名 Output 和 all-or-fail 发布原则 |
| ADR-0007 | Discovery 操作集合中的 `kat import` 与 `kat inspect --dataset` | 静态 manifest、短命 PACK discovery、路径/name 校验与无持久 registry |
| ADR-0010 | Dataset IPC/Manifest、`dataset.*` 查询、Dataset Parquet 数据面与 Dataset inspection 日志边界 | 文件型封闭 typed IPC、进程/日志 owner、Run/Output publication 与失败边界 |
| ADR-0011 | `kat inspect --dataset` 的最小 Skill 资源与无日志边界 | Skill 直接选择平台 Payload、可移动根定位与按需资源校验 |
| ADR-0012 | `kat-cli -> kat-datasource` 固定依赖、crate 统一拥有 Dataset/所有 Datasource type，以及两个 builder 安装同一个 KAT wheel | 源码/部署视图分离、package 内聚与 Payload 黑盒装配边界 |
| ADR-0013 | Import、Dataset inspection/参数/查询，以及 Query Result 经 Runtime Response 内联进入 KAT Response | 单一 Skill、其余操作动词与具名目标句法 |
| ADR-0014 | Data Import → Dataset inspection → `required_tables` 匹配的 Dataset-first analysis flow | 自动选择 PACK/Workflow，并只在实质歧义时询问用户 |
| ADR-0016、ADR-0074 | Dataset inspection、Test Dataset、execution plane、Table Grant/TableGrantResolver 及过渡性 Dataset 参数 | PACK pytest、Workflow/Provider inspection 与 declaration |
| ADR-0017 | `tests/datasets/` Test Dataset | 固定 `workflows/`、`datasources/`、`helpers/`、`tests/` 布局及 import/pytest 所有权 |
| ADR-0019 | `run_workflow`/`test_pack`/inspection 的 Dataset request、Manifest Dataset，以及 `query_run` 的 Dataset/内联 rows | 封闭 operation-specific typed IPC、严格分支与 Runtime Response owner 边界 |
| ADR-0023 | CLI 直接管理本地 Dataset | 无 REST daemon、短命 CLI/Runtime 与文件式 IPC |
| ADR-0024、ADR-0025 | Dataset mutation、`required_tables` 可运行性、根级规范化 `sched_switch` 及 Import result 形状 | Trace fact 的复用门槛，以及未知扩展与损坏数据的 fail-closed 区分 |
| ADR-0027 | 以 Dataset 承担跨 PACK source fact 复用 | PACK 自包含、无 PACK dependency/Exported Capability、PACK 私有 helper 与公共 Authoring API 边界 |
| ADR-0034 | Dataset/Data Import 时钟身份、旧换算执行面、`ctx.sql()` 时间参数及 Query Result 的 KAT 时间投影 | Duration、WallClockTimestamp 与不同时间语义不可混用的领域合同 |
| ADR-0035 | `--dataset`、`required_tables`、Table Grant、DataFrame Output、旧 decorator/inspection 字段，以及 Workflow 参数与旧 Query Result 64 位整数投影保持一致的陈述 | Workflow argv/签名/默认值解析、Click 参数语义与 Python Runtime owner 边界 |
| ADR-0036 | Data Import/Dataset publication、Dataset inspection Response、Run/Query Dataset 状态与内联 rows | Skill-first KAT Response、JSON failure/diagnostic 及其余 operation 的公开投影 |
| ADR-0037 | Dataset inspection 的 typed error 与无日志表述 | 单一 Diagnostic 到 JSON/terminal 的投影、日志故障与可靠 span 语义 |
| ADR-0042 | UnifiedClock/换算执行面、Test Dataset、Dataset inspection/overwrite、Resolved Dataset/`query_run.dataset` 以及 Required tables/Table Grant | 原始 clock relations、`clock_domain + clock_value` 与来源时钟语义边界 |
| ADR-0043 | Runtime 私有 Session/Table Grant 数据执行面与 `kat_convert_clock` UDF | 成熟向量化能力优先、证据驱动的性能下沉与不维护平行实现 |
| ADR-0045 | Payload 只有一个 KAT wheel 的交付假设 | Pack Authoring API 与 Runtime 继续共用 `kat-workflow` wheel |
| ADR-0047 | `kat inspect --dataset` Runtime 边界，以及测试调用依赖 execution plane、Table Grant 与旧 Workflow Context 的隔离方式 | 单一 `kat.pack` module identity、pytest 测试树所有权与生产源码复用边界 |
| ADR-0048 | 旧 ftrace/规范化 `sched_switch` 数据链、`required_tables`、`--dataset` 与 DataFrame mapping Output | 线程 CPU 时间用户问题、可观测区间/聚合语义与 Output relation |
| ADR-0056 | Dataset owner/校验、内联 columns/positional rows、自定义标量投影、CLI 二次组装与“不产生 query artifact” | 可信同版本 IPC 单元、`test_pack`/`run_workflow` owner 与传输失败边界 |
| ADR-0058、ADR-0059 | Trace Streamer Import、Dataset 与旧 Context/DataFrame 调用 | 两个 OpenHarmony Workflow 的分析问题、SQL 含义和 Output 合同 |
| ADR-0061 | Dataset Storage publication、marker 与根级规范化 relation | build-time plan、typed emitter、descriptor-derived relations 及 protobuf 语义 |
| ADR-0066、ADR-0067 | 迁移期保留的 `ctx.sql() -> DataFrame` 兼容入口及 Catalog 不接管它的表述 | 显式 `dp.DataFusionProvider`、纯 Parquet Catalog、短命 Session 和 eager `dp.Table` |

## 验证

交付必须同时证明：保留的实际 Hitrace roots 能生成、编译并流式写入；compiler 对
canonical FQN、relation/name collision 和不支持的 reachable protobuf shape 仍在
构建期给出带 root、message 与 field path 的诊断；Python Hitrace API 与失败发布合同
通过；两个 wheel 的 metadata 互不依赖；Windows 与 Linux Payload 都完成
Hitrace decode → Provider/`dp.open()` → Workflow → Run Output → `kat query` NDJSON
全链路 smoke。任一平台不能替代另一平台，验证记录必须对应最终提交的精确 head。
