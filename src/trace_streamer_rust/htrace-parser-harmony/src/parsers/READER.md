# parsers

`src/parsers` 放具体输入格式解析器，负责把不同 trace 输入转换为统一的 `ParsedTrace`。

## 主要模块

- `htrace.rs`: profiler/htrace 二进制入口。读取 header 或 length-prefixed segment，解码 `ProfilerPluginData`，分发 ftrace/cpu/diskio/memory/process/arkts 插件，并维护 ftrace 调度、binder、workqueue、irq、clock、dma fence、oom score 等事件状态。
- `registry.rs`: 顶层格式识别入口。先解开常见 zip/zlib 包装，再按 bytrace、rawtrace、perf、hisysevent、hilog、htrace 顺序选择具体 parser。
- `bytrace.rs`: bytrace 文本入口。解析文本行、sched switch/wakeup、trace marker、binder transaction 和 softirq entry/exit，维护 CPU running slice、thread state、irq 与 shared callstack。
- `rawtrace.rs`: rawtrace segment 解析，支持二进制 segment 和文本 dump 形态，保留原始事件信息。
- `hilog.rs`: hilog 文本解析，生成 `log` 表并维护日志时间戳、级别、tag、pid/tid、消息体。
- `hisysevent.rs`: hisysevent JSON lines 解析，生成系统事件明细和 measure 表。
- `perf.rs`: perf 数据解析，处理 header、feature section、mmap/comm/sample record，并生成 perf 文件、线程、sample、callchain 等表。

## 业务规则

- 每个 parser 都输出统一的 `ParsedTrace`，并通过 `TraceTableBuilder` 写表。
- 无法结构化识别的行或插件数据应尽量保留到 `raw_event`，方便后续扩展解析能力。
- 需要跨事件计算 duration 的 parser 应维护状态机，并在结束时关闭或回写未完成的行。
- htrace ftrace 事件会先按 timestamp 和原始顺序排序，再进入状态机，避免跨 CPU segment 乱序影响结果。
- ArkTS CPU profiler sample 时间使用 htrace 传入的 MONOTONIC 到 BOOTTIME 转换逻辑对齐主时间轴。

## 设计边界

- 本目录只负责输入格式解析和事件状态推进。
- 表 schema、id 分配和 batch 构造依赖 `htrace-model`。
- plugin 级业务语义优先放在 `src/plugins`，避免 htrace 主 parser 无限膨胀。
