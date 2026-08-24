---
status: accepted
---

# 首个 Bundled PACK 分析线程 CPU 时间

> ADR-0062 已从这个长期 Workflow 的 decorator 删除 `required_tables`；`sched_switch` 依赖由其调用的 Analysis Module 实际查询表达。ADR-0063 只先交付 `kat-kernel/hitrace` Source 并删除两个 Trace Streamer 预发布 PACK，不把旧 Demo 冒充为本决定的长期分析交付；`thread-cpu-time` 仍须在后续切片按本文事实与验证合同实现。本文其余问题、Workflow、Output 与 PACK 所有权决定继续有效。

KAT 的第一条长期用户闭环回答一个明确问题：**这段 Trace 中哪些非空闲线程占用了最多 CPU 时间，主要运行在哪些 CPU 上？** 首个 Bundled PACK 使用一级 name `kat-kernel`，manifest 初始展示字段为 `title = "Kernel Performance"`、`description = "Analyze kernel scheduling behavior from KAT Datasets."` 与 `owner = "Kernel Team"`。它按内核团队的稳定维护责任划分，不因为首个问题属于调度领域而建立更窄的 `kat-scheduling` PACK。

该 PACK 的首个 Workflow name 是 `thread-cpu-time`，title 是 `Thread CPU Time by CPU`，入口位于 `workflows/thread_cpu_time.py`。它没有用户参数，装饰器只声明 `required_tables=["sched_switch"]`；运行时返回单元素具名映射 `{"thread_cpu_time_by_cpu": dataframe}`，不使用含义过宽的默认 `main`。用户正常通过 KAT Skill 表达问题并由 Skill 自动选择；`kat run --pack kat-kernel --workflow thread-cpu-time --dataset <dataset>` 只是开发、测试和高级覆盖入口。

首版 Hitrace `sched_switch` Source table 只发布当前闭环需要、且足以完整表达一次线程切换的八个非空字段：

| column | Arrow type | meaning |
| --- | --- | --- |
| `clock_domain` | `Utf8` | 该事件使用的具体时钟域 |
| `clock_value` | `UInt64` | 该时钟域上的来源读数 |
| `cpu` | `UInt32` | 发生切换的 CPU |
| `cpu_switch_sequence` | `UInt64` | 该 CPU 上从零开始、按来源顺序连续分配的 switch 序号 |
| `previous_thread_id` | `Int32` | 被切出线程的调度 ID |
| `previous_thread_name` | `Utf8` | 切出时观测到的线程名称 |
| `next_thread_id` | `Int32` | 被切入线程的调度 ID |
| `next_thread_name` | `Utf8` | 切入时观测到的线程名称 |

`cpu_switch_sequence` 是当前查询正确性所需的来源事实，不是隐藏行号或未来扩展点。SQL relation 和 Parquet 文件都不承诺隐含行序，同一 CPU 的多个 switch 也可能具有相同 `clock_value`；没有显式来源顺序就无法确定相邻事件。Importer 必须按输入中每个 CPU 的实际事件顺序分配它，不能从时间值、Parquet 行位置或 DataFusion sort stability 重新推断。字段使用 `thread_id` 而不沿用来源的 `pid`，因为 `sched_switch` 记录的是被调度 task/thread 的 ID，不承诺它是进程 ID。priority、state、公共 event `tgid/comm` 与其他 sched event 不属于这条用户问题的最小输入，首版不为完整镜像而发布；原始 Hitrace 文件仍保持不变。

Workflow 对每个 `(clock_domain, cpu)` 按 `cpu_switch_sequence` 排序。当前行的 `previous_thread_*` 表示刚结束运行的线程，前一行的 `clock_value` 是它被切入后可观测区间的起点；因此只对存在前一行的事件计算当前 `clock_value - previous clock_value`。Importer 已经保证同一 CPU 的相邻 switch 时钟读数不倒退，Workflow 再把范围内的差值严格形成 `Int64` 纳秒 Duration。Hitrace 首版准入的 ftrace clock 都是每秒十亿 tick，所以同域、同 CPU 的差值可以在形成 Duration 后跨 CPU 汇总；这不允许直接比较不同 `ftrace_local_cpu_*` domain 的原始 `clock_value`。

Workflow 排除 `previous_thread_id = 0` 的 idle task，再按 `previous_thread_id`、`previous_thread_name` 与 `cpu` 聚合，只产生一张 `thread_cpu_time_by_cpu` Output：

| column | Arrow type | meaning |
| --- | --- | --- |
| `thread_id` | `Int32` | Source table 中观测到的线程 ID |
| `thread_name` | `Utf8` | 与该 ID 一同观测到的线程名称 |
| `cpu` | `UInt32` | 线程运行所在 CPU |
| `observed_cpu_time_ns` | `Int64` | 完整可观测 switch 区间的 CPU 时间总和 |

结果按 `observed_cpu_time_ns` 降序，再按 `thread_id`、`thread_name`、`cpu` 稳定排序。它不重复生成一张线程总量表；Skill 通过有界 Output Query 对这张已经聚合的小表再次求和即可得到跨 CPU 排名。它也不增加 top、时间窗口、include-idle 或百分比参数；这些要么属于查询投影，要么是出现真实不同用户问题后才应新增的 Workflow。没有任何完整非空闲区间时，Workflow 返回具有上述确定 Schema 的零行 Output，而不是失败或省略 Output。

`observed_` 是有意保留的诚实限定：每个 CPU 的首条 switch 之前已经运行的线程没有已知起点，最后一条 switch 切入的线程没有已知终点，两段都不虚构时长。结果只覆盖该 CPU 首条到末条 switch 之间能够闭合的区间，不声称等于完整采集时长。`thread_id + thread_name` 也只是调度器在事件中给出的观测标签，不是跨任意长 Trace 的稳定线程生命周期身份；rename 会自然分成不同记录，首版不为罕见的同名 ID 复用引入 fork/exit/rename identity system。

Hitrace Import 在发布 Dataset 前必须验证每个 CPU 的 switch 来源顺序与 `clock_value` 非递减，并验证相邻事件的前一条 `next_thread_id` 等于后一条 `previous_thread_id`；名称可以因线程重命名而不同。固定 OpenHarmony revision `c8cd47e52de5d01fbf37f00d176d7e9a87773a57` 中，[`StartCapture`](https://gitcode.com/openharmony/developtools_profiler/blob/c8cd47e52de5d01fbf37f00d176d7e9a87773a57/device/plugins/ftrace_plugin/src/flow_controller.cpp) 先发送 `TRACE_START` 统计，随后才清空 kernel trace buffer；`StopCapture` 则在采集停止后发送 `TRACE_END`。[Linux ring buffer reset](https://github.com/torvalds/linux/blob/v6.12/kernel/trace/ring_buffer.c) 会把 `overrun`、`commit_overrun` 与 `dropped_events` 清零，而 [ftrace 文档](https://docs.kernel.org/6.12/trace/ftrace.html) 明确把三者定义为不同方式造成的事件丢失。因此 `TRACE_START` 只提供协议与时钟事实，不参与本次采集的数据完整性判定，也不能与 `TRACE_END` 机械相减。

首版只接受单次 ftrace capture：只要存在首版支持的 ftrace event，就必须有且只有一个完整 `TRACE_END` snapshot，并覆盖所有出现于 `FtraceCpuDetailMsg` 的 CPU；缺失、重复 CPU 或相互冲突的结束统计都使 Import 失败。任一结束统计的 `overrun`、`commit_overrun` 或 `dropped_events` 非零，或者任一 `FtraceCpuDetailMsg.overwrite` 非零，同样证明本次采集不完整。Importer 必须整体失败并提示重新采集，不产生 best-effort `sched_switch` 表、warning table 或部分 CPU 时间；这些统计只用于准入，不发布成 Dataset table。真实 OpenHarmony fixture 仍必须分别覆盖零丢失成功和每类丢失证据失败，但实现者不再需要猜测 counter 是累计值还是区间值。

第一版不新增 `thread_running_interval`、`sched_slice` 或其他跨记录 Source table。当前只有一个真实消费者，而且 DataFusion window functions 已经能够清楚、可测试地完成相邻 switch 运算；现在把它提升为 Dataset Interface 是为想象中的复用预建表。以后出现第二个真实 Workflow 或 PACK 重复同一套区间语义时，再依据 ADR-0024 判断是否把它下沉为 Hitrace Trace fact，并以当时的消费证据设计名称和 Schema。
