# Trace Streamer SQL

使用 SQLite 方言。先查询 `sqlite_schema` 的 `name`、`sql` 确认实际表和列；不同采集内容、解析器版本不保证表结构完全相同。关联使用内部 ID，不把系统 `pid`、`tid` 当作内部键。

## 常用表

### 线程、调度与调用

| 表 | 字段与关联 |
| --- | --- |
| `process`、`thread` | `pid`、`tid` 是系统编号，`name` 是名称；`thread.ipid = process.id` 表达线程所属进程 |
| `sched_slice` | `ts`、`dur` 是调度片段的开始和持续时间，`cpu` 是执行 CPU；`itid` 关联 `thread.id` |
| `thread_state` | `ts`、`dur`、`state` 表达线程状态区间；`itid` 关联 `thread.id`。`R` 是可运行，不能当作正在运行；CPU 执行时间优先查询 `sched_slice` |
| `raw`、`instant` | `raw(ts, name, cpu, itid)` 保存调度等事件；`instant` 保存唤醒事件，`wakeup_from` 是唤醒方内部线程号，解释 `value` 前先检查 `ref_type` |
| `callstack` | `ts`、`dur`、`name` 描述调用，`parent_id` 关联父调用的 `id`。同步调用的 `callid` 指向线程；异步调用带 `cookie`，`callid` 指向进程、`child_callid` 指向线程，`depth` 仅对同步调用有意义 |
| `irq` | `ts`、`dur` 描述中断区间，`cat` 区分 `irq`、`softirq`、`ipi`，`name` 是中断名称 |
| `task_pool` | `allocation_task_row`、`execute_task_row`、`return_task_row` 关联 `callstack.id`，对应分发、执行、返回阶段；各阶段的 `*_itid` 标识线程 |
| `log` | `ts`、`level`、`tag`、`context` 是日志时间、级别、标签和正文；这里的 `pid`、`tid` 是系统编号 |

### CPU 与系统计量

| 表 | 字段与关联 |
| --- | --- |
| `measure`、`cpu_measure_filter` | `measure.filter_id = cpu_measure_filter.id`；filter 的 `cpu`、`name` 区分 CPU 与频率/空闲等指标，`ts`、`dur`、`value` 是计量区间和值 |
| `process_measure`、`process_measure_filter` | 按 `filter_id = id` 关联；filter 的 `ipid`、`name` 指定进程与指标，计量表的 `ts`、`dur`、`value` 保存内存等采样 |
| `sys_mem_measure`、`sys_event_filter`、`measure_filter` | 系统内存计量使用 `ts`、`dur`、`value`；按 `filter_id` 查询 filter 的 `id`、`name`，通用 `measure_filter.type` 区分指标来源 |
| `cpu_usage` | `ts`、`dur` 是采样区间，`total_load`、`user_load`、`system_load` 是总、用户态、内核态负载 |
| `diskio` | `ts`、`dur` 是采样区间，`rd`、`wr` 是读写量，`rd_speed`、`wr_speed` 是速率；不要将速率直接求和当作总量 |
| `network` | 按 `net_type` 区分网络类型；`ts`、`dur`、`tx`、`rx` 及 `tx_speed`、`rx_speed` 描述收发采样，`packet_in`、`packet_out` 描述包数 |
| `smaps` | `timestamp` 是快照时间，`pss`、`resident_size`、`private_dirty` 等描述内存；`path_id`、`protection_id` 关联 `data_dict.id`，跨快照不要重复累加 |

### 帧与内存分配

| 表 | 字段与关联 |
| --- | --- |
| `frame_slice` | `type=0` 为实际帧，`type=1` 为期望帧；`ts`、`dur` 描述帧区间，`ipid`、`itid` 关联进程和线程，`callstack_id` 关联调用。`dur` 缺失表示数据不完整 |
| `frame_maps`、`gpu_slice` | `frame_maps.src_row`、`dst_row` 指向 `frame_slice` 中 App、RenderService 帧的行；`gpu_slice.frame_row` 指向渲染帧，`dur` 是 GPU 渲染时长 |
| `native_hook` | `ipid`、`itid` 关联进程和线程；`event_type` 区分 `AllocEvent`、`FreeEvent`、`MmapEvent`、`MunmapEvent`，`addr` 是地址，`heap_size` 是事件涉及的内存大小 |
| `native_hook_frame` | 通过 `callchain_id` 关联 Native Hook 事件，按 `depth` 排列栈帧；`symbol_id`、`file_id` 关联 `data_dict.id` 取得函数名和文件路径 |
| `native_hook_statistic` | 按 `ipid`、`callchain_id`、`type` 分组查看 `ts` 时刻的累计 `apply_count`、`release_count`、`apply_size`、`release_size`；`type=0` 为 malloc，`type=1` 为 mmap，不跨时间累加累计值 |

Native Hook 的 `start_ts`、`end_ts`、`dur` 描述分配的活跃区间；`all_heap_size` 是该时刻的活跃内存总量，堆事件与映射事件分别统计。不要把不同事件类型的 `heap_size` 全部相加解释成存活内存，也不要把逐事件的 `all_heap_size` 累加。

### 采样、调用栈与诊断

| 表 | 字段与关联 |
| --- | --- |
| `perf_sample`、`perf_thread` | `thread_id` 关联采样线程，`perf_thread.process_id` 是所属进程；与 trace 对齐用 `timestamp_trace`，`timestamp` 是未同步时间。按 `event_type_id` 区分事件，再聚合 `event_count` |
| `perf_callchain`、`perf_files` | `perf_sample.callchain_id = perf_callchain.callchain_id`，按 `depth` 排列；解析符号时同时匹配 `file_id` 与 `symbol_id = serial_id`，取得 `symbol`、`path` |
| `perf_report` | `report_type`、`report_value` 保存采样事件、命令和目标等配置；先确认采样事件含义，再解释计数 |
| `file_system_sample` | `ipid`、`itid` 指定进程线程，`type` 区分文件操作；`start_ts`、`end_ts`、`dur`、`size`、`return_value`、`error_code` 用于分析耗时、读写大小与失败 |
| `bio_latency_sample`、`ebpf_callstack` | I/O 样本的 `start_ts`、`end_ts`、`latency_dur` 描述延迟，按 `callchain_id` 关联栈帧；栈的 `symbols_id`、`file_path_id` 关联 `data_dict.id` |
| `js_cpu_profiler_sample`、`js_cpu_profiler_node` | 按 `function_id` 关联；样本的 `start_time`、`end_time`、`dur` 描述区间，node 的 `parent_id` 描述调用关系，`function_index`、`url_index` 关联 `data_dict.id` |
| `data_dict`、`args` | `data_dict(id, data)` 解码名称与路径；`args` 按 `argset` 组织参数集合，解释 `key`、`value` 时检查 `datatype`，不要把所有整数都当字符串索引 |
| `trace_range`、`stat` | `trace_range(start_ts, end_ts)` 给出 trace 的纳秒时间范围；`stat(event_name, stat_type, count)` 用于检查解析异常、事件丢失与起止不匹配，空结果不一定代表没有发生事件 |

## 时间与查询条件

- 数据库中的事件时间通常已转换到 BOOTTIME；`datasource_clockid(data_source_name, clock_id)` 记录的是来源原始时钟，不能据此对数据库时间再次换算。需要核对时钟对齐时，查看 `clock_snapshot(clock_id, ts, clock_name)`。
- BOOTTIME 包含系统休眠时间，MONOTONIC 不包含；两者都不是墙上时间。跨库或跨来源比较前确认时间单位和对齐依据，不直接比较来源不明的整数。
- Ftrace、Native Hook、Hilog、FPS 使用事件内时间；内存、网络、CPU、进程、磁盘和 HiSysEvent 的周期数据使用插件上报时间，两者不一定表示同一时刻。
- 对完整、有效的持续区间，窗口交集条件为 `ts < :end AND ts + dur > :start`；窗口内时长为 `MIN(ts + dur, :end) - MAX(ts, :start)`。先排除缺失或无效时长，瞬时事件用 `ts >= :start AND ts < :end`。
- 先按目标进程、线程、事件类型和时间范围筛选，再关联与聚合；一对多关联会放大行数，避免重复累计时长或内存。
- 输出明确列名、别名和顺序，与结果 Schema 一致；筛选值使用命名参数，避免拼接 SQL。
