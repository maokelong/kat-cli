---
schema: trace-checklist.v1
analysis_id: run-sample
strategy_id: selected-strategy
executor: llm
status: running
---

# Trace 分析工作清单

## 策略摘要

Strategy Intent: `<本次分析要回答的问题>`

Root Object:
- `<根进程/根线程/根事件/根时间窗口的解析方式>`

Main Evidence Plan:
- `<主证据链，例如线程状态、唤醒边、调度切片、调用栈上下文>`

Analysis State:
- `<需要维护的跨轮状态，例如 frontier、visited、coverage、hypotheses、selected_item>`

Boundary Scope:
- Candidate Boundary: `<只终止当前候选的条件>`
- Global Exit: `<允许结束全局分析的条件>`

## Step 1. 检查 Trace

Status: todo
Type: call-probe
Probe: `trace.inspect`
Inputs:
- `trace`

Params:
- 无

Goal: 确认 trace 可解析并列出可用表。

Done When:
- 已获得 trace bounds。
- 已获得可用表清单。
- 已记录缺失表。

Evidence Ref: `ev.trace.inspect`

Decision:
- pending

Branch Rules:
- 若关键表缺失，后续依赖该表的 step 标记为 `blocked` 或 `partial`。

Loop Until:
- 执行一次

## Step N. 父级循环示例

Status: todo
Type: llm-review
Probe: none
Inputs:
- `analysis_state.json`
- 最新 evidence refs

Params:
- `<策略定义的全局阈值或退出参数>`

Goal: 基于 evidence 和 `analysis_state.json` 判断全局探索是否继续；循环状态继续写入 `analysis_state.json`。

Done When:
- 达到 strategy 定义的全局退出条件。
- 或没有仍有探索价值的候选。

Evidence Ref: 无

Decision:
- pending

Branch Rules:
- 局部边界条件只更新当前候选状态，不直接结束全局循环。
- 后续探索通过 `append-step` 派生下一步；子步骤只表达一轮动作，不独立控制循环退出。

Loop Until:
- `<Global Exit 条件成立>`
- 或 `<候选池耗尽/问题已可回答/无法继续>`

## Step N.A. 一轮循环体示例

Status: todo
Type: call-probe
Probe: `<one.probe>`
Inputs:
- `<来自 analysis_state.json 或上一轮 evidence 的输入>`

Params:
- `<probe 参数>`

Goal: 生成本轮需要的一类确定性 evidence。

Done When:
- evidence 包含本轮所需事实，或明确返回 `empty_result` / `partial`。

Evidence Ref: `ev.<probe>.<seq>`

Decision:
- pending

Branch Rules:
- 根据 evidence 更新 `analysis_state.json`。
- 若触发局部边界，只标记当前候选，不结束父级循环。

Loop Until:
- 本步骤不独立循环；随父级循环每轮执行一次。

## Step M. 基于证据判断下一步

Status: todo
Type: llm-review
Probe: none
Inputs:
- Evidence refs from previous steps

Params:
- 无

Goal: 判断当前问题是否已经可回答；若不可回答，追加派生 step 调用下一个最小 probe。

Done When:
- 已更新 `analysis_state.json`。
- 已决定输出报告或追加下一步。

Evidence Ref: 无

Decision:
- pending

Branch Rules:
- 若 evidence 足够回答用户问题，进入 `synthesize` step。
- 若 evidence 不足但存在明确下一类事实需求，追加最小派生 step。
- 若缺少关键输入或 trace 不支持，标记为 `blocked` 并记录不确定性。

Loop Until:
- 执行一次
