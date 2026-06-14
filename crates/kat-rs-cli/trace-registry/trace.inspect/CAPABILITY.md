# Probe: trace.inspect

## 用途

只读检查 SQLite trace DB 的表、字段、行数和 `trace_range`。

## 关键表

关键路径抽取最小闭环需要：

- `process`
- `thread`
- `thread_state`
- `instant`
- `sched_slice`
- `callstack`
- `trace_range`

首帧定位额外优先使用：

- `callstack`
- 可选参考 `frame_slice`
