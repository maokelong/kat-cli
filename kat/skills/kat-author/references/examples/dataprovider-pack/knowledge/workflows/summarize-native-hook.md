# Summarize Native Hook 分析策略

结果按 `event_type` 汇总 `event_count` 和 `total_heap_size`。先区分分配、释放等事件类型，
比较事件数与总大小是否同步变化；总大小高但事件数低通常意味着少量大对象，事件数高但
总大小有限通常意味着高频小对象。

这份汇总不能单独证明泄漏，因为它没有对象生命周期配对和时间趋势。发现可疑类别后，
下一步应在同一 Trace Streamer SQLite 中增加带时间、调用栈或对象标识的只读查询，再按
来源 schema 验证分配/释放关系。结果为空时先确认 Trace Streamer 产出的数据库包含
`native_hook` 且输入 trace 开启了对应采集能力；不要把成功解码等同于一定含有该表数据。

若要和 Ftrace 或 PostgreSQL 观测融合，先确认共同 clock domain，再让 Provider 返回
边界明确的 eager Table，最后在 DataFusion 中联合查询。
