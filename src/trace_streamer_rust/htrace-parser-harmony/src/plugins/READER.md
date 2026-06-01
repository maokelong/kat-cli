# plugins

`src/plugins` 放 htrace/profiler plugin 及跨输入格式共享模型的业务解析。

## shared.rs

- 解析 trace marker payload: `B|pid|name`、`E`、`E|pid`、`S|pid|name|cookie`、`F|pid|name|cookie`、`C|pid|name|value`，并兼容 bytrace/htrace 中常见的空格分隔参数形态。
- 维护同步调用栈和异步 cookie 映射，负责 begin/end、async begin/end、counter 的状态推进。
- 写入 `callstack`，并把 `name##key=value`、counter value 等参数写入 `args`/`data_dict`。
- 为 bytrace 文本和 htrace ftrace `print`/`tracing_mark_write` 提供共享处理逻辑。

## memory.rs

- 解码 memory plugin 的 process/system memory 数据。
- 将进程维度指标写入 `process_measure` 和 `process_measure_filter`。
- 将系统维度指标写入 `sys_mem_measure` 和 `sys_event_filter`。
- 对同一个 filter 的上一条 metric 回写 duration，保持指标区间可查询。

## process.rs

- 解码 process plugin 的进程采样数据。
- 缓存采样点，并在解析结束时按时间排序生成 `live_process`。
- 维护 process name、pid、ppid、uid、thread count、CPU/内存/IO 等采样字段。
- 使用相邻采样点时间差计算 duration，首采样作为基线参与后续区间生成。

## arkts.rs

- 解码 `arkts-plugin_config` 和 `arkts-plugin` result。
- 将 ArkTS 配置写入 `js_config`，包括 heap 类型、采样间隔、allocation/cpu profiler 开关。
- 支持 chunked JSON 拼接，完整文档到达后解析 JS heap snapshot。
- 写入 `js_heap_files`、`js_heap_info`、`js_heap_nodes`、`js_heap_edges`、`js_heap_string`、`js_heap_location`、`js_heap_sample`、`js_heap_trace_function_info`、`js_heap_trace_node`。
- 解析 CPU profiler `profile.nodes/samples/timeDeltas/startTime`，写入 `js_cpu_profiler_node` 和 `js_cpu_profiler_sample`。

## 设计边界

- plugin 模块只处理插件业务语义和状态机，不直接做 SQL 或 UI 展示。
- 新 plugin 应优先复用 `TraceTableBuilder`，避免绕过统一 schema。
- 输入 framing、顶层 plugin 路由和跨 CPU ftrace 排序属于 `src/parsers` 职责。
