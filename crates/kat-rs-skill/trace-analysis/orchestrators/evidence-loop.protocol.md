# Evidence Loop Protocol

本协议指导 LLM 在 checklist 执行过程中如何根据 CLI evidence 继续推进分析。

所有运行状态都位于调用者当前工作目录的 `.kat/runs/<run_id>/`。不要把 state、checklist、evidence 或报告草稿写入 SKILL 目录。

## 每轮输入

- `.kat/runs/<run_id>/checklist.md`
- `.kat/runs/<run_id>/analysis_state.json`
- `.kat/runs/<run_id>/evidence.jsonl`
- 上一轮 CLI probe 输出

## 每轮流程

1. 读取 `.kat/runs/<run_id>/checklist.md` 中第一个 `Status: todo` 的 step。
2. 若 step 类型为 `call-probe`，准备输入并调用对应 CLI registry probe。
3. probe 执行后，将 CLI 输出追加到 `.kat/runs/<run_id>/evidence.jsonl`。
4. 检查 evidence 的 `status`、`facts`、`limitations`。
5. 将当前 step 标记为 `done`、`blocked` 或 `skipped`。
6. LLM 基于 evidence 更新 `.kat/runs/<run_id>/analysis_state.json` 和 `.kat/runs/<run_id>/checklist.md`。
7. 若问题已经可回答，进入 `synthesize` step。
8. 若问题不可回答且现有 checklist 不足以继续，追加 `append-step` 派生步骤。
9. 若问题不可回答但候选池仍有价值，回到下一个 `llm-review` 决策点选择下一步 probe 调用。

## Probe 调用

每个 probe 调用必须通过 CLI 执行：

```sh
kat-rs probe run --probe <probe_id> --source sqlite --file <trace_or_db_path> --params-file .kat/runs/<run_id>/<probe>.params.json --run-dir .kat/runs/<run_id>
```

trace/db 输入通过 `--file <trace_or_db_path>` 传入；probe 参数先写入 `.kat/runs/<run_id>/<probe>.params.json`，再通过 `--params-file` 传入。

probe 只产出确定性 evidence。probe 可以返回局部事实、局部边界提示和候选更新建议，但是否更新全局 state、是否追加 step、是否进入最终报告，均由 LLM 在 evidence loop 中决定。

## Step 状态

- `todo`: 尚未执行。
- `running`: 正在执行。
- `done`: 已完成并写入 evidence 或 decision。
- `skipped`: 基于 evidence 判定无需执行。
- `blocked`: 缺少输入、缺少表、probe 失败或 trace 不支持。

## Evidence 解读

- `ok`: 可以作为事实引用。
- `empty_result`: 可以作为“未发现”的事实引用。
- `partial`: 可以引用，但必须同步写入不确定性。
- `failed`: 不能作为事实，只能作为系统缺口。

## 派生 Step 规则

LLM 追加派生 step 时必须说明：

- 来源 step。
- 使用了哪些 evidence。
- 为什么需要继续。
- 下一步调用哪个 registry probe。
- 新 probe 输入从哪里来。
- 何时停止。

派生 step 只能表达下一步可执行编排；跨轮候选池、coverage、visited set 和复杂决策状态必须写入 `.kat/runs/<run_id>/analysis_state.json`。

## 全局决策规则

当 strategy 需要多轮探索时：

- `analysis_state.json` 持有 frontier、visited、coverage、selected item 和全局退出条件。
- checklist 中用 `llm-review` 记录每轮继续、跳过、补充或结束的决策。
- 局部边界条件只更新当前候选状态，例如 `terminal`、`explained`、`skipped` 或 `blocked`。
- 每轮结束后，LLM 必须先更新 state，再判断是继续下一轮、追加补充 step，还是进入 `synthesize`。

## 禁止事项

- 禁止根据没有 evidence 的假设追加深层递归。
- 禁止在 checklist 中写复杂循环算法或多层嵌套循环体。
- 禁止跳过 `analysis_state.json` 直接凭记忆推进。
- 禁止把 probe 的局部判断直接写成最终根因。
