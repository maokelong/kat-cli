# htrace-model

`htrace-model` 是 Rust TraceStreamer 的数据模型层，负责把解析器产生的行数据构造成 Arrow `RecordBatch`。

## 业务职责

- 定义 TraceStreamer 表 schema，并保持表名、字段名和数据类型尽量贴近 C++ TraceStreamer。
- 为每张表提供行结构和 `TraceTableBuilder` 写入入口，让 parser/plugin 只需要 push 业务行。
- 在 `finish` 阶段统一生成 `ParsedTrace`，包含 trace id、时间范围、clock domain 和全部表批次。
- 维护轻量索引和回写能力，例如字符串 intern 到 `data_dict`、argset 分配、callstack 行回写、measure duration 回写、JS heap self size 遍历。

## 表模型覆盖

- 基础元信息: `trace_metadata`、`trace_bounds`。
- 进程线程与调度: `process`、`thread`、`sched_slice`、`thread_state`。
- 原始事件与即时事件: `raw_event`、`raw`、`instant`、`irq`。
- 通用指标与资源: `measure`、`measure_filter`、`cpu_measure_filter`、`cpu_usage`、`diskio`、`dma_fence`、`symbols`。
- 共享字典与调用栈: `data_dict`、`args`、`callstack`。
- memory/process plugin: `process_measure`、`process_measure_filter`、`sys_mem_measure`、`sys_event_filter`、`live_process`。
- ArkTS/JS heap 与 CPU profiler: `js_heap_files`、`js_heap_info`、`js_heap_nodes`、`js_heap_edges`、`js_heap_string`、`js_heap_location`、`js_heap_sample`、`js_heap_trace_function_info`、`js_heap_trace_node`、`js_config`、`js_cpu_profiler_node`、`js_cpu_profiler_sample`。
- 文本日志、系统事件和 perf: `log`、`hisysevent_all_event`、`hisysevent_measure`、`perf_report`、`perf_files`、`perf_thread`、`perf_sample`、`perf_callchain`。

## 设计边界

- 本 crate 不解析二进制协议，也不解释插件业务语义。
- schema 变更会影响 parser、query、CLI 和 Web UI，调整前要确认 C++ TraceStreamer 表结构和现有 SQL 兼容性。
- builder 可以做表级索引、id 分配和最终 batch 构造，不应引入输入格式识别或 UI 展示逻辑。
