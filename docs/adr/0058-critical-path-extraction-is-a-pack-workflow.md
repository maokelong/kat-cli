---
status: accepted
---

# 关键路径定位与抽取 Workflow 交付

本 ADR 替代 ADR-0050 中固定的 Workflow、Output 和遍历方案，并重新确立 Perfetto 调度语义基线与来源差异原则。`kat-openharmony-critical-path` 不再交付固定的首帧调度归因，而是公开 `locate-first-actual-frame` 和 `extract-critical-path`：前者发布可审计的 Frame Window；后者从根线程和窗口反向构造带层级、边关系、终止原因与不确定性的路径片段，并将同线程、时间裁剪后的调用栈证据独立输出。

关键路径 helper 保持 PACK 私有；状态、调度与 wakeup 事实负责建边，OpenHarmony Source Adapter 负责线程角色和基于精确规则的 Business Category。调用栈只在状态与调度已经确定的片段上按时间裁剪，不参与片段切分或依赖构造。已有真实片段时，Required table 中缺少匹配行或时间覆盖记录为 Path Uncertainty；根窗口完全没有 `thread_state` 时不存在可承载缺口的真实片段，Workflow 失败而不合成 sentinel。多条调度事实覆盖同一子区间属于事实形状冲突，同样失败而不伪装成覆盖缺失。缺少 wakeup、深度上限、循环或中断边界形成 Path Termination。这样既不把业务分类或调用栈重叠伪造为依赖，也不因单一 PACK 提前扩张平台 API。

`blocked_function` 只属于实际阻塞的线程状态，不沿 wakeup 链复制。精确进程不存在或目标进程没有已完成、正持续时间的 actual frame 时，定位 Workflow 失败且不发布 Run、零行 Output 或目标缺失 sentinel。来源事实足以表达 Perfetto 已有概念时沿用其领域名称、数据关系和算法语义；OpenHarmony 已确认不同的规则必须保留为 Source Adapter 的显式差异，不得暗中改写通用模型。

## 公开契约与兼容边界

`locate-first-actual-frame` 发布单行 `frame_window`；`extract-critical-path` 发布 `critical_path_segments` 与 `critical_path_callstack_evidence`。根线程窗口内的每个观测片段以 `relation_to_parent = root` 独立锚定一棵依赖子树；进入某个上游唤醒者的历史窗口后，`parent_segment_id` 指向下游、被当前行解释的片段，`relation_to_parent = wakeup` 只表示当前片段在精确边界直接唤醒该下游片段，`relation_to_parent = same_thread` 表示该上游窗口中同一线程的较早片段通向较晚片段。`termination_reason` 记录不能继续回溯的确定边界；`uncertainty_reason` 记录调度或调用栈等辅助证据的缺口，两者都不把结果表述为完整因果。

`critical_path` 在本契约中只表示给定窗口内依据可观测调度事实得到的有界依赖路径，不承诺完整因果或全局关键路径。`max_depth` 与 `min_segment_ms` 只在当前真实片段已经发布后停止继续向上游回溯，不删除观测片段、不重新连边，也不生成终止节点；同名参数不构成对旧遍历算法的兼容承诺。

`kat-openharmony-critical-path` 独立承担上述分析契约和可信度门，但与 `kat-openharmony-thread-cpu-time` 共同承担 Deprecated Trace Streamer Datasource 的依赖闭包和退场责任。正式发布前，只有两个 PACK 中依赖该 Datasource 的 Workflow 都已删除或迁移后，才可以移除 Datasource 及其 SQLite 读取依赖。

本预发布适配器只消费 Trace Streamer 已规范到主时间轴的 `ts` 与 `dur`；OpenHarmony Trace Streamer 的主时间轴默认为 BOOTTIME，并以纳秒表达相关 trace 时间，来源依据见[时钟证据记录](../research/openharmony-hiprofiler-clock-domains.md)。因此本 PACK 将这些字段解释为非负的 boottime 纳秒读数，并且只在同一窗口内以已确认的正时长形成 `duration_ns`；负读数、非正时长或 `Int64` 越界均使 Workflow 失败。该例外不形成 Hitrace 时间契约，迁移时必须重新使用目标 Datasource 的 clock-domain facts。

## 生态选型

本实现以 Perfetto revision [`0621e927`](https://github.com/google/perfetto/tree/0621e92721ff5cc329df07ca5bd1b763651d506f) 为调度语义基线，分别参照 [`sched.thread_executing_span`](https://github.com/google/perfetto/blob/0621e92721ff5cc329df07ca5bd1b763651d506f/src/trace_processor/perfetto_sql/stdlib/sched/thread_executing_span.sql) 与 [`critical-path walker`](https://github.com/google/perfetto/blob/0621e92721ff5cc329df07ca5bd1b763651d506f/src/trace_processor/plugins/critical_path/critical_path.cc)。OpenHarmony Source Adapter 是相对该基线的明确来源差异。更新 Perfetto 语义基线时，必须重新评估线程执行区间、wakeup 关系、路径遍历语义和 Source Adapter 差异；若输出契约或领域含义发生变化，应更新本 ADR 或建立新的替代决策。

本切片不直接复用 Perfetto Runtime：KAT 的输入是已导入并注册到 DataFusion 的关系表，而 Perfetto Python API 依赖自身的 Trace Storage 与 PerfettoSQL 执行面。直接接入会引入第二个查询引擎及跨引擎适配，仍不能把其 C++ critical-path plugin 直接作为 KAT DataFrame 算子。因此本切片复用现有 DataFusion、PyArrow 和 Python 边界实现；Perfetto 仅作为来源事实和调度语义的基线。若未来 Hitrace 能以 Perfetto 官方支持的输入直接取得所需稳定结果，应优先删除这份窄移植，而不是继续扩张。
