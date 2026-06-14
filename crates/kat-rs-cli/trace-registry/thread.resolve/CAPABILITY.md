# Probe: thread.resolve

## 用途

按 `itid/tid/thread_name/process_name` 在 SQLite `thread/process` 中解析线程候选。

## LLM 解读规则

- 如果 `frame.first_draw` 已给出 `root_thread_itid`，优先用该值解析根线程。
- 如果用户只给出进程名或 tid，多候选时优先选择 `is_main_thread=1` 且进程与目标一致的线程。
- 本 probe 只解析身份，不判断关键路径。
