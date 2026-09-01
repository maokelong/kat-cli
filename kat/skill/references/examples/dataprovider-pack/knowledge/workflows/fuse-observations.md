# Fuse Observations 分析策略

这个 Workflow 已把 telemetry Database 的线程观测、control Database 的进程名和本地
`thread_placement.parquet` 按业务键融合。先按 `process_name`、`thread_id` 和 `cpu`
汇总 `cpu_usage`，再沿 `clock_value` 检查热点是否持续、迁核或集中在单个 CPU。

结果发散时区分三类信号：单线程持续升高通常需要查看该线程的调用或调度证据；同进程
多线程同时升高应进一步看进程级工作负载；同一 CPU 上跨进程同时升高则优先补充调度、
中断或频率证据。`clock_value` 只有配合 `clock_domain` 才能和其他 trace 对齐。

结果为空时依次检查远端窗口是否有 observation、两个 Database 的 registry 键是否完整、
本地 placement 是否包含相同 `thread_id`。不要把 INNER JOIN 后的空结果直接解释为来源
都没有数据；可分别运行单源 Workflow 或 Provider 查询定位是哪条关联边缺失。
