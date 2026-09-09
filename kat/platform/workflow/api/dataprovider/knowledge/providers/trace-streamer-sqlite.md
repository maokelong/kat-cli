# Trace Streamer SQL

使用 SQLite 方言；按实际数据库已有表和列编写查询，不假定所有采集都包含相同数据。需要确认表名时查询 `sqlite_schema`。

| 数据 | 常用表与关联 |
| --- | --- |
| 进程与线程 | `thread.ipid = process.id`；内部关联键与系统 `pid`、`tid` 不混用 |
| CPU 调度与线程状态 | `sched_slice.itid`、`thread_state.itid` 关联 `thread.id` |
| 同步与异步调用 | `callstack`；同步调用的 `callid` 关联线程，异步调用的 `callid` 指向进程，`child_callid` 指向线程，不能统一按线程连接 |
| Native Hook | `native_hook.ipid/itid` 关联进程/线程内部 ID；先区分 `event_type`，再解释大小与持续时间 |

- `R` 表示 Runnable，不等于正在运行；运行状态的具体字符串以实际数据库为准。CPU 执行时长优先从 `sched_slice` 取数。
- 时间戳不能直接当作墙上时间；事件时刻与插件上报时刻也不一定相同。跨来源比较前确认时钟域和单位。
- 查询时间窗口时，对有效持续区间使用 `ts < :end AND ts + dur > :start`；聚合窗口内时长时裁剪到交集。缺失或无效时长先按该表合同处理。
- 查询输出明确列名、别名和顺序，与结果 Schema 一致；筛选值用命名参数，避免拼接 SQL。

详细字段见[上游数据表说明](https://gitcode.com/openharmony/developtools_smartperf_host/blob/master/smartperf_host/trace_streamer/doc/des_tables.md)，时间语义见[时间戳规则](https://gitcode.com/openharmony/developtools_smartperf_host/blob/master/smartperf_host/trace_streamer/doc/des_timestamp.md)和[时钟说明](https://gitcode.com/openharmony/developtools_smartperf_host/blob/master/smartperf_host/trace_streamer/doc/times.md)。
