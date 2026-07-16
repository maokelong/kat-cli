---
status: accepted
---

# Hitrace 事件保留时钟域与原始读数

HiProfiler `.htrace` 是多时钟容器：ftrace/Hitrace event、Native Hook event、`ProfilerPluginData` envelope 和文件头 clock snapshots 可以使用不同编码与 clock domain，其中 envelope 时间表示 packet/report observation time，不是 payload event time。HiProfiler 当前源码与真实样本只证明采集开始附近存在 clock snapshot，不保证像 Perfetto 一样在整个采集期间周期校准；证据记录在 [`docs/research/openharmony-hiprofiler-clock-domains.md`](../research/openharmony-hiprofiler-clock-domains.md) 与 [`docs/research/industry-multi-clock-observability.md`](../research/industry-multi-clock-observability.md)。KAT 不据此预造一套公共纳秒时间坐标，但接受同一 trace segment 的 `snapshot_id = 0` 作为该 segment 内跨 domain 换算的有效锚点。

Datasource 用 `UnifiedClock { ClockDomain, ClockValue }` 表达来源时间事实。`ClockValue` 是来源时钟上的非负 `u64` 原生读数，不把纳秒写进类型承诺；Datasource Adapter 可以把合法 `tv_sec/tv_nsec` 无损合成为纳秒整数。每个 `ClockDomain` 的定义在 Dataset 中保存一次，拥有解释该读数所需的时钟类型与固定整数频率；跨 domain 对齐证据仍由 `clock_snapshot` 表达。定义使用普通 `clock_domain` Source table，物理上仍是既有规则下的 `tables/clock_domain.parquet`，不增加 catalog、根级 manifest 或 field metadata 协议。任何 Dataset facts 引用的 domain 都必须在该表中恰好存在一条定义。

`clock_domain` Source table 的第一版 Schema 只有三个非空字段：Dataset 内唯一的 `clock_domain: Utf8`、使用 KAT 封闭值的 `clock_type: Utf8`，以及大于零的 `ticks_per_second: UInt64`。第一版 `clock_type` 精确封闭为 `boottime`、`monotonic`、`monotonic_coarse`、`monotonic_raw`、`realtime`、`realtime_coarse`、`ftrace_global`、`ftrace_local`，当前全部以 `1_000_000_000` ticks per second 编码；只有 `realtime` 与 `realtime_coarse` 声明 Unix epoch 墙上时间语义，后者只降低精度，其他类型不表示 UTC。Hitrace header 六种系统时钟使用各自同名 domain 与 type；ftrace 实际 `boot`、`mono`、`global` 分别映射为 `boottime`、`monotonic`、`ftrace_global`，`local` 在 CPU 3 上映射为 domain `ftrace_local_cpu_3` 与 type `ftrace_local`；Native Hook 的 `boot`、`mono`、`mono_raw`、`realtime` 以及已确认的空配置按同一完整名称映射。第一版不接受 jiffies、`unknown`、`other` 或 `custom`，RMQ 等语义未确认来源直接失败；以后只有真实 Datasource 证据出现时才增加新的封闭值。第一版也不增加重复的 `unit`，不保存 scope、`origin`、`is_unix_epoch`、monotonicity、description、offset、nullable 扩展字段或 JSON。

`clock_domain` 由 Datasource 分配完整匹配 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$` 的稳定可读名称，并成为该 Datasource 的表契约。单例系统时钟使用 `boottime`、`monotonic`、`monotonic_raw`、`realtime` 等完整名称，不把 Hitrace 的 `boot`、`mono` 等来源缩写直接泄露给 PACK；同一 Dataset 中确需区分多个 scope 时才增加必要后缀，例如 `ftrace_local_cpu_3`。Dataset 已经限定设备、采集与启动实例，因此名称不编码 UUID、设备名、采集时间、启动 ID 或频率。`clock_type` 只做分类，不能充当目标 alias；KAT 不转换大小写、不做模糊匹配，也不在只有一个同类型 domain 时自动选择。目标不存在时诊断列出实际可用 domain 后失败。不同 Dataset 中相同的名称仍不是同一个时钟实例。

事件表把这个逻辑值平铺为两个不可分割解释的列：

```text
clock_domain: Utf8 NOT NULL
clock_value: UInt64 NOT NULL
```

`clock_value` 不叫 `timestamp` 或 `timestamp_ns`，因为它脱离 `clock_domain` 后既不能说明单位，也不能证明可与另一列比较。换算结果仍是目标 domain 的 `ClockValue`，发布时使用 `boottime_clock_value` 一类带 domain 的自说明名称；纳秒频率本身不赋予 UTC 或 Duration 语义。只有同域两点经过业务校验后的差值才可命名为 `duration_ns`，只有换算到契约声明为 Unix epoch 的墙上时间 domain 后再严格 cast，才形成 `Timestamp(ns, UTC)`。Hitrace 来源事件不重复保存一份数值相同的公共纳秒列，也不增加逐行 `unit` 或 `hz` 字段。

Datasource 必须根据来源证据确认 domain。ftrace/Hitrace 的 `FtraceCpuStatsMsg.trace_clock` 是整份输入文件的会话级事实：Importer 扫描全部非空报告，一致重复合法，规范化后必须得到唯一受支持值，并把它应用于全部标准 tracefs ftrace/Hitrace 事件，包括报告位置之前的事件。它不按 packet、报告位置或 CPU 分段，不采用 last-wins；`local` 只在生成具体 domain 时按事件 CPU 拆分。有这类事件却没有任何有效报告时失败，任一非空报告未知或多个报告冲突时也失败；没有这类事件的文件不要求报告存在。Native Hook event packet 本身没有 clock 字段，必须依据产生该批事件的配置语义确定。Native Hook 配置缺失时失败，已识别的非空 `clock` 按声明解释，空值按照当前生产者的明确语义解释为 `REALTIME`，未知非空值或彼此冲突的配置失败；外层 envelope 只用于验证一致性，不能作为 fallback 或跨插件的通用推断规则，与配置冲突时失败。来源编码非法、domain 无法确认或对应 domain 定义不完整也使整个 Data Import 失败。

第一版不把 `ProfilerPluginData.clock_id + tv_sec + tv_nsec` 发布为 Dataset table，也不把它逐行复制进事件表。一个没有事件关联的 observation table 会让用户自然期待 KAT 回答“这个 packet 对应哪些事件”，而当前插件协议并没有提供一套可复用的统一关系；同时 envelope 的合法 `ClockId` 还包含进程 CPU、线程 CPU、alarm、TAI 与 SGI cycle 等首版 `clock_type` 无法解释的坐标。仅为保存字段而发布它，会迫使 KAT 虚构关系、丢弃合法值或扩大时钟契约，却没有当前 Workflow 需要这种查询。Dataset 因而只是当前分析能力的规范化投影，不是原始容器的逐字段档案；用户保存在覆盖目标之外的 `.htrace` 保留全部 envelope 信息。Datasource 只在 Native Hook 等已知插件规则明确事件与 envelope 关系时把它用于内部一致性校验和诊断。以后出现真实的数据包延迟、阻塞或丢失分析任务时，再一次性设计具有明确 clock 语义和事件关联的完整 packet entity；不预留 `plugin_observation` 半成品接口。

多个已确认 clock domain 本身不再导致 Data Import 失败。文件头 snapshots 与 ftrace `clocks_detail` 继续作为不可变来源事实保存在普通 `clock_snapshot` Source table，其第一版 Schema 只有非空 `snapshot_id: UInt64`、`clock_domain: Utf8` 与 `clock_value: UInt64`。Snapshot group 不由 KAT 推断：`.htrace` 文件头中的六个 clock reading 共同构成一组；每个独立 `TracePluginResult` 中非空的整个 `clocks_detail` 列表各构成一个新 group，空列表不产生 group。Datasource 按文件中的 group 出现顺序从零分配 ID，因此正常文件 header 是 `snapshot_id = 0`；它不跨 payload 合并 group，也不逐 reading 拆分。同组来源 clock 名规范化后出现重复 domain 时 Data Import 失败，`(snapshot_id, clock_domain)` 必须唯一。第一版只把 `snapshot_id = 0` 作为全 Dataset baseline，来源与目标 domain 必须同时且唯一地出现在该 group；后续 snapshot 只作为来源事实保留，不参与换算，也不因显示 offset 变化而使操作失败。KAT 不拼接不同 group 做多跳转换；相同具体 domain 的转换恒等且不需要 snapshot。

Baseline 是当前 trace segment 内充分的跨域映射证据，KAT 将其关系作为常量沿用到整个 segment，不检测或修正 offset 漂移。目标 domain 定义或 baseline reading 缺失、重复、冲突时，具体换算操作直接失败。这个规则是 KAT 接受采集系统证据的产品策略，不是对底层时钟稳定性的证明：suspend、NTP、手动校时或其他时钟变化造成的分析后果由使用者负责。KAT 也不把相同 domain 名称自动当成同一设备、同一次启动或同一个时钟实例。

第一版八种准入 `clock_type` 的频率都固定为每秒十亿 tick。设事件来源读数为 `source_value`，baseline 中来源与目标读数分别为 `source_base` 和 `target_base`，KAT 只做同频平移：

```text
if source_value >= source_base:
    target_value = target_base + (source_value - source_base)
else:
    target_value = target_base - (source_base - source_value)
```

实现使用 PyArrow 批量 checked integer kernels，不经过浮点数或 Python 逐行对象。相同具体 domain 恒等返回。任何一行因为定义或 baseline reading 缺失、频率不是首版固定值、结果小于零或超过 `u64` 而无法换算时，整个操作失败，不返回原值、NULL 或部分结果。第一版不实现异频缩放、`u128` 乘除、舍入或多跳；以后只有真实 Datasource 证据要求准入不同频率的 `clock_type` 时，才重新设计并验证对应换算。

关系运算可能让一个原本非空的事件变成可选值：来源 `clock_domain` 与 `clock_value` 同时为 NULL 时，`kat_convert_clock` 返回 NULL；只有一个为 NULL 时，逻辑上的 `UnifiedClock` 已不完整，整个查询失败。两个输入都非 NULL 时继续使用严格换算，未知 domain、缺少证据和越界不能降级为 NULL。这个“全空传播、半空失败”规则使 LEFT JOIN 无匹配行保持普通关系语义，同时不掩盖破损的值对。`target_domain` 必须是非空固定字符串，SQL NULL 或 Python `None` 直接拒绝。

两个来源 Expr 的 Arrow 类型必须精确为 `Utf8 clock_domain` 与 `UInt64 clock_value`。UDF 不把来源 `LargeUtf8`、`Utf8View`、有符号整数、Decimal 或其他可转换类型隐式 cast，也不在 Python 入口增加例外。需要从其他物理类型形成时钟值的 PACK 必须先使用 DataFusion 的严格显式 cast；负数、越界或非法文本在 cast 或计划边界失败，类型诊断同时显示实际类型和期望类型。这避免普通整数被偶然提升为 ClockValue，也不把 DataFusion 版本相关的 coercion 规则纳入 KAT Interface。SQL target 只接受普通字符串字面量，Python target 只接受普通 `str`；Bundled DataFusion 对 literal 的物理 string 表示是引擎内部细节，不属于来源 coercion 或公共类型承诺。

Workflow 与 Output Query SQL 都使用 Runtime 注册的 scalar UDF `kat_convert_clock(clock_domain, clock_value, target_domain)`；第三个参数必须是字符串字面量，例如 `boottime`。DataFrame Workflow 使用 `ctx.convert_clock(clock_domain_expr, clock_value_expr, *, target_domain: str)`，它只构造调用同一 UDF 的 DataFusion Expr。两个入口都返回目标 domain 下的 `UInt64 ClockValue`，不返回 Struct 或重复目标 domain 列；发布时由 PACK 选择自说明列名。调用者不传 snapshot、频率或 Dataset 路径，Runtime 也不开放通用 UDF lookup。这复用 DataFusion 的 SQL 与 Expr UDF seam，同时保持 SessionContext、注册表和时钟证据私有。

目标 domain 是 Workflow 的业务选择，不是普通用户输入，也不是 Skill 在 `kat run` 执行前根据 Dataset 临时推导的策略。PACK 作者依据 Datasource 的稳定表契约显式选择目标，并用 Test Dataset 验证；目标或 baseline 不存在时，严格诊断列出可用 domain 与具体缺失证据。第一版不增加 target-domain discovery 命令、Context 方法或 Dataset inspection 字段。`kat inspect --dataset` 仍由 Rust Dataset Storage 只读取文件树和 Parquet metadata；让它解释 `clock_domain`、`clock_snapshot` 行会复制 Python Runtime Resolver 的语义，而仅返回名称或“可转换”布尔值又不能证明具体表达式可换算。Output Query 真有临时发现需要时，直接查询已经注册的普通 `dataset.clock_domain`。只有反复出现真实的 Workflow 执行前动态选择任务后，才设计 Runtime-owned 的完整能力检查，不提前建立 clock graph、转换矩阵或表达式血缘。

首版唯一实现是 Workflow Runtime 私有的 `stable` Python/PyArrow batch UDF。它对整批 Arrow array 使用 PyArrow compute kernels，不调用 `.as_py()`、不建立 Python per-row loop，也不新增 PyO3 module、FFI capsule、KAT native wheel 或 Rust 平行实现。引擎只拥有 UDF 注册、SessionContext、执行生命周期与资源边界；时钟能力作为引擎之上的私有库/UDF 对 SQL 和 Expr 提供同一语义。只有代表性 trace 的真实性能证据证明这里成为关键瓶颈时，才保持两个公开入口不变替换为更底层实现，不提前建立双实现或抽象 port。

换算结果仍然只是目标 domain 上的 `ClockValue`。第一版只有目标定义的 `clock_type` 为 `realtime` 或 `realtime_coarse` 时，PACK 才能继续使用 DataFusion 的严格 Arrow cast 得到 `Timestamp(ns, UTC)`；`realtime_coarse` 只降低精度。超出 Arrow timestamp 承载范围或转换不合法时查询失败，不使用 `try_cast` 产生 NULL。KAT 不增加 `origin`、`is_unix_epoch`、wall-clock UDF 或 Context 方法。其他 domain 即使频率同为每秒十亿 tick，也不自动获得 UTC 语义，第一版不要求它们可格式化为公历时间。

Runtime 创建普通执行面时不要求或解析时钟表。第一次实际执行任一换算入口时，它才从当前请求可用的 Resolved Dataset 按需读取并验证 `clock_domain`，为该执行面构建一次内存 Resolver 并复用；进程结束后直接丢弃，不创建磁盘缓存、manifest 或跨进程状态。Workflow 在本次执行提供 Dataset 时使用它；Output Query 只有在 `query_run.dataset` 为 `available` 时使用其中查询当下的当前 Dataset，即使该路径已被覆盖。`not_provided`、`unavailable` 或可用 Dataset 缺少时钟证据时，实际换算失败，但不影响未执行换算的 Workflow 或纯 `output.*` 查询。Runtime 不分析 UDF 参数来自 `output.*` 还是 `dataset.*`，不保存旧 evidence、revision 或表达式 lineage，历史 Output 与当前 Dataset 是否仍可共同解释由用户负责。目标和所有非空来源 domain 都必须有合法定义，因此同 domain 恒等换算也不能绕过 `clock_domain`；恒等换算本身不需要 `clock_snapshot`。只有执行中实际出现跨 domain 输入时，Resolver 才要求双方同时存在于 `snapshot_id = 0`，缺失或非法 snapshot 使该操作整体失败。其他 Datasource 不被迫生成占位表。Runtime 的内部证据读取不要求 Workflow 把两张表加入 Required tables；PACK 直接查询它们仍服从普通 Table Grant。

需要比较、排序、Join 或相减不同 domain 的时间时，PACK 先用上述函数把两边换算到同一个显式 target domain，再使用 DataFusion 原生运算符。第一版不增加 `kat_clock_compare`、`kat_clock_join`、`kat_clock_diff`，也不声称能够阻止未经换算的裸值运算：平铺后的 `clock_value` 是普通 Arrow `UInt64`，可靠识别任意表达式的时间意图需要重新采用自定义类型、Struct 或自建不完整的 SQL/Logical Plan 血缘规则。KAT 不根据列名猜测；DataFusion 接受的裸整数运算仍可执行，但其时间语义由 PACK 负责。

要把两点之差发布为 `duration_ns`，PACK 必须选择同一个纳秒频率 target domain，明确验证业务终点不早于起点且差值在 `Int64` 范围内，再使用 DataFusion 原生算术。KAT 不从列名推断起止关系，不增加 Duration UDF，也不自动取绝对值、交换两端或把负差变成 NULL。没有可换算到的纳秒 domain 时仍可分析原始 tick，但第一版不能从中形成 KAT Duration。

第一版不实现通用 clock graph、误差传播、跨设备时间线或 best effort 换算。Datasource 不改写 `.htrace` 内容；但它若位于用户显式授权整体清除的 Dataset 目标内，仍服从 `--overwrite-dataset` 的目录语义。KAT 统一的是时钟值结构、证据边界和失败语义，不是来源读数的物理单位。
