# Rust Rewrite Status

更新时间: 2026-06-01

## 本轮补齐范围

- `arkts/js heap`
  - 新增 Rust 表模型与解析: `js_config`、`js_cpu_profiler_node`、`js_cpu_profiler_sample`。
  - CPU profiler 支持 `profile.nodes/samples/timeDeltas/startTime`，节点函数名和 URL 写入共享 `data_dict`，sample 时间按 MONOTONIC -> BOOTTIME clock snapshot 转换。
  - 当前样例中 `js_heap_*`、`js_config`、`js_cpu_profiler_*` 均已纳入 HTML target table 验算。
- shared dictionary / args
  - `args.datatype` 枚举对齐 C++: int=0、string=1、boolean=3。
  - `TraceTableBuilder` 在写入 process/thread/raw/instant/irq/callstack/filter/symbol 等业务表时同步把关键字符串写入 `data_dict`，补上 C++ DataIndex 模型的一部分。
  - htrace/bytrace binder 补齐 `destination thread`、`destination name`、`destination slice id` 关联参数。
  - bytrace softirq entry/exit 写入 `irq`，并在 exit 时补齐 `irq_ret` / `vec` args。
- `compare-cpp-sqlite`
  - target table 扩展到 20 张，新增 `js_config`、`js_cpu_profiler_node`、`js_cpu_profiler_sample`。
  - 新增 `args_detail_sample`、扩展 `data_dict_sample`，便于在 HTML 中直接看 args/key/string_value 差异。

## 最新对比结论

报告文件: `trace_streamer/trace_streamer_rust/target/compare_validation_report.html`

`htrace_pbreader` 场景:

- 已对齐:
  - `callstack`: C++ 110799 / Rust 110799。
  - `process_measure`: C++ 20986 / Rust 20986。
  - `process_measure_filter`: C++ 2246 / Rust 2246。
  - `sys_mem_measure`: C++ 992 / Rust 992。
  - `sys_event_filter`: C++ 124 / Rust 124。
  - `live_process`: C++ 10546 / Rust 10546。
  - `js_heap_files`: C++ 3 / Rust 3。
  - `js_heap_info`: C++ 96 / Rust 96。
  - `js_heap_nodes`: C++ 98505 / Rust 98505。
  - `js_heap_edges`: C++ 458481 / Rust 458481。
  - `js_heap_string`: C++ 76698 / Rust 76698。
  - `js_heap_location`、`js_heap_sample`、`js_heap_trace_function_info`、`js_heap_trace_node`: 当前样例均为 0 / 0。
  - `js_config`: C++ 1 / Rust 1。
  - `js_cpu_profiler_node`: C++ 4 / Rust 4。
  - `js_cpu_profiler_sample`: C++ 67 / Rust 67。
- 仍未对齐:
  - `data_dict`: C++ 150785 / Rust 121088。
  - `args`: C++ 748323 / Rust 112635。
  - `args_by_datatype`: C++ 为 `0=389299, 1=348729, 3=10295`；Rust 为 `0=71760, 1=30580, 3=10295`。boolean 已对齐，int/string 仍主要缺 raw ftrace 字段和 C++ 内部 argset 写入。

`bytrace_full` 场景:

- 已对齐:
  - `args`: C++ 28 / Rust 28，且 datatype 分布同为 `0=17, 1=9, 3=2`。
  - `callstack`: C++ 4 / Rust 4。
  - `process_measure` / `process_measure_filter` / `sys_mem_measure` / `sys_event_filter` / `live_process` / `js_heap_*` / `js_config` / `js_cpu_profiler_*`: 当前样例均为 0 / 0。
- 仍未对齐:
  - `data_dict`: C++ 89 / Rust 44。
  - 差异主要来自 C++ 初始化阶段预注册的默认字典项，以及部分 bytrace/scheduler/H marker 相关字符串仍只保存在 Rust 业务列中。

## 已执行验证

- `cargo test -p htrace-parser-harmony`: 25 passed。
- `cargo check -p htrace-engine-cli --bin compare-cpp-sqlite`: passed。
- `cargo test --workspace`: passed。
- `cargo run -p htrace-engine-cli --bin compare-cpp-sqlite -- --html-output target/compare_validation_report.html`: passed，并重新生成 HTML 报告。

## 剩余 Gap

- 已由 HTML 验算出的 gap:
  - `htrace_pbreader.data_dict`: 还差 29697 行。Rust 已接入 symbol 与主要业务表字符串入字典，但尚未完整复刻 C++ 默认字典、全部 raw ftrace 字段名/字段值、统计项和部分内部模型字符串。
  - `htrace_pbreader.args`: 还差 635688 行。下一阶段需要按 C++ ArgsFilter/RawEventParser 的粒度，把 raw ftrace 事件字段、sched/irq/raw/instant/measure 的内部参数批量写入 argset。
  - `bytrace_full.data_dict`: 还差 45 行。`args` 已对齐，剩余是默认字典与未入字典的文本列。
- 代码覆盖分析出的后续范围:
  - memory plugin 旁路表: `smaps`、`ashmem`、`dma`、`gpu process/window`、`window manager`、`cpu dump`、`profile mem`、`RS image` 等仍未进入 Rust；当前样例中用户关注的 `process_measure` / `sys_mem_measure` 已对齐。
  - ArkTS timeline split/filter: 当前样例的 snapshot/config/CPU profiler 已对齐；split file/filter 逻辑仍需后续有样例时继续验算。
