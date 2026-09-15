# Query Observations 分析策略

先确认查询窗口满足 `start_clock_value < end_clock_value`，并把 `clock_domain` 当作结果
时间列不可分割的语义。结果按 `clock_value, thread_id` 排序，包含线程、时钟值和
`cpu_usage`，适合检查一个远端 Database 内的原始观测变化。

分析时先按线程聚合 `cpu_usage` 的数量、均值、峰值，再检查峰值附近的连续观测。若只有
少数线程异常，下一步收窄时间窗口并关联线程/进程元数据；若所有线程同步变化，优先检查
采集窗口、时钟 domain 和系统级负载证据。空结果先核对 Database、窗口边界和来源是否
使用同一 clock domain，不把空表解释为“没有性能问题”。

需要进程名或本地 CPU placement 时，改用 `fuse-observations`，不要在分析层自行建立
远端连接或猜测跨源关联键。
