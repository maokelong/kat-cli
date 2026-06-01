# htrace-parser-harmony

`htrace-parser-harmony` 是 Harmony/OpenHarmony trace 的解析层。

## 业务职责

- 自动识别 bytrace、htrace/profiler、rawtrace、hilog、hisysevent、perf 等输入格式。
- 将输入解析为 `htrace-model::ParsedTrace`，保持表名和字段尽量贴近 C++ TraceStreamer。
- 提供统一的 `HarmonyTraceParser` trait、格式探测函数和 `parse_trace_file`/`parse_trace_bytes` 入口。
- htrace/profiler 入口按 plugin name 分发到 ftrace、cpu、diskio、memory、process、arkts 等解析逻辑。
- bytrace 文本入口解析 sched、wakeup、trace marker、binder、softirq，并写入调度表、irq、raw_event 和 shared callstack 表。

## 模块结构

- `registry.rs`: 负责输入格式探测和顶层 parser 路由。
- `parser.rs`: 定义各 parser 共享的解析接口。
- `src/parsers`: 放具体输入格式解析器，包括 htrace、bytrace、rawtrace、hilog、hisysevent、perf。
- `src/plugins`: 放 htrace/profiler plugin 及跨格式共享状态机，包括 shared、memory、process、arkts。

## 设计边界

- parser 层负责格式识别、协议解码、时间戳处理、状态机推进和插件路由。
- Arrow schema、表行结构和最终 batch 构建由 `htrace-model` 负责。
- SQL 查询、对比报告和前端展示不放在本 crate。
