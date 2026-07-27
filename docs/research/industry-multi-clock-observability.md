# 多时钟可观测系统的业内做法与 KAT 时间轴设计

> 决策更新：ADR-0051 保留本文的时钟证据与批量换算结论，但第一版只通过
> `ctx.convert_clock(...)` 暴露换算，不再注册 SQL `kat_convert_clock(...)`。

> 调研日期：2026-07-14
>
> 结论先行：成熟系统不会让普通查询者手写 clock conversion。Perfetto 在具有周期同步证据时导入到一条 trace time；OpenTelemetry 则在互操作协议入口直接要求 Unix epoch；Linux ftrace/perf 保留采集者选择的 clock。它们共同要求时间值的 domain 与计量语义可确认，不能只靠一个整数或字段名猜测。
>
> KAT 不直接照搬 Perfetto 的导入时统一 `ts` Interface。第一版保留 `UnifiedClock { ClockDomain, ClockValue }`：事件表平铺 `clock_domain + clock_value`，每个 domain 的类型与固定整数频率在普通 `clock_domain` Source table 中定义一次，跨域证据由 `clock_snapshot` 保存。HiProfiler 当前只有稀疏起始快照；KAT 明确只接受同一 trace segment 的 `snapshot_id = 0` 作为整个 segment 的常量映射锚点，但不检测或修正 suspend、NTP、手动校时等造成的后续变化，其分析风险由使用者承担。

## 1. 范围与判断标准

本文回答三个问题：

1. 业内多时钟 trace/observability 系统把 clock domain、同步证据和换算放在哪一层；
2. 普通用户最终看到什么 Interface；
3. 在 KAT 当前“单次本地 capture、Hitrace Datasource → Dataset/Parquet → DataFusion、raw source 不可变、die soon、Skill first”的范围内，如何保留完整时钟语义，同时不把单位参数逐行暴露给用户。

只使用官方规范、官方文档和生产者/消费者源码。这里不设计跨设备同步服务、误差传播系统、NTP/PTP 集成或跨 Dataset 时间线。

## 2. 业内系统怎么做

### 2.1 Perfetto：采集保留 domain，导入统一 trace time

Perfetto 是最接近 KAT 的参照物，因为它同样面对 ftrace、应用事件、日志和硬件时钟等异构本地 trace source。

其采集协议包含两个关键事实：每个 `TracePacket` 可以通过 `timestamp_clock_id` 指明自身 clock domain；`ClockSnapshot` 同时记录两个或多个 clock 的读数，形成同步点。内置 clock、sequence-scoped custom clock 和 global custom clock 都是显式概念，而不是从字段名猜测。[Perfetto 多时钟设计](https://perfetto.dev/docs/concepts/clock-sync)、[`ClockSnapshot` protobuf](https://android.googlesource.com/platform/external/perfetto/+/refs/heads/main/protos/perfetto/trace/clock_snapshot.proto)

Trace Processor 在导入时重建 clock graph，把事件时间转换到 primary trace clock。直接连接不存在时可以沿最短路径多跳换算；每一跳使用相邻 snapshot 的 offset。不同 clock 会因为 suspend 或调频而改变相对关系，所以 `traced` 会周期性记录内置 clocks，Trace Processor 再按时间选择邻近 snapshot，而不是永远套用采集开始时的一个常量 offset。[Perfetto conversion operation](https://perfetto.dev/docs/concepts/clock-sync#operation)

Perfetto 还明确处理非单调 clock：如果 snapshot 显示一个 domain 发生倒退，它不能继续充当无歧义的 source path，只能作为 conversion target。这一点尤其针对可能跳变的 realtime。[Perfetto caveats](https://perfetto.dev/docs/concepts/clock-sync#caveats)

重要的是，复杂度没有泄露给普通分析者：Trace Processor 的事件表只暴露已经位于 trace time 的 `ts` 和 `dur` 纳秒列；原始映射证据另外保存在 `clock_snapshot` 表中，列出 trace time、clock id/name 和原始 clock value。[Perfetto `slice` 与 `clock_snapshot` 表](https://perfetto.dev/docs/analysis/sql-tables#clock_snapshot)

需要公历时间时，Perfetto 也没有要求用户拼 offset，而是提供 `to_realtime(ts)`、`abs_time_str(ts)` 等标准函数；转换失败返回 `NULL`。这说明 wall-clock presentation 是 trace time 之上的显式派生能力，不是所有 `ts` 的固有含义。[Perfetto time conversion functions](https://perfetto.dev/docs/analysis/stdlib-docs#prelude-functions)

可概括为：

```text
producer timestamp + clock id + repeated snapshots
                    │
                    ▼
          Trace Processor clock module
                    │
                    ▼
       one primary trace-time `ts` for SQL
```

### 2.2 OpenTelemetry：在互操作 seam 统一成 epoch，保留时间角色

OpenTelemetry 解决的是跨进程、跨语言、跨厂商传输，不是内核 trace 文件的任意 clock graph。它选择了更强的 wire-level 约束：Tracing API 的 `Timestamp` 是从 Unix epoch 开始的时间，`Duration` 是两个事件之间的 elapsed time；两者是不同概念。[OpenTelemetry Tracing API time types](https://opentelemetry.io/docs/specs/otel/trace/api/#time)

Logs Data Model 又把“事件发生时间”和“收集系统观察到的时间”拆成 `Timestamp` 与 `ObservedTimestamp`，二者都是 Unix epoch 纳秒。源时间未知时可以缺失，但 observation time 应在 OTel 首次观察事件时设置。换言之，OTel 保留的是时间的业务角色，不是把两个来源不同的时间塞进同一个模糊字段。[OpenTelemetry Logs Data Model](https://opentelemetry.io/docs/specs/otel/logs/data-model/#field-timestamp)

OTel 不承诺传输文件按 timestamp 排好序，官方 file exporter 明确说记录无序，timestamps 也不保证单调。这避免把“同为 epoch 编码”误解成“到达顺序、因果顺序或单调时钟已经被解决”。[OTLP File Exporter ordering](https://opentelemetry.io/docs/specs/otel/protocol/file-exporter/#streaming-appending)

对 KAT 的启示不是“全部改成 UTC”，而是：

- 公共 Interface 必须给时间一个确定语义；
- event time 与 observation/report time 不能混用；
- 如果协议选择只暴露一种 timestamp，归一责任必须在协议入口之前完成，而不能交给每个消费者补救。

### 2.3 Linux ftrace/perf：采集者选 clock，消费者不能靠整数猜

Linux ftrace 给 ring-buffer event 加 timestamp 时使用当前 `trace_clock`。`local` 很快但严格 per-CPU，跨 CPU 可能不同步；`global` 跨 CPU 同步；`mono` 对应 `CLOCK_MONOTONIC`；`mono_raw` 不受频率调整；`boot` 对应 `CLOCK_BOOTTIME` 并计入 suspend。改变 `trace_clock` 还会清空当前 ring buffer，说明 clock 是采集会话级语义，不是每个值自己携带的普通单位。[Linux ftrace `trace_clock`](https://docs.kernel.org/6.6/trace/ftrace.html#the-file-system)

Linux perf 同样允许采集者通过 `perf_event_attr.use_clockid/clockid` 或 `perf record --clockid` 选择时间字段的 clock；支持集合包括 MONOTONIC、MONOTONIC_RAW，并视事件支持 BOOTTIME、REALTIME、TAI。这个能力的目的之一就是让 perf 样本能够与其他工具的 timestamp 对齐。[`perf_event_open(2)` clockid](https://man7.org/linux/man-pages/man2/perf_event_open.2.html)、[`perf-record --clockid`](https://man7.org/linux/man-pages/man1/perf-record.1.html)

ftrace/perf 提供的是“采集时选对并记录所选 clock”的低层机制，不是通用的离线多时钟换算系统。消费者如果丢掉选中的 clock，只剩一个 `u64`，其语义无法恢复。

### 2.4 Chrome Trace Event/Catapult：用 sync marker 合并 trace，现代路径交给 Perfetto

旧 Chrome Trace Event/Catapult 通过 phase `c` 的 `clock_sync` marker 关联不同 trace 的 clock domain。issuer 记录 `issue_ts` 和接收时 timestamp，receiver 记录同一个 `sync_id`；Catapult importer 先收集这些 marker，再交给 clock sync manager 对齐模型。[Chrome clock-sync producer interface](https://chromium.googlesource.com/catapult/+/88390a179e95252699e4f04790381e7159d29997/common/py_trace_event/py_trace_event/trace_event.py)、[Catapult TraceEvent importer](https://android.googlesource.com/platform/external/chromium-trace/+/nougat-mr1.3-release/catapult/tracing/tracing/extras/importer/trace_event_importer.html)

当前 producer 源码还明确指出：使用 proto format 且预先写入 clock snapshot 时，旧 marker 不再生效，同步由 Trace Processor 的 ClockSnapshot 机制完成。这反映了业内演进方向：把一次性的、格式专属 sync marker 收敛为有 clock identity、可重复采样、由统一 importer 消费的模型。[Chrome `clock_sync` documentation](https://chromium.googlesource.com/catapult/+/88390a179e95252699e4f04790381e7159d29997/common/py_trace_event/py_trace_event/trace_event.py)

## 3. 共性、差异和不能照抄的部分

| 系统 | 原始输入如何表达时间 | 换算发生在哪里 | 普通查询看到什么 | 关键限制 |
| --- | --- | --- | --- | --- |
| Perfetto | packet clock id + repeated ClockSnapshot | Trace Processor import | 一条 primary trace-time `ts` | 正确性依赖足够的 snapshot；检测非单调 source |
| OpenTelemetry | wire format 统一 Unix epoch；区分 event/observed role | producer/SDK/Collector 进入 OTLP 前 | epoch timestamp、duration | 不提供任意本地 clock graph；epoch 不等于有序或无 skew |
| Linux ftrace/perf | 会话选择 `trace_clock`/`clockid` | 采集端选 clock，或更高层消费者处理 | 原始 timestamp | 单独的整数不能说明 domain；`local` 可能跨 CPU 不同步 |
| Chrome Trace Event | trace-local timestamp + clock sync marker | Catapult import；现代 proto 转向 Perfetto | 对齐后的 model time | 旧 marker 模型信息量有限，正在被 ClockSnapshot 取代 |

共同原则有四条：

1. **clock domain 是采集事实，不是字段命名惯例。**
2. **同步证据与事件值分开保存。** snapshot/marker 不逐行重复。
3. **换算由平台集中拥有，不由每个查询作者重复实现。**
4. **只有证据足够时才发布统一时间轴；否则保留 domain 与原始读数，不能伪造可比较性。**

系统之间真正不同的是证据强度与产品承诺。Perfetto 有重复 snapshot，因而可以处理 offset 随时间改变；OTel 直接要求 source 在进入协议前给出 epoch；ftrace/perf 只保证选中的采集 clock。KAT 不声称具有 Perfetto 的漂移修正能力，但可以选择信任 HiProfiler 提供的初始映射并明确把后续变化风险交给使用者；同时仍保留原始 domain、value 与 snapshot，使这个假设可检查而不是不可逆地写进唯一时间列。

## 4. HiProfiler 的 snapshot 密度核验

### 4.1 官方源码中的触发条件

ftrace plugin 的 `ParseBasicData()` 每次被调用都会执行 `ReportClockTimes()`，后者顺序读取 REALTIME、REALTIME_COARSE、MONOTONIC、MONOTONIC_COARSE、MONOTONIC_RAW 和 BOOTTIME 六个 clock，写成一组 `clocks_detail`。[basic-data 与 clock report](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp#L181-L199)、[六种 clock 的读取](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp#L769-L797)

`StartCapture()` 在启动采集时调用一次 `ParseBasicData()`。普通 event polling loop 不按周期调用它，只在 `isReportBasicData_` flag 被置位时再次调用；该 flag 来自 plugin 的 `onReportBasicDataCallback`。[启动调用](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp#L245-L255)、[poll loop 条件调用](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp#L328-L341)、[callback](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/ftrace_module.cpp#L57-L65)

当前官方 service 源码中可见的生产调用路径是在 split-file writer 要切分文件时 refresh plugin sessions；新 split file 同时重建自己的 header time source。也就是说，`clocks_detail` 可能因文件切分再次出现，但不是普通 basic-data polling 的固定周期采样。[split 时 refresh](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/services/plugin_service/src/plugin_service.cpp#L507-L521)、[新 split file 重建 time source](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/services/profiler_service/src/trace_file_writer.cpp#L339-L366)

因此，源码可以证明“开始时一次、文件切分时可能再次产生”，不能证明类似 Perfetto 的固定 cadence。即使未来还有未覆盖的外部 caller，KAT 也不能把未声明的周期性当成格式保证。

### 4.2 真实样本

本地样本：

- 文件：`hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace`
- 大小：159,513,402 bytes
- SHA-256：`da6877da3f24db1e4754b9f06bcfb35830fb1fffc2ae827ee306548f2cf9f4b9`
- 一个 profiler section，26,338 个 `ProfilerPluginData` envelope，其中 26,337 个为 `ftrace-plugin`
- 只有 **1 个** ftrace payload 含 `clocks_detail`，其中正好有上述六个 clock reading；它位于第 2 个 envelope
- 文件头另有一组同类六 clock reading；header BOOTTIME 为 `909080460000 ns`，ftrace `clocks_detail` BOOTTIME 为 `909341510313 ns`
- ftrace event 大约覆盖 `909695938750..939094278906 ns`，所以两组锚点都在事件区间开始之前，约 29.4 秒的事件区间内没有后续校准点

统计方法是按 `.htrace` 的 1024-byte header 与 length-prefixed `ProfilerPluginData` framing 遍历 envelope，再按官方 protobuf wire fields 统计 `TracePluginResult.clocks_detail`；没有根据 payload 大小或 timestamp 数值猜测。

这个样本不能证明所有 HiProfiler 版本永远只有一组，但足以否定“当前正常采集已经像 Perfetto 一样提供周期性 snapshots”的假设。结合源码触发条件，KAT 第一版把 HiProfiler 证据视为稀疏起始锚点，而不是 piecewise calibration series；产品策略仍接受该锚点用于整个 trace segment，只是不宣称能够发现或修正后续 offset 变化。

## 5. 面向用户的 KAT 选择

### 5.1 统一值结构，不统一原始单位

推荐把以下条件设为 Data Import 的成功不变量：

> 每个已支持的事件时间都能还原为完整的 `UnifiedClock { ClockDomain, ClockValue }`；KAT 不在缺少证据时把不同来源读数伪装成公共纳秒时间。

事件表将这个逻辑值平铺为：

```text
clock_domain: Utf8 NOT NULL
clock_value: UInt64 NOT NULL
```

`ClockValue` 是对应 domain 上的非负原生读数。`tv_sec/tv_nsec` 可以在不改变数值语义时无损规范化为纳秒整数，但值类型本身不把单位固化成纳秒。Linux 的 jiffies 说明未来的内核时钟读数可能依赖采集侧 `HZ`，不能靠列名猜单位；当前 Hitrace 证据没有把任何受支持 event time 证明为 jiffies，因此第一版不接受该 `clock_type`，只保留能够以后扩展固定整数频率而无需改变 Schema 的数据模型。[Linux time conversion helpers](https://docs.kernel.org/driver-api/basics.html)

计量方式、频率和时钟实例身份属于 `ClockDomain` 定义，在 Dataset 中的普通 `clock_domain` Source table 保存一次。该表只有 `clock_domain: Utf8`、`clock_type: Utf8` 与 `ticks_per_second: UInt64` 三个非空字段；identity 在 Dataset 内唯一，频率必须大于零。第一版 type 只接受 `boottime`、`monotonic`、`monotonic_coarse`、`monotonic_raw`、`realtime`、`realtime_coarse`、`ftrace_global`、`ftrace_local`，当前全部使用 `1_000_000_000` ticks per second；不设 jiffies、`unknown`、`other` 或 `custom`。Datasource 为 domain 分配 lowercase snake_case 的稳定完整名称：单例使用同名 domain，确有多个 scope 时才使用 `ftrace_local_cpu_3` 等必要后缀。Dataset 本身已经限定设备、采集和启动实例，名称不再编码 UUID、设备、时间、启动 ID 或频率。事件行只携带 domain 引用和 value，不逐行重复 `unit`、`hz` 或 tagged JSON object。它复用 `tables/clock_domain.parquet`、Dataset inspection 和 DataFusion 的既有表机制，不增加根级 catalog 或容易在派生时丢失的 field metadata。跨域换算仍返回目标 domain 的 `ClockValue`；只有同域两点经过业务校验后的差值才叫 `duration_ns`，只有 Unix epoch 墙上时间 domain 的结果经严格 cast 后才形成 `Timestamp(ns, UTC)`。

### 5.2 Import 只验证来源事实完整，不要求预先统一

第一版私有导入模块采用以下策略：

- ftrace/Hitrace 把整份输入中的所有非空 `FtraceCpuStatsMsg.trace_clock` 视为同一会话的重复报告：规范化后必须唯一且受支持，并应用于报告前后的全部事件；不按 packet、位置或 CPU 分段，也不采用 last-wins，`local` 只在 domain identity 中按事件 CPU 拆分；
- 有 ftrace/Hitrace 事件却没有有效报告，或报告未知、冲突时失败；没有这类事件时不要求报告；Native Hook 和其他已支持 stream 同样必须有明确且无冲突的 domain 证据；
- 来源编码必须能无损、无歧义地形成当前 domain 的非负 `u64 ClockValue`；
- 每个被引用的 domain 都必须有足以解释其 value 的定义；
- 多个已确认 domain 可以共存，不因为暂时没有跨域映射而拒绝整个 capture；
- `ftrace_local` 等带 CPU scope 的时钟必须把 scope 纳入 domain identity，不能只保存字符串 `local`；
- envelope/report observation time 只在已知插件规则内用于一致性校验和诊断，不冒充 payload event time；第一版不发布缺少事件关联的 observation table，也不复制进事件表，完整原值仍留在用户保存在覆盖目标之外的 `.htrace`；
- 未知 domain、RMQ 时间语义未证实、非法编码或来源 metadata 冲突仍立即失败。

文件头和 `clocks_detail` 继续进入 `clock_snapshot` Source table。Schema 只有非空 `snapshot_id: UInt64`、`clock_domain: Utf8` 与 `clock_value: UInt64`，并保证 `(snapshot_id, clock_domain)` 唯一。Group 边界只来自原始容器：文件头六个 reading 是一组，每个独立 `TracePluginResult` 的非空完整 `clocks_detail` 列表各是一组，空列表不产生 group；不跨 payload 合并，也不逐 reading 拆分。Datasource 按 group 的文件出现顺序从零分配 ID，正常 header 是 ID 0；同组规范化后重复 domain 使 Data Import 失败。第一版只使用 `snapshot_id = 0` 作为全 Dataset baseline，来源与目标 domain 必须共同出现；KAT 把其中关系作为常量沿用到整个当前 trace segment。后续 group 只保留，不参与换算，也不因显示漂移而使操作失败；不同 group 不拼接多跳。目标 domain 定义或 baseline reading 缺失、重复、冲突时，具体操作失败并报告拥有和缺少的事实。Import 不因暂时无法换算而丢掉其他仍可分析的来源事件。

## 6. 最小领域模块与存储 Seam

### 6.1 Seam 与职责

```text
Hitrace Datasource Adapter
  ├─ event stream -> confirmed ClockDomain
  ├─ event encoding -> checked ClockValue
  ├─ domain evidence -> Dataset domain definitions
  └─ header/clocks_detail -> ClockSnapshot facts
                    │
                    ▼
       Dataset writer -> clock_domain + clock_value
```

Hitrace Adapter 只解释协议事实：哪个字段是 event time、哪个只是 envelope observation time，如何确认 clock identity，以及来源编码如何无损形成 `ClockValue`。Envelope observation 没有当前分析任务所需的统一事件关系，且其合法 ClockId 超出首版封闭时钟集合，因此 Adapter 只在已知插件路径中用它校验一致性，不交给 Dataset writer。Dataset writer 只接收已经完整的领域值和 domain 定义，不选择公共原点，也不执行未经请求的跨域换算。HiProfiler 协议变化留在 Adapter，物理 Arrow/Parquet 细节留在 writer，形成清晰的 Locality。

### 6.2 领域值

```rust
struct ClockValue(u64);

struct UnifiedClock {
    clock_domain: ClockDomain,
    clock_value: ClockValue,
}
```

`ClockDomain` 不是只有 `BOOTTIME` 或 `REALTIME` 的枚举标签，而是 Dataset 内一个具体时钟坐标的身份。Dataset 边界已经提供设备、采集和启动 scope，domain 名只在一个 Dataset 内区分实际存在的坐标；CPU-local 等同一 Dataset 内的必要 scope 才进入名称。两个值都写着 BOOTTIME，不足以证明它们属于同一时钟实例，跨 Dataset 的同名 domain 也不自动相同。定义使用普通 `tables/clock_domain.parquet`，事件列通过 `clock_domain` 值引用对应行。

### 6.3 首版只做同频平移

首版八种准入 `clock_type` 都固定为每秒十亿 tick。设事件来源读数为 `source_value`，`snapshot_id = 0` 中的来源与目标读数分别为 `source_base` 和 `target_base`：

```text
if source_value >= source_base:
    target_value = target_base + (source_value - source_base)
else:
    target_value = target_base - (source_base - source_value)
```

KAT 使用 PyArrow 批量 checked integer kernels 完成差值和平移，不经过浮点数或 Python per-row object；Arrow 的 `_checked` 算术会在整数溢出时报错，而不是回绕。[PyArrow compute functions](https://arrow.apache.org/docs/python/api/compute.html) 相同具体 domain 恒等返回；跨 domain 只做这一次直接换算，不拼接多跳。

缺少定义或 baseline reading、频率不是首版固定值、目标结果小于零或超过 `u64` 时，任何一行都会使整个操作失败，不返回原值、NULL 或部分结果。第一版不实现异频缩放、`u128` 乘除或舍入；新的真实 Datasource 证据要求准入不同频率时再单独设计，不提前把未来算法带进当前发布闭包。

NULL 只表达关系上的缺席：来源 `clock_domain` 与 `clock_value` 同时为 NULL 时结果为 NULL，使 LEFT JOIN 的未匹配行不需要额外 CASE；恰好一个为 NULL 则破坏 `UnifiedClock` 值对并使整个查询失败。两个输入都非 NULL 时，未知 domain、证据缺失和越界仍严格失败。目标 domain 必须是精确类型的非空 Python `str`；空字符串、`None`、其他类型和 `str` 子类都不属于 Interface。

Context 在构造私有 UDF Expr 时把两个来源 Expr 严格 cast 为 Arrow
`Utf8 clock_domain` 与 `UInt64 clock_value`，让零行和非零行关系服从同一规划合同。
DataFusion 能安全转换的 `LargeUtf8`、`Utf8View` 与非负有符号整数可以使用；负数、
越界、非法文本或其他不安全转换使整个 Workflow 失败，不用 `try_cast` 降级为 NULL。
KAT 明确保证规范 `Utf8`/`UInt64`、`LargeUtf8`/`Utf8View` domain 与可表示的非负
`Int64`；其他来源类型即使能被固定版本引擎转换，也不属于 Pack Authoring Interface。
`target_domain` 仍在构造 Expr 前由 Pack Authoring API 直接验证为精确的非空 Python
`str`，不依赖 DataFusion 的隐式转换。DataFusion 54 Python API 没有可靠的
schema-aware planning callback，因此 KAT 不再用行级 `arrow_typeof(...)` 值模拟精确
来源物理类型门禁；见 ADR-0054。

### 6.4 单一 Pack Authoring API 入口

DataFrame Workflow 通过当前 Execution Lease 上的薄入口构造私有 UDF Expr：

```python
ctx.convert_clock(
    col("clock_domain"),
    col("clock_value"),
    target_domain="boottime",
)
```

入口返回目标 domain 下的 `UInt64 ClockValue`。KAT 不返回 Struct、不逐行重复目标 domain，也不公开 `ctx.udf(name)`；需要发布结果时由 PACK 使用 `boottime_clock_value` 等自说明名称。`target_domain` 必须是 Datasource 发布的精确 domain identity；`clock_type` 不充当 alias，KAT 不做大小写转换、模糊匹配或唯一同类型自动选择，找不到时列出实际 domain 后失败。调用者不提供 snapshot、频率或 Dataset 路径。

首版用 Workflow Runtime 私有的 `stable` Python/PyArrow scalar UDF 实现。回调始终接收和返回 Arrow batches，并只调用 PyArrow compute kernels；不使用 `.as_py()`、Python 逐行循环、PyO3、FFI capsule、自建 native wheel 或 Rust 平行实现。该 UDF object 不向 `SessionContext` 注册；SQL 中调用 `kat_convert_clock(...)` 按 DataFusion 普通未知函数失败。DataFusion 54 Python API 没有可靠的规划期字面量校验入口，因此 SQL UDF 延后到 API 提供可靠规划回调之后，见 ADR-0051。只有代表性 trace 的真实性能证据证明批量实现成为关键瓶颈时，才重新评估下沉，不预建双实现或 port abstraction。

换算到目标 domain 并不等于得到 UTC。第一版直接由封闭的 `clock_type` 承担 origin 契约：只有 `realtime` 与 `realtime_coarse` 表示 Unix epoch 墙上时间，后者只是更低精度；PACK 才能继续使用 DataFusion 现成的严格 [`arrow_cast`](https://datafusion.apache.org/user-guide/sql/data_types.html) 派生 `Timestamp(ns, UTC)`。越界或非法转换使查询失败，不用 `try_cast` 静默产生 NULL。其他 domain 即使也是每秒十亿 tick，也不强求能够显示为公历时间。KAT 不增加 `origin`、`is_unix_epoch`、wall-clock UDF 或 Context 方法。

不同 domain 的比较、排序、Join 与相减先把两边显式换算到同一个 target domain，再复用 DataFusion 原生运算符。KAT 不再增加 compare、join、diff 平行函数，也不承诺拦截裸 `clock_value` 运算：选择平铺的 Arrow `UInt64` 后，DataFusion 不携带其领域类型；根据列名或不完整血缘猜测意图既可绕过又会误伤合法整数计算。未经换算的表达式可能正常执行，其时间语义属于 PACK 的测试和 review 责任。

形成 `duration_ns` 还要求 target domain 的 `ticks_per_second` 恰好为 `1_000_000_000`。PACK 决定业务起点与终点，显式检查终点不早于起点且差值能装入 `Int64`，再用 DataFusion 原生算术产生结果。第一版不提供 diff/Duration UDF，不对负差取绝对值、交换两端或返回 NULL；没有可到达纳秒 domain 时只保留原始 tick，不伪造 KAT Duration。

时钟表不是所有 Dataset 或 Workflow 的启动前提。Runtime 在 Workflow execution plane 创建时构建一个私有的空 Resolver，但只有在实际执行换算时才从当前 Resolved Dataset 读取并验证 `clock_domain`，随后在本次执行面内复用已加载证据；它不建立磁盘缓存或跨进程状态。未提供 Dataset 或缺少时钟证据时，实际换算失败，但未执行换算的 Workflow 仍可运行。合法 domain 定义即使对恒等换算也必需，snapshot 则只在实际跨 domain 时要求。Runtime 的内部读取不加入 Workflow Table Grant，PACK 直接查询时仍按普通 Required tables 约束。

### 6.5 当前 Interface 不变量

1. `UnifiedClock` 始终只是 `{ ClockDomain, ClockValue }` 的不可变值；
2. 来源事件表总是同时发布 `clock_domain` 与 `clock_value`；
3. `clock_value` 不携带单位，不能脱离对应 domain 定义解释；
4. 时钟类型与固定整数频率在每个 Dataset 的 `clock_domain` Source table 中保存一次，不逐行重复；
5. 多个已确认 domain 可以导入；未知或冲突 domain 仍使对应受支持输入失败；
6. 换算结果使用带目标 domain 的 `clock_value` 名称；只有经过业务校验的实际时长才使用 `duration_ns`，Unix epoch 墙上时间经严格 cast 后使用 Arrow `Timestamp(ns, UTC)`；
7. raw source 与 `clock_snapshot` facts 不被覆盖，第一版换算只使用 `snapshot_id = 0`；
8. `REALTIME` 可以作为合法 domain，KAT 不检测、修正或担保其单调性；
9. 来源与目标 domain 必须共同出现在 baseline，不拼接不同 snapshot group 做多跳；定义或 baseline reading 缺失、重复、冲突时失败，不猜测、不返回原值、不降级为 NULL。
10. 裸 `clock_value` 仍是普通 `UInt64`，KAT 不声称识别或阻止任意表达式中的跨域误用；受支持的跨域语义先显式换算到同一 target，再使用 DataFusion 运算符。
11. `duration_ns` 只由同一纳秒 target 上经 PACK 验证为非负且落入 `Int64` 的差值产生；KAT 不增加 Duration UDF 或隐式修正。
12. 每个 execution plane 创建一个私有内存 Resolver，时钟证据只在换算实际使用时加载；缺表不影响未使用换算的 Workflow。
13. 时钟换算只产生目标 `ClockValue`；第一版只有 `clock_type` 为 `realtime` 或 `realtime_coarse` 时，PACK 才用 DataFusion 严格 cast 派生 `Timestamp(ns, UTC)`，KAT 不增加 origin 字段或专用 UDF。
14. 时钟换算只通过 `ctx.convert_clock(...)` 暴露；SQL 中的 `kat_convert_clock(...)` 未注册并按未知函数失败。
15. 换算入口把来源 Expr 严格 cast 为 Arrow `Utf8 + UInt64`，安全 coercion 可用，
    不安全转换使 Workflow 失败；target 仍只接受精确的非空 Python `str`。
16. 首版只有一个 Runtime 私有的 `stable` Python/PyArrow batch UDF 实现，不逐行转 Python object，也不预建 Rust/FFI 平行实现；只有真实性能证据才触发重新评估。

### 6.6 最小测试矩阵

- 合法 `tv_sec/tv_nsec` 无损形成纳秒计量的 `ClockValue`，非法范围和 `u64` 溢出失败；
- `clock_type` 只接受首版八个值且频率为 `1_000_000_000`；jiffies、未知值和无法确认的 RMQ 时间拒绝；
- `clock_domain` 定义拒绝空值、重复 identity、未知 clock type、零频率和无法用固定整数频率解释的来源；
- domain identity 拒绝非法格式，Hitrace 来源缩写稳定映射为完整名称；大小写变体、clock type 和不存在的 target 不会被隐式解析；
- boottime 与 realtime event stream 同时出现时可以导入，并各自引用正确 domain；
- `ftrace_local` 的不同 CPU 不会被错误合并为一个 domain；
- 同一 stream 出现冲突 clock、Native Hook 配置缺失或未知/RMQ domain 时失败；
- 一致的 `trace_clock` 重复报告被接受并覆盖报告前后的事件；缺失、未知、冲突、packet-local、CPU-local 与 last-wins 解释不被接受；
- domain 定义缺失或与来源编码冲突时失败；
- 文件头和 `clocks_detail` 原样成为 snapshot facts，不自动改变事件读数；
- header 与每个非空完整 `clocks_detail` 列表分别形成 group，按文件顺序从零分配 snapshot ID；不跨 payload 合并或逐 reading 拆分，同组重复 domain 失败，后续 group 即使显示漂移也不改变 baseline 换算；
- 同 domain 换算恒等；跨 domain 按 baseline 做同频 checked 平移，不实现异频缩放或舍入；
- 换算结果小于零或超过 `u64` 时整批失败，不发布 NULL 或部分结果；
- 来源 domain 与 value 同时为 NULL 时传播 NULL；恰好一个为 NULL 时整批失败；target 为 NULL 或空字符串时拒绝；
- 零行和非零行的 `LargeUtf8`/`Utf8View`、非负有符号整数等安全 coercion 与规范
  `Utf8 + UInt64` 输入结果一致；负数、越界、非法文本和其他不安全转换失败；
- `ctx.convert_clock(...)` 正常构造换算 Expr；空字符串、`None`、非字符串和 `str` 子类在构造 Expr 前失败；SQL 直接调用 `kat_convert_clock(...)` 按未知函数失败；
- `duration_ns` 派生覆盖正常零值与正值、终点早于起点、超过 `Int64` 和不存在纳秒 target；后三者不能被自动修正为 Duration；
- 未调用换算时缺少时钟表不影响 Workflow；调用后同域只要求合法定义，跨域再要求 baseline，Resolver 在同一 execution plane 内只创建一次并按需加载证据；
- Import 失败不会发布部分 Dataset。

## 7. Trade-offs 与被拒方案

### 选择的代价

- `clock_value` 不是可以脱离 domain 直接显示成秒数的通用时间；普通分析需要 KAT 提供受约束的时间操作。
- 同一 domain 字符串可能在许多事件行重复，但不再逐行重复单位、频率和换算证据；列式编码可以压缩重复值。
- KAT 使用初始 snapshot 计算出的映射可能在 segment 后部因 suspend、NTP 或手动校时偏离真实关系，且第一版不会检测或修正。
- DataFusion 无法从普通 `UInt64` 自动恢复 `ClockDomain` 语义；KAT 提供并严格验证正确的显式换算路径，但不会可靠拦截 PACK 绕过该路径的裸数值运算。

这些代价换来一个清晰事实边界：Dataset 不丢失时钟来源，KAT 集中解释 `HZ`、snapshot 和 offset，普通 PACK 不重复实现换算。初始锚点被定义为当前产品可接受的证据，而不是底层时钟永远稳定的证明；底层时钟单调性、映射漂移和数据质量由使用者负责。

### 拒绝：把所有来源读数强制叫 `timestamp_ns`

这对 `clock_gettime` 的纳秒结果看似自然，却会把 jiffies、硬件 counter 等其他计量方式错误地重新贴标签。导入时统一换算还会要求 KAT 在证据不足时提前选择目标 domain、精度和舍入规则。只有真实换算结果才使用单位后缀。

### 拒绝：每行增加 `unit` 或 `hz`

单位与频率是 domain 定义，不是每个 event 的业务变化。逐行保存会重复占用存储、查询列和模型上下文，同时允许同一 domain 行之间出现互相矛盾的参数。把它们在 Dataset 中定义一次，既保留信息量，也减少用户负担。

### 拒绝：把初始映射描述成稳定性保证

Linux 的 BOOTTIME 会累计 suspend、MONOTONIC 会受渐进校时影响而 RAW 不会、REALTIME 还可能发生不连续调整，所以初始 offset 在理论上能够变化。[Linux `clock_gettime` 时钟语义](https://man7.org/linux/man-pages/man3/clock_gettime.3.html)、[Linux kernel timekeeping](https://docs.kernel.org/5.15/core-api/timekeeping.html) KAT 仍选择接受 HiProfiler 的初始 snapshot，但文档、Schema 和错误不能把这项风险策略写成“整个 segment 已被证明稳定”；原始值与 snapshot 必须保留，且不承诺漂移检测或修正。

### 拒绝：现在实现通用 clock graph

Perfetto 的 BFS、sequence-scoped/global clocks、跨 trace/machine scope 都解决了 KAT 当前尚未证实的问题。KAT 当前只为已准入的同频时钟提供使用 `snapshot_id = 0` 的严格 baseline 平移，并通过 `ctx.convert_clock(...)` 暴露；不搜索路径、不拼接多跳，也不预建完整图引擎。以后只有真实来源引入异频或多段映射需求时，才重新设计相应操作。

## 8. 推荐决策

从用户和架构两方面，推荐用以下一句话概括当前选择：

> `UnifiedClock` 是 `{ ClockDomain, ClockValue }` 组成的不可变值。`ClockValue` 保留来源时钟的非负原生读数，domain 定义在普通 `clock_domain` Source table 中保存一次；事件表平铺 `clock_domain + clock_value`，不逐行重复单位，也不预造一份公共 `timestamp_ns`。同一 trace segment 的 `snapshot_id = 0` 是有效的常量跨域映射锚点，后续 snapshot 只保留来源事实，不参与换算；KAT 不跨 group 拼接多跳，不要求周期校准，也不检测或修正后续漂移。换算结果仍以目标 domain 的 `ClockValue` 表达；只有合格差值才使用 `duration_ns`，只有 Unix epoch 墙上时间经严格 cast 后才形成 `Timestamp(ns, UTC)`。定义或 baseline reading 缺失、重复、冲突时操作失败。

这个模型不承诺不同 Dataset、不同设备、不同启动或 Dataset 重建前后的同名 domain 自动一致。KAT 也不为此持久化 generation 或全局时钟身份；需要跨边界对齐时必须有额外证据。

这保留了原始证据和明确失败边界，同时采用符合当前 OpenHarmony 移动设备经验的风险策略。Runtime 只通过 `ctx.convert_clock(...)` 为 DataFrame Workflow 构造私有 UDF Expr，不向 SQL 注册函数，也不泄露 snapshot、频率或底层 SessionContext。首版实现停留在 PyArrow batch library 层；SQL UDF 等 DataFusion Python API 提供可靠规划期校验入口后再重新设计。
