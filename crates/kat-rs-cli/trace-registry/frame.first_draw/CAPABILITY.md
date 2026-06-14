# Probe: frame.first_draw

## 用途

从 SQLite `callstack` 中定位目标进程的 `firstDrawFrame:1` marker，并抽取根分析窗口。

## 读取表

- `callstack`: marker 文本在 `name` 字段中。
- `thread`: `callstack.callid = thread.itid`，用于获得根线程。
- `process`: 通过 `thread.ipid = process.ipid` 过滤目标进程。

## 输出

- `frame_start_ts`: `layoutMeasureDurationStartTimestamp`。
- `frame_end_ts`: `layoutMeasureDurationEndTimestamp`。
- `duration_ns`: 首帧分析窗口时长。
- `root_thread_itid`: marker 所在线程，对关键路径根线程解析有强参考价值。
- `marker_payload`: 原始 marker 文本。

## LLM 解读规则

- 若 marker 命中，后续关键路径 root window 使用 `frame_start_ts/frame_end_ts`。
- 若 marker 线程就是目标主线程，可直接把 `root_thread_itid` 交给 `thread.resolve` 或 `thread_state_profile`。
- 若存在多个候选，优先选择 process/tid 与用户目标一致且最早出现的 `firstDrawFrame:1`。
