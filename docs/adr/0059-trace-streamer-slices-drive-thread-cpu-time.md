---
status: accepted
---

# Trace Streamer 预发布 Demo 的线程 CPU 时间

本决定将 `kat-openharmony-thread-cpu-time/thread-cpu-time` 限定为依赖 Deprecated Trace Streamer SQLite 输入的预发布分析契约。它不替代 ADR-0048 和 ADR-0024：长期 `kat-kernel/thread-cpu-time` 仍必须以 Hitrace `sched_switch` 为唯一 Required table，按相邻 switch 形成完整可观测区间。

Demo 保持无参数入口、唯一 `thread_cpu_time_by_cpu` Output、按线程/名称/CPU 聚合以及稳定排序。`kat-openharmony-thread-cpu-time` 独立承担线程 CPU 时间分析契约和可信度门，但与 `kat-openharmony-critical-path` 共同承担 Deprecated Trace Streamer Datasource 的依赖闭包和退场责任，符合 ADR-0028；不得被描述为正式 `kat-kernel` 能力或长期 Hitrace 用户闭环。

本决定局部替代 ADR-0013、ADR-0021 和 ADR-0022 中 Trace Streamer “枚举并物化全部非系统表与 view”的 relation 范围，且只改变 Deprecated Trace Streamer Datasource。该 Datasource 现在只预检并物化 `main` schema 的非系统实体表，跳过 view。SQLite view 没有此入口可依赖的稳定来源声明类型，当前 Demo 也不消费它们；因此 Trace Streamer Dataset 不再承诺完整 SQLite 查询面。该规则适用于每次 Trace Streamer Import，不是为本 Workflow 设置的表白名单，也不改变 Hitrace 或其他 Datasource 的表契约。

Workflow 以 `sched_slice.itid = thread.itid` 关联线程：`itid = 0` 是 idle，`dur IS NULL` 是没有完整可观测 CPU 时长的 slice，均不计入结果。对其余行，`thread.tid`、`thread.name`、`sched_slice.cpu` 和 `sched_slice.dur` 分别形成 Output 的线程 ID、线程名、CPU 与 `observed_cpu_time_ns`。`dur` 是来源已经给出的时长；本切片不从 `ts` 推导时钟域、switch 顺序或新的区间边界，也不生成或伪装为 Hitrace `sched_switch` 或 `thread_running_interval`。

Workflow 在发布 Output 前严格将线程 ID 转为 `Int32`、CPU 转为 `UInt32`、时长总和转为 `Int64`；无法表示的来源值使 Run 失败，不能截断或改写。线程表缺失匹配或线程名的 slice 同样不计入，因为它们不能形成用户可解释的线程记录。名称是 Trace Streamer 维表在导入时提供的来源标签，不承诺重建历史 rename。

本决定不增加 top、时间窗口、include-idle、百分比、线程总量副本或第二个 Workflow。Skill 继续通过对已聚合 Output 的有界 Query 得到跨 CPU 排名和主要 CPU，Output 不承担该二次投影。

首次正式发布前，`kat-openharmony-thread-cpu-time` 与 `kat-openharmony-critical-path` 中依赖 Trace Streamer Datasource 的 Workflow 必须分别删除或迁移到对应的长期 facts；只有两个 PACK 都不再依赖该 Datasource 后，才可以移除 Datasource 及其 SQLite 读取依赖。线程 CPU 时间迁移必须满足 ADR-0048；不保留 alias、兼容期或把 Demo 提升为 `kat-kernel` 的迁移入口。
