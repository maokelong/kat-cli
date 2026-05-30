# htrace Run Workflow State 设计

## 背景

当前 `harmony-trace-analysis` skill 通过 `SKILL.md` 描述 8 阶段分析流程：

1. 收集输入
2. 加载 profile
3. 执行 overview atomics
4. 编写 Topdown Brief
5. 选择或生成策略
6. 执行深度分析
7. 生成 replay YAML
8. 输出最终报告

这个流程目前主要依赖 LLM 阅读并遵守文字说明。遇到上下文压缩、长时间任务、用户说“继续”、或模型从历史验证产物中恢复时，容易出现两类问题：

- 模型跳过中间阶段，例如未执行 overview atomics 就写 Topdown Brief，或未生成 replay 就写最终报告。
- 用户无法感知当前流程进度，不知道已经完成了什么、正在做什么、下一步是什么、是否存在阻塞。

因此需要把流程状态从上下文中外部化为持久 artifact，并由 Rust CLI 提供最小阶段门禁。

## 目标

- 让 `run-state.yaml` 成为单次分析任务的流程事实源。
- 让 `progress.md` 成为用户可读的进度面板。
- 让 OpenCode 在上下文压缩后可以从 `.last-run -> run-state.yaml -> progress.md` 恢复。
- 让 Rust CLI 提供 `init/status/guard/advance`，阻止明显跳阶段。
- 保持当前 8 阶段分析流程不变。
- 先用文件格式落地，再逐步把阶段门禁接入 CLI。

## 非目标

- 不在第一轮实现完整 workflow DSL。
- 不在第一轮实现可视化 UI。
- 不在第一轮实现 replay judge、assertion evaluator 或 signature 判定器。
- 不让 CLI 接管 LLM 的 Topdown Brief、策略选择或报告撰写。
- 不把用户运行产物提交到 git。

## 业内参考模式

本设计吸收以下成熟模式：

- LangGraph checkpoint/thread state：把流程状态持久化，支持恢复、人审和失败续跑。
- Temporal durable execution/event history：把工作流进展外部化，依靠确定性状态恢复。
- Prefect/Airflow task state：把 task/run 状态作为一等对象，暴露 running、completed、failed、skipped 等生命周期。
- BPMN/Camunda process instance：用显式阶段、网关和当前节点表达流程进度。
- Agent tracing/span：记录 tool call、阶段切换和失败原因，便于用户和开发者追踪。

对本项目的适配原则是：不引入完整工作流引擎，先用轻量状态文件和 CLI guard 获得 80% 的可靠性。

## 架构

新增运行状态层：

```text
OpenCode / LLM
  -> 读取 SKILL.md
  -> 读取或创建 run-state.yaml
  -> 调用 htrace run status
  -> 调用 htrace run guard
  -> 调用 htrace atomic / replay
  -> 调用 htrace run advance
  -> 刷新 progress.md
  -> 输出阶段进度和最终报告
```

新增运行目录：

```text
runs/<run-id>/
  run-state.yaml
  progress.md
  evidence/
  artifacts/
.last-run
```

职责边界：

- `run-state.yaml`：机器可读事实源，记录阶段状态、关键输入、产物、决策和阻塞原因。
- `progress.md`：用户可读展示层，由 `run-state.yaml` 派生。
- `evidence/`：atomic 输出、SQL、CSV、stderr、结构化 envelope。
- `artifacts/`：Topdown Brief、strategy draft、replay YAML、final report。
- `.last-run`：记录最近运行目录，支持用户说“继续”时恢复。
- `validation/`：开发验证产物，不作为当前用户任务状态。

`runs/` 和 `.last-run` 必须加入 `.gitignore`。

## 状态模型

最小 `run-state.yaml`：

```yaml
schema_version: 1
run_id: 20260528-103012
status: running
current_stage: collect_input
created_at: "2026-05-28T10:30:12+08:00"
updated_at: "2026-05-28T10:30:12+08:00"

trace:
  path: D:\path\to\trace.htrace
  kind: htrace

question:
  raw: 冷启动为什么慢
  domain_hint: scheduler-kernel
  target_process_hint: wechat

profile:
  selected: null
  router_result: null
  knowledge_loaded: []

stages:
  collect_input:
    status: in_progress
    started_at: "2026-05-28T10:30:12+08:00"
    completed_at: null
    artifacts: []
  load_profile:
    status: pending
    artifacts: []
  overview_atomics:
    status: pending
    artifacts: []
  topdown_brief:
    status: pending
    artifacts: []
  strategy_selection:
    status: pending
    artifacts: []
  deep_analysis:
    status: pending
    artifacts: []
  replay_generation:
    status: pending
    artifacts: []
  final_report:
    status: pending
    artifacts: []

decisions: []
blocked_reason: null
```

阶段状态枚举：

```text
pending
in_progress
completed
blocked
skipped
failed
```

固定阶段 ID：

```text
collect_input
load_profile
overview_atomics
topdown_brief
strategy_selection
deep_analysis
replay_generation
final_report
```

## 阶段门禁

阶段顺序：

```text
collect_input -> load_profile -> overview_atomics -> topdown_brief -> strategy_selection -> deep_analysis -> replay_generation -> final_report
```

门禁规则：

- `collect_input -> load_profile`：要求 `trace.path` 和 `question.raw` 存在。
- `load_profile -> overview_atomics`：要求 `profile.selected` 存在，且 profile 文件可读。
- `overview_atomics -> topdown_brief`：要求至少 `trace_sanity_check` 成功，overview evidence 已落盘。
- `topdown_brief -> strategy_selection`：要求 `artifacts/topdown-brief.md` 存在，且引用 overview evidence。
- `strategy_selection -> deep_analysis`：要求已选择 approved strategy，或 draft strategy 已经用户审核通过。
- `deep_analysis -> replay_generation`：要求参与最终判断的 atomic 已执行，关键 evidence 已落盘。
- `replay_generation -> final_report`：要求 replay YAML 或 signature YAML 已生成。
- `final_report -> completed`：要求 `artifacts/final-report.md` 存在，包含 replay 路径和不确定性。

第一轮可以把规则写入 Rust 常量表 `RunWorkflow`。等稳定后，再考虑外置为 `skill/config/workflow.yaml`。

## CLI 设计

新增命令组：

```text
htrace run init
htrace run status
htrace run guard
htrace run advance
```

### run init

创建 run 目录和初始状态。

```powershell
htrace run init `
  --out runs `
  --trace D:\work\smartperf\test\test.htrace `
  --question "冷启动为什么慢" `
  --domain scheduler-kernel `
  --target-process wechat
```

输出：

```json
{
  "run_id": "20260528-103012",
  "run_dir": "runs/20260528-103012",
  "state": "runs/20260528-103012/run-state.yaml",
  "progress": "runs/20260528-103012/progress.md",
  "current_stage": "collect_input"
}
```

### run status

读取 `run-state.yaml`，输出机器可读状态，并刷新 `progress.md`。

```powershell
htrace run status runs/20260528-103012 --json
```

输出：

```json
{
  "run_id": "20260528-103012",
  "status": "running",
  "current_stage": "overview_atomics",
  "completed_stages": ["collect_input", "load_profile"],
  "next_allowed": ["run_overview_atomic", "complete_overview_atomics"],
  "blocked_reason": null,
  "progress": "runs/20260528-103012/progress.md"
}
```

### run guard

检查当前动作是否允许执行。OpenCode 每次跨阶段或执行阶段关键动作前调用。

```powershell
htrace run guard runs/20260528-103012 --action write_topdown_brief --json
```

允许时：

```json
{
  "allowed": true,
  "current_stage": "topdown_brief",
  "action": "write_topdown_brief"
}
```

拒绝时：

```json
{
  "allowed": false,
  "current_stage": "overview_atomics",
  "action": "write_topdown_brief",
  "reason": "overview_atomics 未完成，不能写 Topdown Brief"
}
```

### run advance

完成当前阶段并进入下一阶段。

```powershell
htrace run advance runs/20260528-103012 `
  --from overview_atomics `
  --to topdown_brief `
  --artifact evidence/overview/trace_sanity_check.csv `
  --artifact evidence/overview/process_startup_candidates.csv `
  --decision "overview atomics completed"
```

CLI 行为：

1. 校验 `--from` 是当前阶段。
2. 校验 `--to` 是合法下一阶段。
3. 校验必要 artifact 存在。
4. 更新 `run-state.yaml`。
5. 刷新 `progress.md`。

输出：

```json
{
  "advanced": true,
  "from": "overview_atomics",
  "to": "topdown_brief",
  "progress": "runs/20260528-103012/progress.md"
}
```

## 阶段动作映射

```text
collect_input:
  - complete_input

load_profile:
  - route_profile
  - complete_profile

overview_atomics:
  - run_overview_atomic
  - complete_overview_atomics

topdown_brief:
  - write_topdown_brief
  - complete_topdown_brief

strategy_selection:
  - select_approved_strategy
  - generate_draft_strategy
  - request_strategy_review
  - approve_draft_strategy
  - complete_strategy_selection

deep_analysis:
  - run_strategy_atomic
  - branch_strategy
  - complete_deep_analysis

replay_generation:
  - write_replay
  - validate_replay
  - complete_replay_generation

final_report:
  - write_final_report
  - complete_final_report
```

## progress.md

`progress.md` 从 `run-state.yaml` 派生，面向用户展示。

示例：

```markdown
# 分析进度

Run ID：20260528-103012
Trace：D:\work\smartperf\test\test.htrace
问题：冷启动为什么慢

## 当前阶段

overview_atomics：执行 profile overview atomics

## 已完成

- collect_input：已收集 trace、问题、领域提示
- load_profile：已选择 scheduler-kernel profile

## 正在进行

运行 overview atomic，确认当前 trace 中实际存在什么异常信号。

## 下一步

进入 topdown_brief，基于 overview evidence 总结当前 trace 的问题形态。

## 阻塞项

无

## 关键产物

- run-state.yaml
- evidence/overview/
```

OpenCode 每次阶段切换后，应向用户短报：

```text
已完成 overview_atomics，证据写入 evidence/overview/。现在进入 topdown_brief：我会先总结这个 trace 里实际存在的异常信号，再决定策略。
```

如果 guard 拒绝：

```text
我还不能进入 final_report：replay_generation 未完成，缺少 artifacts/replay.yaml。先回到 replay_generation 阶段补齐。
```

## 上下文压缩恢复

`SKILL.md` 需要新增硬规则：

```text
如果当前对话可能经过上下文压缩，或用户说“继续”，或不确定当前阶段：
1. 不要继续推理。
2. 先定位 run-state.yaml。
3. 调用 htrace run status。
4. 读取 progress.md。
5. 用 current_stage 恢复流程。
6. 向用户报告当前阶段、已完成、下一步。
```

恢复时禁止：

```text
- 直接重跑全部流程
- 直接写最终报告
- 用 validation/ 旧结果代替当前 run evidence
- 跳过 guard
```

状态恢复优先级：

1. 用户显式指定的 run 目录。
2. 当前工作目录 `.last-run`。
3. 最近修改的 `runs/*/run-state.yaml`。
4. 如果都没有，创建新 run。

## OpenCode Skill 约束更新

`skill/SKILL.md` 应新增：

- 每次开始或恢复分析时，必须先查找或创建 run。
- 每次执行阶段关键动作前，必须调用 `htrace run guard`。
- 每次阶段完成后，必须调用 `htrace run advance`。
- 每次用户可见更新时，报告 `progress.md` 中的当前阶段、已完成、下一步和阻塞项。
- 不允许用 `validation/` 历史产物替代当前 run evidence。

## 实施范围

新增文件：

```text
cli/src/commands/run.rs
cli/src/run/mod.rs
cli/src/run/model.rs
cli/src/run/workflow.rs
cli/src/run/progress.rs
```

修改文件：

```text
cli/src/main.rs
cli/src/commands/mod.rs
.gitignore
skill/SKILL.md
docs/RUST_CLI_ARCHITECTURE.md
docs/NEXT_ITERATION_HANDOFF.md
```

第一轮不修改 atomic/replay judge 的语义，只增加流程状态和阶段门禁。

## 测试方法

单元测试：

- 初始化 run state。
- 阶段顺序合法性。
- guard allowed / denied。
- advance from/to 校验。
- progress.md 渲染。
- `.last-run` 写入。

CLI 集成测试：

```powershell
htrace run init --out runs --trace sample.htrace --question "冷启动为什么慢" --domain scheduler-kernel
htrace run status runs/<run-id> --json
htrace run guard runs/<run-id> --action complete_input --json
htrace run advance runs/<run-id> --from collect_input --to load_profile --decision "input collected"
htrace run guard runs/<run-id> --action write_final_report --json
```

预期：

- `init` 创建 `run-state.yaml`、`progress.md`、`evidence/`、`artifacts/`。
- `status` 输出 `current_stage`。
- 合法 `guard` 返回 `allowed=true`。
- 非法 `guard` 返回 `allowed=false` 和原因。
- `advance` 更新 `current_stage` 并刷新 `progress.md`。

OpenCode 行为测试：

- 用户说“继续”：先读 `.last-run`，再 `status`，再报告当前阶段。
- 模型试图直接写 final report：`guard` 拒绝，回到缺失阶段。
- 上下文压缩后恢复：不重跑全部流程，不使用 `validation/` 旧产物，按 `run-state.yaml` 继续。

## 完成标准

- `run-state.yaml` 成为流程事实源。
- `progress.md` 成为用户可读进度面板。
- `SKILL.md` 明确要求每次恢复先 `run status`。
- `htrace run guard` 能阻止明显跳阶段。
- `htrace run advance` 能推进阶段并刷新 progress。
- `runs/` 和 `.last-run` 不进入 git。
- 文档说明 `validation/` 与 `runs/` 的边界。
- 测试覆盖状态模型、阶段门禁和 progress 渲染。

## 风险与缓解

- 风险：第一版规则写死在 Rust 中，后续领域扩展可能需要更灵活的流程。
  缓解：先用常量表实现，稳定后再外置为 `skill/config/workflow.yaml`。
- 风险：OpenCode 仍可能忘记调用 guard。
  缓解：在 `SKILL.md` 中把 `run status/guard/advance` 写成硬约束，并在 progress 中暴露当前合法动作。
- 风险：artifact 校验过严影响探索性分析。
  缓解：第一版只校验阶段关键产物，允许阶段内多次试探 atomic。
- 风险：用户运行产物泄露到 git。
  缓解：把 `runs/` 和 `.last-run` 加入 `.gitignore`，文档明确边界。
