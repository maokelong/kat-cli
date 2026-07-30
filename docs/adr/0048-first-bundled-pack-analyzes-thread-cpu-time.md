---
status: accepted
---

# 首个 Bundled PACK 分析线程 CPU 时间

KAT 的首条长期用户闭环回答“哪些非空闲线程占用了最多 CPU 时间，主要运行在哪些 CPU 上”，由按内核团队长期所有权划分的 `kat-kernel` PACK 通过 `thread-cpu-time` Workflow 交付唯一 `thread_cpu_time_by_cpu` Output，其 Schema 精确只有 `thread_id`、`thread_name`、`cpu` 与 `observed_cpu_time_ns`；没有完整非空闲区间时返回同一 Schema 的零行结果。Importer 必须为每个 CPU 按来源顺序发布连续 `cpu_switch_sequence`，因为关系没有隐式行序且 `clock_value` 可以相同；Workflow 据此排序，只累计相邻 `sched_switch` 之间能够闭合的区间，首尾未知区间不虚构。存在首版支持的 ftrace event 时，Hitrace Import 只接受单次 capture，并必须证明时钟不倒退、线程切换连续且结束统计无丢失证据，否则整体失败而不发布 best-effort 数据；在出现第二个真实消费者前也不按 ADR-0024 把这套跨记录派生提前固化为新 Source table。
