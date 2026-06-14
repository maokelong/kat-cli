---
schema: trace-checklist.v1
analysis_id: critical-path-probe-draft
strategy_id: critical-path
executor: llm
status: draft
data_source: sqlite
trace: <trace_or_db_path>
---

# Trace 关键路径分析工作清单

本 checklist 是给 LLM 执行分析流程用的显式编排文件。LLM 必须按步骤推进，probe 只产出确定性证据；分支、覆盖率判断、候选选择和最终问题定位由 LLM 基于证据完成。

## SQLite 表语义

- `process`: 进程表，主键字段为 `ipid`，业务 pid 字段为 `pid`。
- `thread`: 线程表，主键字段为 `itid`，业务 tid 字段为 `tid`，通过 `ipid` 关联进程。
- `thread_state`: 线程状态表，按 `itid/ts/dur/state` 描述状态片段；`Running` 表示正在 CPU 执行，`R/R+` 表示 runnable。
- `instant`: 瞬时事件表；`name='sched_wakeup'` 时，`ref` 是被唤醒线程 `itid`，`wakeup_from` 是唤醒方 `itid`。
- `sched_slice`: 调度切片表，按 `itid/ts/dur/cpu` 描述 CPU 执行事实。
- `callstack`: 函数 span 表，`callid` 对应 `thread.itid`；目标首帧 marker 在 `name` 字段中。
- `frame_slice`: 帧切片表，可作为首帧和 frame/vsync 交叉校验。
- `trace_range`: trace 起止时间。

## Step 1. 检查 Trace/SQLite 输入

Status: todo
Type: call-probe
Probe: `trace.inspect`
Tables:
- `sqlite_master`
- `trace_range`
- 所有可见表的 `PRAGMA table_info`

Inputs:
- `trace`: `<trace_or_db_path>`

Params:
- 无

Goal: 确认 trace 输入可读，并检查关键路径抽取所需表是否存在。

Done When:
- evidence 中包含 `trace_range`。
- evidence 中包含表清单、字段清单和行数。
- `process/thread/thread_state/instant/sched_slice/callstack/trace_range` 都被标记为 present。

Evidence Ref: `ev.trace.inspect.001`

Decision:
- 待定

Branch Rules:
- 若 `thread_state` 缺失，关键路径状态画像阻塞。
- 若 `instant` 缺失，等待链只能记录为不闭合，不能递归 waker。
- 若 `sched_slice` 缺失，Running/Runnable 的调度证据标记为 `partial`。
- 若 `callstack` 缺失，首帧 marker 和函数上下文标记为 `partial`，必要时改用 `frame_slice` 兜底定位窗口。

Loop Until:
- 执行一次

## Step 2. 定位目标首帧窗口

Status: todo
Type: call-probe
Probe: `frame.first_draw`
Tables:
- `callstack`
- `thread`
- `process`
- 可选交叉校验: `frame_slice`

Inputs:
- `trace`: `<trace_or_db_path>`
- `process_query`: `<目标进程名或 pid>`

Params:
- `process_query`: `<process_query>`
- `max_rows`: `20`

Goal: 从 `firstDrawFrame:1` marker 中抽取根分析窗口和 marker 所在线程。

Done When:
- evidence status 为 `ok`，并包含 `frame_start_ts/frame_end_ts/duration_ns`。
- evidence 包含 `root_thread_itid/root_thread_tid/process_name/pid`。
- LLM 已把 `frame_start_ts/frame_end_ts` 写入 `analysis_state.root_window`。

Evidence Ref: `ev.frame.first_draw.001`

Decision:
- 待定

Branch Rules:
- 若命中多个首帧 marker，优先选择与用户目标进程名/pid 一致且时间最早的 `firstDrawFrame:1`。
- 若未命中 marker，才使用 `frame_slice` 的目标进程首个 actual/expect frame 作为候选窗口，并在报告中标记不确定性。
- 若 marker 的线程是目标进程主线程，可直接把 `root_thread_itid` 作为 Step 3 输入。

Loop Until:
- 执行一次，或 marker 缺失后完成 fallback 决策

## Step 3. 解析根线程

Status: todo
Type: call-probe
Probe: `thread.resolve`
Tables:
- `thread`
- `process`

Inputs:
- `trace`: `<trace_or_db_path>`
- `thread_query`: 优先使用 Step 2 的 `root_thread_itid`，否则使用用户给定 tid/thread name

Params:
- `thread_query`: `<root_thread_itid 或 root_tid>`
- `max_rows`: `20`

Goal: 将根线程解析为明确的 `itid/tid/thread_name/pid/process_name`。

Done When:
- evidence 中包含至少一个线程候选，或明确返回 `empty_result`。
- LLM 在 Decision 中选择一个根线程，写入 `analysis_state.root_thread`。

Evidence Ref: `ev.thread.resolve.001`

Decision:
- 待定

Branch Rules:
- 若 Step 2 已给出 `root_thread_itid`，优先选择 `itid` 精确匹配的候选。
- 若只按 tid/name 查询且多个候选，优先选择 `is_main_thread=1` 且进程与目标一致的线程。
- 若无法确定根线程，停止执行并请求补充输入。

Loop Until:
- 已选定根线程，或当前步骤被阻塞

## Step 4. 关键路径全局候选池循环

Status: todo
Type: llm-review
Probe: none
Tables:
- 循环体按子步骤使用 `thread_state/instant/thread/process/sched_slice/callstack`

Inputs:
- `analysis_state.root_thread`
- `analysis_state.root_window`
- `analysis_state.frontier.next_candidate_edges`
- `analysis_state.coverage`
- 最新 evidence 引用

Params:
- `coverage.target_ratio`: `0.99`
- `max_depth`: `8`
- `min_segment_ns`: `0`

Goal: 循环探索全局候选池，直到关键路径约 99% 的 root window 时间都能被证据解释，或没有仍有价值的 `pending` 候选。

Done When:
- LLM 判断 `coverage.coverage_ratio >= 0.99`，且解释区间都有 evidence 支撑。
- 或全局 `frontier.next_candidate_edges` 中没有仍有价值的 `status=pending` 候选。
- 已进入 Step 5/6 补充上下文，或进入 Step 7 综合输出。

Evidence Ref: 无

Decision:
- 待定

Branch Rules:
- `max_depth`、环、`udk-irq`、缺失证据、等待链不闭合等停止条件只终止当前 `selected_edge`，不得终止全局候选池循环。
- 当前 `selected_edge` 到达探索边界时，将该候选边标记为 `terminal` 或 `explained`，写入 `terminal_reason/explanation_kind`，然后回到全局候选池选择下一个 `pending` 候选边。
- 每轮最多选择一个 `selected_edge` 继续执行。
- LLM 从所有 depth 的 `status=pending` 候选边中全局挑选，不按深度分层展开。
- 全局候选池循环退出只能由覆盖率判断或候选池耗尽判断触发。
- Step 4.A、Step 4.B、Step 4.C、Step 4.D 是本循环的一轮循环体，单个子步骤不独立控制循环退出。

Loop Until:
- `coverage.coverage_ratio >= 0.99`，且 LLM 判断剩余未解释时间不会影响关键路径结论。
- 或不存在仍有探索价值的 `status=pending` 候选。

## Step 4.A. 选择本轮候选

Status: todo
Type: llm-review
Probe: none
Tables:
- 无

Inputs:
- `analysis_state.frontier.next_candidate_edges`
- `analysis_state.coverage`
- 最新 evidence 引用

Params:
- 无

Goal: 从全局候选池中选择一个 `status=pending` 的候选边；首轮没有候选时，以根线程和根窗口作为本轮查询对象。

Done When:
- 首轮已设置 `selected_edge = null`，并以根线程 `itid` 和根窗口作为本轮查询对象。
- 非首轮已选择一个 `selected_edge`，并准备好 `itid/start_ts/end_ts/depth`。
- 若没有有价值的 `pending` 候选边，已记录候选池耗尽。

Evidence Ref: 无

Decision:
- 待定

Branch Rules:
- 优先选择能解释更大未解释时间窗口的候选边。
- 其次选择 `dependency_end_ts` 更接近 root window `end_ts` 的候选边。
- 跳过 `status != pending` 的候选边。
- 跳过已由 `critical_path.thread_identity` 标记为 `udk-irq` 的候选边，并将其标记为 `terminal`。

Loop Until:
- 本步骤不独立循环；随 Step 4 每轮执行一次。

## Step 4.B. 查询本轮线程状态画像

Status: todo
Type: call-probe
Probe: `critical_path.thread_state_profile`
Tables:
- `thread_state`
- `instant`
- `thread`
- `process`

Inputs:
- `trace`: `<trace_or_db_path>`
- `itid`: 首轮使用根线程 `itid`；后续轮次使用 `selected_edge.waker_itid`
- `start_ts`: 首轮使用 root window `start_ts`；后续轮次使用 `selected_edge.dependency_start_ts`
- `end_ts`: 首轮使用 root window `end_ts`；后续轮次使用 `selected_edge.dependency_end_ts`
- `depth`: 首轮为 `0`；后续轮次使用 `selected_edge.depth`
- `max_depth`: `8`
- `min_segment_ns`: `0`
- `visited_edges`: 来自 `analysis_state.json`
- `candidate_frontier`: `analysis_state.frontier.next_candidate_edges`
- `selected_edge`: 来自 Step 4.A；首轮为 `null`
- `root_window`: 根分析窗口
- `explained_intervals`: `analysis_state.coverage.explained_intervals`
- `inherited_blocking_context`: 来自 `analysis_state.json`

Params:
- `itid`: `<root_itid>`
- `start_ts`: `<root_start_ts>`
- `end_ts`: `<root_end_ts>`
- `depth`: `0`
- `max_depth`: `8`
- `min_segment_ns`: `0`
- `visited_edges`: `[]`
- `candidate_frontier`: `[]`
- `selected_edge`: `null`
- `root_window`: `{ "start_ts": "<root_start_ts>", "end_ts": "<root_end_ts>" }`
- `explained_intervals`: `[]`
- `inherited_blocking_context`: `null`

Goal: 生成本轮线程窗口内的状态事实、等待候选、新 waker，并输出合并后的全局候选池。

Done When:
- evidence 中包含 `state_summary_ns/dominant_state/segments`。
- evidence 中包含 `candidate_wait_segments` 或 `edge_boundary_hints`。
- evidence 中包含 `new_candidate_edges`。
- `new_candidate_edges[*]` 使用 `dependency_start_ts/dependency_end_ts` 表示依赖窗口时间戳。
- evidence 中包含全局 `next_candidate_edges`，其中包含旧候选和本轮新候选。
- 如果本轮有 `selected_edge`，evidence 中包含 `selected_edge_update`。

Evidence Ref: `ev.critical_path.thread_state_profile.*`

Decision:
- 待定

Branch Rules:
- trace 中 `Running` 才是 CPU running；`R/R+` 必须解释为 runnable。
- 若 `edge_boundary_hints` 包含 `no_state_segments/max_depth_reached/wait_chain_not_closed/all_candidate_edges_repeated`，只更新当前 `selected_edge` 状态，不停止全局循环。
- 若窗口主要为 `Running`，当前候选边可标记为 `explained`，并追加 Step 5/6 补充调度和调用栈上下文。
- 若同一窗口同时出现 `mostly_running` 与 `wait_chain_not_closed`，优先按 `mostly_running` 解释当前候选边；微小 Runnable 或缺下一层 waker 不应抢占主结论。
- 若存在带 `waker_itid` 的等待候选，将本轮新候选边合并到 `analysis_state.frontier.next_candidate_edges`。
- 所有 depth 的候选都进入同一个全局候选池，由 LLM 在 Step 4.D 从全集中挑选。
- 若等待候选无 `waker_itid`，不凭空补全依赖线程，记录等待链不闭合。

Loop Until:
- 本步骤不独立循环；随 Step 4 每轮执行一次。

## Step 4.C. 解析本轮新 waker 身份

Status: todo
Type: call-probe
Probe: `critical_path.thread_identity`
Tables:
- `thread`
- `process`

Inputs:
- `trace`: `<trace_or_db_path>`
- `itids`: 对 Step 4.B evidence 中 `new_candidate_edges[*].waker_itid` 去重后的列表

Params:
- `itids`: `<本轮新发现_waker_itid列表>`

Goal: 查询本轮新发现的 waker 身份，判断是否为 `udk-irq`、IO 线程候选或普通 worker。

Done When:
- evidence 中包含本轮新 waker 的线程名、进程名和分类信息。
- 若本轮没有新 waker，本步骤标记为 `skipped`。
- LLM 已获得足够信息，用于在 Step 4.D 更新新候选边状态。

Evidence Ref: `ev.critical_path.thread_identity.*`

Decision:
- 待定

Branch Rules:
- 若本轮新 waker 是 `udk-irq`，只将对应候选边标记为 `terminal`，`terminal_reason=irq_terminal`，不得终止全局循环。
- 若本轮新 waker 命中 IO 线程集合且不在排除集合，只将对应候选边标记为 `terminal` 或 `explained`，`terminal_reason=io_thread_terminal`。
- 若本轮新 waker 是普通 worker，将对应候选边保留为 `status=pending`。
- 不解析历史候选池中的全部 waker；历史候选应在首次发现那轮解析并记录。

Loop Until:
- 本步骤不独立循环；随 Step 4 每轮执行一次。

## Step 4.D. LLM 评审并回到全局候选池

Status: todo
Type: llm-review
Probe: none
Tables:
- 无

Inputs:
- `ev.critical_path.thread_state_profile.*`
- `ev.critical_path.thread_identity.*`
- `analysis_state.json`

Params:
- 无

Goal: 基于证据更新分析状态，并决定是否追加派生步骤或进入下一轮候选选择。

Done When:
- `analysis_state.json` 已更新 `visited_edges/frontier.next_candidate_edges/coverage/hypotheses`。
- 当前 `selected_edge` 已被标记为 `visited/explained/terminal`。
- 如果需要继续，LLM 已回到 Step 4.A，从全局候选池挑选下一个 `status=pending` 候选。
- 如果不需要继续，已进入 Step 5/6 补充上下文，或进入 Step 7 综合输出。

Evidence Ref: 无

Decision:
- 待定

Branch Rules:
- `max_depth`、环、`udk-irq`、缺失证据等只更新当前候选边的 `terminal_reason`，不设置全局 `stop_reason`。
- 如果当前候选窗口已由 Running/调度切片/调用栈或终端边界解释，将其区间加入 `coverage.explained_intervals`。
- 只有当 LLM 基于证据判断关键路径已解释约 99% 时间，才停止从候选池继续选择。
- 若覆盖率未达到 99%，必须回到 Step 4.A 继续挑选有价值的 pending 候选，除非候选池耗尽。

Loop Until:
- 本步骤不独立循环；执行后回到 Step 4，由 Step 4 的 `Loop Until` 判断是否继续下一轮。

## Step 5. 查询调度切片画像

Status: todo
Type: call-probe
Probe: `critical_path.sched_profile`
Tables:
- `sched_slice`

Inputs:
- `trace`: `<trace_or_db_path>`
- `itid`: 当前选中或需要解释的线程
- `start_ts`: 当前窗口开始时间
- `end_ts`: 当前窗口结束时间

Params:
- `itid`: `<current_itid>`
- `start_ts`: `<window_start_ts>`
- `end_ts`: `<window_end_ts>`
- `max_rows`: `2000`

Goal: 对 Running/Runnable 相关窗口补充 CPU 执行事实。

Done When:
- evidence 中包含调度切片，或明确返回 `empty_result`。
- LLM 已记录调度事实是否支撑自身执行或调度等待。

Evidence Ref: `ev.critical_path.sched_profile.*`

Decision:
- 待定

Branch Rules:
- 若 `thread_state` 显示主要为 `Running`，且 `sched_running_ns` 与 running 时长接近，可作为自身执行证据。
- 若 Runnable 占比高但 `sched_slice` 稀少，作为调度等待事实输入，不直接写成根因。

Loop Until:
- 每个需要补充调度证据的窗口执行一次。

## Step 6. 查询调用栈上下文

Status: todo
Type: call-probe
Probe: `critical_path.callstack_context`
Tables:
- `callstack`
- `thread`
- `process`

Inputs:
- `trace`: `<trace_or_db_path>`
- `itid`: 当前选中或需要解释的线程，可选但建议提供
- `start_ts`: 当前窗口开始时间
- `end_ts`: 当前窗口结束时间

Params:
- `itid`: `<current_itid>`
- `start_ts`: `<window_start_ts>`
- `end_ts`: `<window_end_ts>`
- `max_rows`: `2000`

Goal: 获取窗口函数上下文，辅助解释 Running 或阻塞函数，但不单独作为关键路径证据。

Done When:
- evidence 中包含调用栈 span 或 top names，或明确返回 `empty_result`。

Evidence Ref: `ev.critical_path.callstack_context.*`

Decision:
- 待定

Branch Rules:
- 调用栈只能作为上下文，不能替代 `thread_state`、调度切片或唤醒边事实。
- 对关键路径窗口应尽量传入 `itid`，避免不同线程 span 混杂。

Loop Until:
- 每个需要补充调用栈上下文的窗口执行一次。

## Step 7. 综合输出

Status: todo
Type: synthesize
Probe: none
Tables:
- 无

Inputs:
- 所有 evidence 引用
- `analysis_state.json`

Params:
- 无

Goal: 输出事实、推断和不确定性。

Done When:
- 报告区分事实、推断和不确定性。
- 每条事实引用 evidence。
- 没有把最长片段直接写成根因。

Evidence Ref: 无

Decision:
- 待定

Branch Rules:
- 若证据不足，必须说明缺口，不生成强结论。
- `next_candidate_edges` 是探索候选池，不是最终关键路径。
- 最终结论必须说明哪些 root-window 区间已解释、哪些区间仍不确定。

Loop Until:
- 报告已生成
