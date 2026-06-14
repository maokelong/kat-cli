# Probe: critical_path.callstack_context

## 用途

查询窗口内的函数 span 上下文，辅助解释 Running、阻塞函数或 Binder/图像解码等行为。它只提供上下文，不单独证明关键路径。

## 读取表

- `callstack`: 函数 span，`callid` 对应 SQLite `thread.itid`。
- `thread`: 补充 `tid/thread_name/ipid`。
- `process`: 补充 `pid/process_name`。

## 输入说明

- `db`: SQLite 数据库路径。
- `itid`: 可选线程过滤。兼容旧参数名 `utid`。
- `start_ts/end_ts`: 当前窗口。
- `max_rows`: 返回行数上限。

## LLM 解读规则

- 优先与 `thread_state_profile`、`sched_profile` 交叉验证。
- 长 span 或高 overlap 只能说明上下文覆盖窗口，不能单独写成根因。
- 对关键路径窗口建议传入 `itid`，避免多个线程的 callstack 混在同一个证据中。
