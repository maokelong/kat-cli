# Probe: critical_path.thread_state_profile

## 用途

查询 SQLite `test.db` 中单个线程在窗口内的 `thread_state` 画像，并从 `instant(name='sched_wakeup')` 补出唤醒边候选。

## 读取表

- `thread_state`: 线程状态片段，主键字段为 `itid`。
- `instant`: 唤醒事件，`ref` 表示被唤醒线程 `itid`，`wakeup_from` 表示唤醒方 `itid`。
- `thread`: 补充 `tid/thread_name/ipid`。
- `process`: 补充 `pid/process_name`。

## Checklist 生成建议

生成 `call-probe` step：
- Probe: `critical_path.thread_state_profile`
- Inputs:
  - `db`
  - `itid`
  - `start_ts`
  - `end_ts`
  - `depth`
  - `max_depth`
  - `min_segment_ns`
  - `visited_edges`
  - `candidate_frontier`
  - `selected_edge`
  - `explained_intervals`
  - `root_window`
  - `inherited_blocking_context`

执行后紧跟一个 `llm-review` step。`next_candidate_edges` 是全局候选池，不按 depth 分层；LLM 后续从所有 `status=pending` 的候选中挑一个继续分析。

## 输入说明

- `itid`: 当前分析线程的 SQLite 内部线程 ID。兼容旧参数名 `utid`，但在 `test.db` 语义中应使用 `itid`。
- `start_ts/end_ts`: 当前依赖窗口。
- `depth`: 当前依赖深度。
- `visited_edges`: 已访问依赖边，用于环检测。
- `candidate_frontier`: 进入本轮前的全局候选池。
- `selected_edge`: 本轮 LLM 选择分析的候选边；根线程首轮为 `null`。
- `explained_intervals`: LLM 已判定解释清楚的 root-window 子区间。
- `root_window`: 根分析窗口，用于计算 coverage ratio。
- `inherited_blocking_context`: 上层 D 状态或阻塞函数上下文。

## 输出 evidence 说明

- `state_summary_ns`: 按状态聚合的裁剪后耗时。
- `dominant_state`: 当前窗口耗时最长的状态。
- `segments`: 裁剪后的状态片段。
- `candidate_wait_segments`: `R/R+/S/D/D-IO` 等等待或可运行候选。
- `new_candidate_edges`: 本轮从 `instant.sched_wakeup` 新发现的 waker edge。
- `next_candidate_edges`: 合并旧候选与新候选后的全局候选池。
- `selected_edge_update`: 当前 `selected_edge` 的建议状态更新。
- `visited_edges_after`: 当前 selected edge 访问后的 visited 边集合。
- `coverage`: 基于 LLM 输入 `explained_intervals` 的覆盖率计算。
- `edge_boundary_hints`: 当前 edge 的边界提示，只影响当前 edge；`mostly_running` 表示窗口至少 90% 为 CPU running。
- `frontier_hints`: 全局候选池提示，例如没有 pending edge。

## LLM 解读规则

- `Running` 才表示正在 CPU 执行；SQLite DB 中的 `R/R+` 解释为 `runnable`。
- `new_candidate_edges` 只代表可探索候选，不代表最终关键路径。
- `max_depth`、环、`udk-irq`、缺失证据等只终止当前候选边，不终止全局候选池循环。
- 若等待片段没有 `instant.sched_wakeup` 证据，不凭空补全 waker。
- 若 `edge_boundary_hints` 同时包含 `mostly_running` 和 `wait_chain_not_closed`，优先把当前边解释为自身执行，微小 Runnable 不应抢占主结论。
- 只有 LLM 基于 evidence 判断关键路径约 99% 时间已解释，才停止继续从全局候选池选择。
