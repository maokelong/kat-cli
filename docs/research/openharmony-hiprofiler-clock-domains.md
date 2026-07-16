# OpenHarmony HiProfiler 时间戳与时钟域

> 调研日期：2026-07-14
> 结论先行：HiProfiler/Hitrace 文件里不存在一种统一的 `timestamp`。同一份采集可以同时包含 ftrace 的 `boot`/`mono` 时间、Native Hook 的 `realtime` 时间、以及插件数据包的上报时间。它们不仅编码形式不同，时钟原点和暂停语义也不同。KAT 在确定展示格式之前，必须先保留时钟域；否则一个看似正常的整数会被错误排序、相减或格式化成日期。

## 调研范围与证据等级

本文只回答 KAT 当前需要回答的问题：OpenHarmony HiProfiler 产物中的时间值是什么、能否比较，以及 KAT 最小需要保留什么。不设计完整的时间转换框架。

- **A：生产者/官方消费者源码**，用于确认实际写入和读取行为。
- **B：官方 protobuf、配置和文档**，用于确认公开字段，但不能替代实现中缺失的语义。
- **C：本地真实 `.htrace` 样本**，用于证明组合确实出现，不作为格式规范。
- **未知**：公开源码不足以证明，KAT 不应猜测。

主要源码基线：

- OpenHarmony `developtools_profiler`：[`c8cd47e`](https://gitcode.com/openharmony/developtools_profiler/tree/c8cd47e52de5d01fbf37f00d176d7e9a87773a57)
- OpenHarmony `developtools_smartperf_host`：[`5c5afb0`](https://gitcode.com/openharmony/developtools_smartperf_host/tree/5c5afb0c479b070148d8a6e336120638a1a03930)
- OpenHarmony `hiviewdfx_hitrace`：[`aeef6f3`](https://gitcode.com/openharmony/hiviewdfx_hitrace/tree/aeef6f3eb65084ca1ae576a4d5277b8d00755ebb)
- Linux tracing 与 POSIX clock 语义：[ftrace `trace_clock`](https://docs.kernel.org/6.6/trace/ftrace.html#trace-clock)、[`clock_gettime(2)`](https://man7.org/linux/man-pages/man2/clock_gettime.2.html)

## 一份文件里的四种时间信息

| 层次 | 编码 | 时钟域从哪里确定 | 含义 | 证据 |
| --- | --- | --- | --- | --- |
| `.htrace` 文件头 | 六组 `u64` 纳秒值 | 字段本身分别代表 boottime、realtime、monotonic 等 | 采集开始附近的一组跨时钟校准锚点 | A |
| `ProfilerPluginData` 外层信封 | `clock_id + tv_sec + tv_nsec` | 外层插件配置；部分写入路径会固定为 realtime | 这批插件数据被上报/封装的时间，不是其中事件发生时间 | A/B |
| ftrace/Hitrace 事件 | 单个 `uint64 timestamp` | 本次 tracefs 实际选中的 `trace_clock` | 内核 ring buffer 给事件打的时间；Hitrace marker 也进入该 ring buffer | A/B |
| Native Hook 事件 | `tv_sec + tv_nsec` | Native Hook 内层配置的 `clock`；缺省为 realtime | 分配、释放等 hook 事件发生时间 | A/B |

因此，“这是 HiProfiler timestamp”不是一个完整类型。至少还要回答：它属于哪一层、哪个 clock、哪次采集。

## 1. ftrace 与 Hitrace 事件

### 值从哪里来

`FtraceEvent` 只声明了一个 `uint64 timestamp`，没有单位和 clock id（[protobuf](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/protos/types/plugins/ftrace_data/default/ftrace_event.proto#L58-L60)）。解析器从 ftrace page header 取得基准时间、累加 event delta 后原样写入该字段，没有换算（[解析声明](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/include/ftrace_parser.h#L44-L60)、[解析实现](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/ftrace_parser.cpp#L462-L480)）。

所以它的语义取决于 tracefs 的 `trace_clock`，而不取决于 protobuf 字段名。当前插件允许 `boot`、`global`、`local`、`mono`（[allowlist](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp#L53)）；配置为空时当前实现选择 `boot`（[默认行为](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp#L291-L299)），但官方配置示例也明确使用 `mono`（[README](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/README_zh.md#L455-L475)）。默认值不能被当成文件格式承诺。

插件会读取实际生效的 `/trace_clock`，把它写入 `FtraceCpuStatsMsg.trace_clock`（[写入代码](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp#L733-L747)、[字段定义](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/protos/types/plugins/ftrace_data/default/trace_plugin_result.proto#L54-L63)）。这是解析产物时应优先采用的事实，而不是从请求配置或外层信封反推。

### 各 clock 的语义

标准 Linux tracefs 路径中的值按纳秒使用，但它们不是同一个坐标系：[内核文档](https://docs.kernel.org/6.6/trace/ftrace.html#trace-clock)明确区分这些时钟。

| `trace_clock` | 原点/暂停语义 | 可安全比较的范围 |
| --- | --- | --- |
| `boot` | `CLOCK_BOOTTIME`；包含系统 suspend 时间 | 同一设备、同一次 boot/capture 中的同域事件 |
| `mono` | `CLOCK_MONOTONIC`；不包含 suspend，可能受 NTP 频率调整 | 同一设备、同一次 boot/capture 中的同域事件 |
| `global` | 跨 CPU 同步的 trace clock，不是 UTC | 同一次 capture 的同域事件 |
| `local` | 快速的 per-CPU clock，CPU 之间可能不同步 | 只能在同一次 capture、同一 CPU 内直接比较 |

`boot` 和 `mono` 即使在某次没有 suspend 的样本中数值接近，也不能合并成一种类型。`global`/`local` 也不能被格式化为现实世界日期。

Hitrace 的 marker 会写入 tracefs `trace_marker`（[写入点](https://gitcode.com/openharmony/hiviewdfx_hitrace/blob/aeef6f3eb65084ca1ae576a4d5277b8d00755ebb/interfaces/native/innerkits/src/hitrace_meter.cpp#L407-L424)、[接口实现](https://gitcode.com/openharmony/hiviewdfx_hitrace/blob/aeef6f3eb65084ca1ae576a4d5277b8d00755ebb/interfaces/native/innerkits/src/hitrace_meter.cpp#L929-L962)），最终仍由同一个内核 ring buffer 打时间。因此，在同一 ftrace 会话中，Hitrace marker 与其他 ftrace event 共享实际选中的 `trace_clock`。

### 一个尚不能下结论的分支

鸿蒙内核的 RMQ 路径直接组合 `RmqConsumerData.timeStamp + RmqEntry.timeStampOffset`（[消费代码](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp#L488-L518)、[结构定义](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/include/ftrace_common_type.h#L45-L63)）。本次能取得的公开源码没有给出生产者侧的单位与原点约束。对这条路径，单位和 clock domain 都应标记为**未知**，不能套用标准 tracefs 结论。

## 2. Native Hook 事件

Native Hook 的配置含 `string clock`（[protobuf](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/protos/types/plugins/native_hook/native_hook_config.proto#L35-L60)）。事件生产者用该 clock id 调用 `clock_gettime`（[raw data builder](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/native_hook/src/rawdata_builder.cpp#L62-L72)），然后把结果作为 `tv_sec`、`tv_nsec` 写入 `NativeHookData`。事件 protobuf 本身却没有 clock id（[结果定义](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/protos/types/plugins/native_hook/native_hook_result.proto#L252-L261)）。

配置字符串支持 `realtime`、`mono`、`mono_raw`、`boot` 等；空值或无法识别的值会落到 `CLOCK_REALTIME`（[映射实现](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/base/src/common.cpp#L862-L882)）。这意味着：

- `tv_sec/tv_nsec` 只是 timespec 的编码形式，不天然代表 Unix 时间；
- 只有 clock 为 realtime 时，才能按 Unix epoch 格式化成人类日期；
- realtime 可能被校时而跳变，不适合在没有额外约束时计算 duration 或保证严格顺序；
- 当前 Native Hook 主路径的外层 packet 使用同一配置 clock 打包，因此可以帮助恢复事件 clock，但这个关系不是所有插件的通用规则（[封包实现](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/native_daemon/src/stack_preprocess.cpp#L2750-L2757)）。

## 3. `ProfilerPluginData` 外层时间不是事件时间

所有插件数据外面还有一层 `ProfilerPluginData`。它包含 clock id 与 `tv_sec/tv_nsec`（[protobuf](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/protos/services/common_types.proto#L49-L72)），普通 writer 在封包时读取配置的 clock 来打时间（[BufferWriter](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/api/src/buffer_writer.cpp#L72-L92)），另一个 protoencoder 路径甚至固定使用 realtime（[FinishReport](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/api/src/buffer_writer.cpp#L108-L120)）。

特别容易误解的是，ftrace 内层的 `TracePluginConfig.clock`（[定义](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/protos/types/plugins/ftrace_data/default/trace_plugin_config.proto#L17-L26)）和外层 `ProfilerPluginConfig.clock`（[定义](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/protos/services/common_types.proto#L19-L25)）是两个不同字段。命令行配置解析只把前者序列化进 plugin payload（[解析代码](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/cmds/src/parse_plugin_config.cpp#L185-L201)）。

所以，ftrace event 完全可能是 `boot` 纳秒，而包裹它的 envelope 是 Unix realtime。外层时间应该命名和理解为“数据包上报/观察时间”，不能叫 event timestamp，也不能通用地拿来推断 payload clock。

## 4. 文件头提供跨时钟锚点，但不是精确同步点

`.htrace` 文件头保存 boottime、realtime、realtime_coarse、monotonic、monotonic_coarse、monotonic_raw 六个 `u64` 纳秒值（[header 定义](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/services/profiler_service/src/trace_file_header.h#L35-L50)）。writer 对六种 clock 依次调用 `clock_gettime` 后写入（[实现](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/services/profiler_service/src/trace_file_writer.cpp#L158-L202)）。ftrace 结果中也能携带相似的 `clocks_detail` 快照（[schema](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/protos/types/plugins/ftrace_data/default/trace_plugin_result.proto#L20-L47)、[采样实现](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp#L769-L797)）。

这些值可作为 clock-domain 转换的校准锚点，但不是原子读取：每次 `clock_gettime` 之间都有时间差，realtime 在采集期间还可能被校时。任何转换都应保留所用 snapshot、误差/质量和来源，不能把换算后的值伪装成原始事实。

OpenHarmony 自己的 Trace Streamer 也采用显式 clock-domain 模型：主时间轴默认 BOOTTIME（[默认值](https://gitcode.com/openharmony/developtools_smartperf_host/blob/5c5afb0c479b070148d8a6e336120638a1a03930/smartperf_host/trace_streamer/src/base/ts_common.cpp#L16-L17)），根据 clock snapshot 做 source-to-primary 转换（[ClockFilter](https://gitcode.com/openharmony/developtools_smartperf_host/blob/5c5afb0c479b070148d8a6e336120638a1a03930/smartperf_host/trace_streamer/src/base/clock_filter.cpp#L37-L63)），并读取 ftrace 实际 `trace_clock`（[parser](https://gitcode.com/openharmony/developtools_smartperf_host/blob/5c5afb0c479b070148d8a6e336120638a1a03930/smartperf_host/trace_streamer/src/parser/pbreader_parser/pbreader_parser.cpp#L724-L747)）。这证明官方消费者也不把所有原始整数当成同一种时间。

不过，Trace Streamer 在缺少映射时会原样返回输入 timestamp。KAT 不应照搬这一静默 fallback：它会让“没有转换”看起来像“转换成功”，与当前项目的 die-soon 原则冲突。

## 5. 本地样本验证

以下仅用于确认上述组合真实存在。

- `hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace`：159,513,402 bytes，SHA-256 `da6877da3f24db1e4754b9f06bcfb35830fb1fffc2ae827ee306548f2cf9f4b9`
- `melotopia-coldstart-nativehook-20260619-132403.htrace`：29,239,027 bytes，SHA-256 `1145a473e6b2aa0a085368f9c5df5eb9e919cec7f5d62dc15b12180fdee28e65`

### SmartPerf/ftrace 样本

样本 `hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace`：

- 文件头 boottime 为 `909080460000 ns`，realtime 为 `1779531907315266250 ns`；
- ftrace 配置和实际 stats 均为 `boot`；
- ftrace `sched_switch.timestamp` 落在约 `909695938750..939094278906`；
- 包裹这些数据的 `ProfilerPluginData` 却为 clock id `0`（realtime），秒值约 `1779531907..1779531937`。

同一文件明确同时存在“事件的 boot 纳秒”和“数据包的 realtime 秒/纳秒”。

### Native Hook 样本

样本 `melotopia-coldstart-nativehook-20260619-132403.htrace`：

- 文件头 boottime 为 `535782204754221 ns`，monotonic 为 `350975339149572 ns`，两者相差约 184806 秒，说明 suspend 语义在真实设备上不能忽略；
- Native Hook 配置没有指定 clock，事件和 envelope 均落在 realtime，秒值约 `1781846647..1781846705`。

这也验证了“编码是 sec/nsec”不等于“类型已经完整”；其 realtime 语义来自配置默认值。

## 6. KAT 当前会丢失什么

当前解析链只保留数值，没有保留证明其语义所需的证据：

- [`formats/hitrace/file.rs`](../../crates/kat-rs-datasource/src/formats/hitrace/file.rs#L19-L47) 的文件头模型截止到 `data_type`，没有读取官方 header 中随后出现的 clock snapshots；
- [`domains/ftrace/packet.rs`](../../crates/kat-rs-datasource/src/domains/ftrace/packet.rs#L37-L51) 只遍历事件，丢弃 `clocks_detail` 和 `ftrace_cpu_stats.trace_clock`；
- [`domains/ftrace/event.rs`](../../crates/kat-rs-datasource/src/domains/ftrace/event.rs#L8-L24) 和 [`sinks/arrow/ftrace.rs`](../../crates/kat-rs-datasource/src/sinks/arrow/ftrace.rs#L8-L23) 只携带 `timestamp` 与 CPU 等上下文，没有 clock domain；
- [`formats/hitrace/profiler/envelope.rs`](../../crates/kat-rs-datasource/src/formats/hitrace/profiler/envelope.rs#L12-L42) 没有把 `ProfilerPluginData.clock_id/tv_sec/tv_nsec` 放入解码上下文；
- [`domains/native_hook/event.rs`](../../crates/kat-rs-datasource/src/domains/native_hook/event.rs#L1-L10) 和 [`sinks/arrow/native_hook.rs`](../../crates/kat-rs-datasource/src/sinks/arrow/native_hook.rs#L8-L20) 只保留 sec/nsec，没有 clock domain。

直接后果是：KAT 可以按一个 ftrace 整数列排序，却不能证明跨 CPU 排序合法；可以把不同插件的数字放在一起，却不能安全地比较或相减；也无法知道一个值能否显示为日期。

## 7. 对 KAT 的最小建议

### 现在应确定的约束

1. **底层时间值携带 clock domain identity。** Datasource 用 `UnifiedClock { ClockDomain, ClockValue }` 表达“哪个时钟域上的什么读数”。`ClockValue` 是非负 `u64` 原生读数，不把纳秒写进类型承诺；计量方式和具体时钟身份在 Dataset 的普通 `clock_domain` Source table 中保存一次，不逐行重复。第一版 `clock_type` 只接受 `boottime`、`monotonic`、`monotonic_coarse`、`monotonic_raw`、`realtime`、`realtime_coarse`、`ftrace_global`、`ftrace_local`，当前频率均为 `1_000_000_000`；jiffies、`unknown`、`other`、`custom` 与无法确认的 RMQ 时间不准入。Datasource 把 `boot`、`mono` 等来源缩写规范化为 `boottime`、`monotonic` 等稳定 lowercase snake_case identity，必要的 CPU scope 才进入名称；Dataset 已经提供设备、采集和启动边界，不再把 UUID、设备、时间、启动 ID 或频率编码进 domain。
2. **原始时间不可变，但 Dataset 不是原始容器镜像。** 文件头 clock snapshots、ftrace `clocks_detail` 与实际 `trace_clock` 作为具有当前分析语义的来源事实进入 Dataset；Datasource 不改写 `.htrace` 内容。用户把原文件保存在覆盖目标之外时，Envelope 时间仍可从中重新解码；第一版不为了逐字段留存而发布没有事件关联的 Dataset table。
3. **跨域运算先显式换算，但不伪造运行时类型保证。** 具有时间语义的比较、排序、join 或相减必须先把两边换算到同一个明确 target domain；同一 domain 的直接运算还必须处于同一 Dataset，`local` 还必须同 CPU。事件列物理上是普通 Arrow `UInt64`，第一版不以自定义类型、列名猜测或 SQL 血缘规则拦截裸整数运算；未经换算的表达式可能被 DataFusion 接受，但其时间语义由 PACK 负责。
4. **ftrace 以实际 stats 为准。** `FtraceCpuStatsMsg.trace_clock` 是整份输入文件的 ftrace 会话级事实。Importer 扫描全部非空报告，一致重复允许，规范化后必须唯一且受支持，并统一解释报告之前和之后的全部 ftrace/Hitrace 事件；不按 packet、报告位置或 CPU 分段，不采用 last-wins。`local` 只在生成 domain 时按事件 CPU 拆分。有这类事件但没有有效报告，或报告未知、RMQ 语义未明、彼此冲突时，整个 Data Import 失败；没有这类事件时不要求报告。原始 Hitrace 文件仍是未修改的来源，不在 Dataset 中发布语义不完整的裸值。
5. **Native Hook 把配置的 clock 传播到事件。** 内层 event packet 没有 clock 字段；配置缺失使 Data Import 失败，已识别的非空值按声明解释，空值按当前生产者的明确语义解释为 `REALTIME`，未知非空值或冲突配置失败。外层 envelope 在当前主路径中使用同一配置 clock，只用于验证一致性；它不能作为 fallback 或跨插件的通用推断规则，与配置冲突时失败。
6. **envelope 时间只作内部协议证据。** 它是 packet/report observation time，不是 event time；当前只在已知插件规则明确关系时用于一致性校验和诊断，不发布 observation table，也不复制进事件表。协议枚举还包含首版时钟模型不支持的进程/线程 CPU clock、alarm、TAI 与 SGI cycle；等真实 packet 分析任务出现后，再把 packet entity、clock 语义和事件关系一起设计完整。
7. **跨域转换由 KAT 集中拥有。** `clock_snapshot` 使用非空 `snapshot_id`、`clock_domain` 与 `clock_value`，并保证 `(snapshot_id, clock_domain)` 唯一；header 六个 reading 是一组，每个独立 `TracePluginResult` 的非空完整 `clocks_detail` 列表各是一组，Datasource 按 group 的文件出现顺序从零分配 ID，不跨 payload 合并或逐 reading 拆分。第一版只用 `snapshot_id = 0` 作为 baseline，来源与目标 domain 必须共同出现；后续 group 只保留，不参与换算，也不拼接多跳。首版所有准入时钟均为每秒十亿 tick，KAT 只用 PyArrow 批量 checked integer kernels 平移两边 baseline 差值，不实现异频缩放、`u128` 或舍入。Baseline 关系沿用到整个当前 trace segment；KAT 不检测或修正采集期间的 offset 变化，其分析后果由使用者负责。定义或 baseline reading 缺失、重复、冲突以及结果越界使整个操作失败，不做 best effort，也不让每个 Workflow 重写换算逻辑。只有关系运算产生的 domain 与 value 同时为 NULL 时传播 NULL；半空值对仍严格失败。

### 现在不应确定的事项

- 不急着统一成 RFC 3339、普通文本或 JSON；第一版只有目标 domain 的 `clock_type` 为 `realtime` 或 `realtime_coarse` 时，PACK 才能在显式换算后用 DataFusion 严格 cast 格式化为 UTC，其他时钟不强求能够显示为日期。
- 不急着设计通用时钟同步服务、误差传播框架或跨设备时间线。
- 不提前发布无法回答“对应哪些事件”的 `plugin_observation` table；未来若有 packet 延迟、阻塞或丢失分析任务，再设计完整 packet entity。
- 不因为 Trace Streamer 选择 BOOTTIME，就覆盖或删除原始 clock-domain 信息。

后续讨论根据原始信息不可变和多计量时钟的约束重新收敛了 ADR 0042：`UnifiedClock` 是 `ClockDomain` 与 `ClockValue` 组成的不可变时间值；事件表以 `clock_domain + clock_value` 平铺保存，`ClockValue` 保留来源计量而不统一冒充纳秒。每个 domain 的类型与固定整数频率在普通 `tables/clock_domain.parquet` Source table 中保存一次，不增加逐行单位字段或新的 catalog。`clock_snapshot` 以 `snapshot_id + clock_domain + clock_value` 保存跨域证据，第一版只使用 ID 为零的 baseline，不使用后续 group 或多跳。多个已确认 domain 可以共存；KAT 不检测或修正 baseline 之后的漂移，风险由使用者承担。换算结果仍是目标 domain 的 `ClockValue`；首版只有 `clock_type` 为 `realtime` 或 `realtime_coarse` 时，PACK 才用 DataFusion 严格 cast 派生 `Timestamp(ns, UTC)`，其他时钟不强求转换。定义或 baseline reading 缺失、重复、冲突时操作失败；Datasource 不改写原始 `.htrace` 内容，显式整体覆盖目标的目录删除语义另行成立。
