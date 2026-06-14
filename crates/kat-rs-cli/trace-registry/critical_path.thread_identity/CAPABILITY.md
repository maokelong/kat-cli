# Probe: critical_path.thread_identity

## 用途

把候选 `itid` 解析为线程名、进程名，并给出 deterministic 的 `udk-irq` / IO 线程候选分类。

## 读取表

- `thread`: `itid/tid/name/ipid/is_main_thread`。
- `process`: `ipid/pid/name`。

## Checklist 生成建议

在 `critical_path.thread_state_profile` 发现 `new_candidate_edges[*].waker_itid` 后调用本 probe。只解析本轮新发现的 waker，历史候选不重复解析。

## 输入说明

- `itids`: 本轮新发现的 waker `itid` 去重列表。兼容旧参数名 `utids`。

## LLM 解读规则

- 命中 `udk-irq` 时，只终止对应候选边，不终止全局候选池循环。
- 命中 IO 线程候选集合时，可把对应边标记为 `terminal` 或 `explained`，最终结论仍需综合上下文。
- 普通 worker 保持 `status=pending`，由 LLM 后续从全局候选池挑选。
