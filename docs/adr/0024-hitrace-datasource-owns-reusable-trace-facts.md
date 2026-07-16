---
status: accepted
---

# Hitrace Datasource 拥有可复用 Trace facts

Hitrace Datasource Module 在 Data Import 中同时生成直接事件表，以及需要跨记录规范化但可被多个 PACK 复用的 Trace facts，并通过 Dataset mutation Interface 写入 Dataset；这些表的名称和 Schema 由 Hitrace 自身设计，不从过渡性的 Trace Streamer 路径推导。PACK 只消费这些事实形成领域分析，不以隐藏预处理 Workflow 或 Run Output 生成共享输入；Hitrace 只在至少产生一行事实时创建对应表，不为已知 Schema 生成零行占位，因此具体 Workflow 是否可运行可以继续只由 Required tables 判断。

“可复用”必须由当前多个真实消费者证明，不能只因一种跨记录计算理论上可能再次出现就提前增加 Dataset 表。首个 `kat-kernel/thread-cpu-time` Workflow 直接使用带明确 `cpu_switch_sequence` 的 `sched_switch` Source table，并复用 DataFusion window functions 计算相邻 switch 的可观测 CPU 时间；当前不增加 `thread_running_interval`、`sched_slice` 或其他平行事实表。以后真实重复出现后再把已经稳定的共同语义下沉，不让 Datasource 为单个 PACK 提前拥有分析策略。
