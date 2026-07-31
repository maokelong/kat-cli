---
status: superseded by ADR-0058
---

# Trace 分析以 Perfetto 语义为基线

KAT 分析与 Perfetto 相同的调度概念、且来源事实足以表达该概念时，优先采用 Perfetto 已建立的领域名称、数据关系和算法语义，不再发明一套近义分类或近似遍历。调度分析以 `thread_state` 的 `io_wait`、`blocked_function`、`waker_id` 与 `irq_context`，以及 `thread_executing_span`、`wakeup_graph` 和 blocker 时间归因为参照；`blocked_function` 只属于实际阻塞的线程状态，不沿 waker 链复制。当前基线参考 Perfetto `0621e927` 的 [`sched.thread_executing_span`](https://github.com/google/perfetto/blob/0621e92721ff5cc329df07ca5bd1b763651d506f/src/trace_processor/perfetto_sql/stdlib/sched/thread_executing_span.sql) 与 [critical-path walker](https://github.com/google/perfetto/blob/0621e92721ff5cc329df07ca5bd1b763651d506f/src/trace_processor/plugins/critical_path/critical_path.cc)。

这是一项领域语义基线，不是对 Perfetto 私有下划线 API、Trace Processor、PerfettoSQL module loader 或 SQL 方言的运行时依赖。KAT 仍用现有 SQL、DataFusion、PyArrow 和普通 Python 实现当前最小能力，并以真实 Trace fixture 锁定行为；Perfetto 无法表达或 OpenHarmony 已确认不同的规则必须作为来源 Adapter 的显式差异保留，例如 Demo 的已知 I/O 工作线程和 `udk-irq` 边界，不得暗中扭曲通用模型。

这里不直接复用 Perfetto Runtime 有明确输入与执行边界原因。官方 Trace Processor Python API 从 trace 文件、字节流或既有 Trace Processor 实例建立自己的 C++ Trace Storage 与 PerfettoSQL 执行面；标准库模块也在该执行面上消费 Perfetto 表。当前 Demo 的输入却是已经由 Deprecated Trace Streamer 物化并由 KAT Dataset/DataFusion 注册的关系，不是可直接交给该 API 的 Perfetto trace。直接复用会要求把同一批事实再转换成 Perfetto 支持的 trace 输入，或者同时交付第二个查询引擎和一套跨引擎表适配，仍无法直接调用 C++ critical-path plugin 作为 KAT DataFrame 算子。首版因此只窄移植当前消费者需要的已公开语义并固定上游 revision，不复制 Trace Processor、PerfettoSQL loader 或完整标准库；若以后 Hitrace 能以官方支持的输入直接取得所需稳定结果，应优先删除这份移植而不是继续扩张。参见 [Trace Processor Python API](https://perfetto.dev/docs/analysis/trace-processor-python)、[Trace Processor architecture](https://perfetto.dev/docs/design-docs/trace-processor-architecture) 与 [PerfettoSQL standard library](https://perfetto.dev/docs/analysis/stdlib-docs)。

当前 `kat-openharmony-demo` 不只把旧图算法改成 Perfetto 风格的名字，而是以来源事实 Adapter、`thread_executing_span`、`wakeup_graph` 和按窗口裁剪的 blocker walk 替换旧遍历。旧 `max_depth`、删除短片段后重新连边、沿 waker 链复制 blocked caller、通用 `confidence`/`uncertainty`、合成终止节点和自创线程状态分类都不迁移；只有来源事实足以按上述语义形成真实归属时才输出，不足时必须明确失败，不用这些字段或无归属行伪造完整性。公开 Workflow 仍使用 `first-frame-scheduling-dependencies`，只有真实 OpenHarmony Trace 回归证明其满足关键路径语义后，才重新评估 `critical_path` 名称。

公开结果不暴露算法 graph。`scheduling_dependencies` 把所选 frame thread 与最终 blocker 各自的真实 `thread_state`、`io_wait` 和 `blocked_function` 投影到完整、互不重叠的时间归因区间；两侧事实不得互相复制。内部 span、节点/边 ID、depth、priority 和按区间重叠猜出的 callstack 不属于 Output。已有目标帧但事实不足以完整归因时操作失败，不以时间缺口、合成行或 best-effort 结果伪装成功。

精确 `process_name` 不存在与目标进程没有已完成、正持续时间的 actual frame 分别形成可读 Workflow failure；两者都表示没有取得分析目标，而不是“分析成功但没有依赖”。KAT 不发布 Run、零行 Output 或 `target_not_found` sentinel。查询可以在同一个有界结果中携带 process existence 与可选目标 frame，避免为了诊断建立另一套发现能力或错误码。
