---
status: accepted
---

# Trace 分析以 Perfetto 语义为基线

当 KAT 分析 Perfetto 已定义且来源事实足以表达的 Trace 概念时，以固定 revision `0621e927` 的 [`thread_executing_span`](https://github.com/google/perfetto/blob/0621e92721ff5cc329df07ca5bd1b763651d506f/src/trace_processor/perfetto_sql/stdlib/sched/thread_executing_span.sql) 与 [critical-path walker](https://github.com/google/perfetto/blob/0621e92721ff5cc329df07ca5bd1b763651d506f/src/trace_processor/plugins/critical_path/critical_path.cc) 为语义锚点，采用其领域名称、`wakeup_graph` 关系和 blocker 归因，并把 OpenHarmony 等来源差异显式留在 Adapter。当前输入已是 KAT Dataset/DataFusion 关系，直接复用 Perfetto Runtime 会复制存储、查询引擎和适配层，因此只窄移植真实消费者所需的公开语义；若将来能从官方支持输入直接取得稳定结果，应删除移植而非扩张它。Demo 只在来源事实足以形成真实、完整且互不重叠的 blocker 时间归因时输出，否则明确失败，不迁移旧 `max_depth` 等启发式或合成终止节点、不公开算法 graph、不以 best-effort 伪造完整性，也不在真实回归证明前把 Workflow 称为 `critical_path`。
