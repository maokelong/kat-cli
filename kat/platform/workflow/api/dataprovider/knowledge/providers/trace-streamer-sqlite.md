# Trace Streamer SQL

使用 SQLite 方言。先查询 `sqlite_schema` 的 `name`、`sql` 确认实际表和列；不同采集内容、解析器版本不保证表结构完全相同。关联使用内部 ID，不把系统 `pid`、`tid` 当作内部键。

## 常用表

| 表 | 字段与关联 |
| --- | --- |
| `process`、`thread` | `pid`、`tid` 是系统编号，`name` 是名称；`thread.ipid = process.id` 表达线程所属进程 |
| `sched_slice` | `ts`、`dur` 是调度片段的开始和持续时间，`cpu` 是执行 CPU；`itid` 关联 `thread.id` |
| `thread_state` | `ts`、`dur`、`state` 表达线程状态区间；`itid` 关联 `thread.id`。`R` 是可运行，不能当作正在运行；CPU 执行时间优先查询 `sched_slice` |
| `callstack` | `ts`、`dur`、`name` 描述调用，`parent_id` 关联父调用的 `id`。同步调用的 `callid` 指向线程；异步调用带 `cookie`，`callid` 指向进程、`child_callid` 指向线程，`depth` 仅对同步调用有意义 |
| `frame_slice` | `type=0` 为实际帧，`type=1` 为期望帧；`ts`、`dur` 描述帧区间，`ipid`、`itid` 关联进程和线程，`callstack_id` 关联调用。`dur` 缺失表示数据不完整 |
| `native_hook` | `ipid`、`itid` 关联进程和线程；`event_type` 区分 `AllocEvent`、`FreeEvent`、`MmapEvent`、`MunmapEvent`，`addr` 是地址，`heap_size` 是事件涉及的内存大小 |

Native Hook 的 `start_ts`、`end_ts`、`dur` 描述分配的活跃区间；`all_heap_size` 是该时刻的活跃内存总量，堆事件与映射事件分别统计。不要把不同事件类型的 `heap_size` 全部相加解释成存活内存，也不要把逐事件的 `all_heap_size` 累加。

## 时间与查询条件

- 数据库中的事件时间通常已转换到 BOOTTIME；`datasource_clockid(data_source_name, clock_id)` 记录的是来源原始时钟，不能据此对数据库时间再次换算。需要核对时钟对齐时，查看 `clock_snapshot(clock_id, ts, clock_name)`。
- BOOTTIME 包含系统休眠时间，MONOTONIC 不包含；两者都不是墙上时间。跨库或跨来源比较前确认时间单位和对齐依据，不直接比较来源不明的整数。
- Ftrace、Native Hook、Hilog、FPS 使用事件内时间；内存、网络、CPU、进程、磁盘和 HiSysEvent 的周期数据使用插件上报时间，两者不一定表示同一时刻。
- 对完整、有效的持续区间，窗口交集条件为 `ts < :end AND ts + dur > :start`；窗口内时长为 `MIN(ts + dur, :end) - MAX(ts, :start)`。先排除缺失或无效时长，瞬时事件用 `ts >= :start AND ts < :end`。
- 先按目标进程、线程、事件类型和时间范围筛选，再关联与聚合；一对多关联会放大行数，避免重复累计时长或内存。
- 输出明确列名、别名和顺序，与结果 Schema 一致；筛选值使用命名参数，避免拼接 SQL。
