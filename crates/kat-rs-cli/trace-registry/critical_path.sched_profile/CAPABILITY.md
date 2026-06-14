# Probe: critical_path.sched_profile

## 用途

按 SQLite `sched_slice.itid` 查询当前窗口内 CPU 执行切片，补充 `thread_state_profile` 对 `Running/Runnable` 的判断。

## 读取表

- `sched_slice`: `itid/ts/dur/cpu/priority/end_state`。

## 输入说明

- `db`: SQLite 数据库路径。
- `itid`: SQLite 内部线程 ID。兼容旧参数名 `utid`。
- `start_ts/end_ts`: 当前窗口。
- `max_rows`: 返回行数上限。

## 输出说明

- `sched_running_ns`: 窗口内裁剪后的调度运行总时长。
- `cpu_summary_ns`: 按 CPU 聚合的运行时长。
- `slices`: 裁剪后的调度切片。

## LLM 解读规则

- `sched_slice` 只能证明线程在 CPU 上运行过，不能单独解释等待依赖。
- 若 `thread_state` 显示主要为 `Running`，且 `sched_running_ns` 与 running 时长接近，可作为自身执行证据。
- 如果 `thread_state` 显示 Runnable 时间高但 sched 很少，可作为调度等待事实输入，不直接写成根因。

## 禁止结论

- 禁止仅凭 sched 切片输出根因。
- 禁止把 CPU 号或切片数量直接解释为卡顿原因。
