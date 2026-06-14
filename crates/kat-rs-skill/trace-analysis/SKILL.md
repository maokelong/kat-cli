---
name: trace-analysis
description: 鸿蒙 trace 相关问题分析入口
---

# Trace 通用分析入口

你是 `kat-rs-skill` 的统一入口和 LLM 决策层。你的职责是读取用户问题和专家 strategy，经 orchestrators 生成 checklist，并按 evidence loop 推进分析。

SKILL 只保存决策、编排和证据解读规则；不保存 probe 脚本，不保存运行产物。

## 工作协议

1. 理解用户问题、trace 输入、时间窗口和目标对象。
2. 选择或请求一个 strategy pack。
3. 读取 `orchestrators/strategy-to-probes.protocol.md`，将 strategy 摘要成 checklist 生成所需的结构化信息。
4. 基于 strategy 摘要、CLI registry probe 能力和 `orchestrators/checklist.template.md` 生成显式 Markdown checklist。
5. 按 `orchestrators/evidence-loop.protocol.md` 执行 checklist。
6. 每个执行步骤只能调用 CLI registry probe，或执行 `llm-review`、`append-step`、`synthesize` 决策。
7. probe 只产出确定性 evidence，不直接产出最终根因。
8. LLM 读取 evidence 后更新 state、checklist 和报告草稿。
9. 最终报告必须分为事实、推断和不确定性。

## 运行产物目录

每次分析的运行产物必须写入调用者当前工作目录下的 `.kat/runs/<run_id>/`，包括：

- `question.md`
- `analysis_state.json`
- `checklist.md`
- `evidence.jsonl`
- 报告草稿和派生说明

不要把运行产物写入 SKILL 目录。

## Probe Registry 约束

probe registry 位于 `crates/kat-rs-cli/trace-registry/`。SKILL 只引用 registry 中声明的 probe 能力，不维护 probe 脚本或每次运行生成的 probe 代码。

每次 probe 调用必须通过 CLI 执行：

```sh
kat-rs probe run --probe <probe_id> --source sqlite --file <trace_or_db_path> --params-file .kat/runs/<run_id>/<probe>.params.json --run-dir .kat/runs/<run_id>
```

trace/db 输入通过 `--file <trace_or_db_path>` 传入；probe 参数先写入 `.kat/runs/<run_id>/<probe>.params.json`，再通过 `--params-file` 传入。

证据来自 CLI 输出并追加到 `.kat/runs/<run_id>/evidence.jsonl`。如果现有 registry probe 不足，LLM 只能输出最小 probe 能力需求和 CLI registry 变更建议；相关代码变更应进入 `crates/kat-rs-cli/trace-registry/` 以及必要的 operators。

## Checklist 编排约束

LLM 必须先读取并遵循：

- Strategy To Probes Protocol: `orchestrators/strategy-to-probes.protocol.md`
- Evidence Loop Protocol: `orchestrators/evidence-loop.protocol.md`
- Checklist Template: `orchestrators/checklist.template.md`

每次分析必须基于 strategy 和 registry probe 能力生成一个显式 Markdown checklist，并写入 `.kat/runs/<run_id>/checklist.md`。

生成 checklist 时必须保留模板字段结构：

- `Status`
- `Type`
- `Probe`
- `Inputs`
- `Params`
- `Goal`
- `Done When`
- `Evidence Ref`
- `Decision`
- `Branch Rules`
- `Loop Until`

checklist 只表达 LLM 可见的简单编排：`call-probe`、`llm-review`、`append-step`、`synthesize`。复杂分支、递归栈、候选池、覆盖率和跨轮状态必须写入 `.kat/runs/<run_id>/analysis_state.json`，不得塞进 checklist。

## 禁止事项

- 禁止把没有 evidence 支撑的推断写成事实。
- 禁止把最长耗时片段直接写成根因。
- 禁止跳过 checklist 直接给结论。
- 禁止脱离 `checklist.template.md` 自由编写 checklist 格式。
- 禁止把复杂递归状态机写进 checklist。
- 禁止让 probe 直接输出最终根因。
- 禁止把 strategy 的终止条件不分范围地写成全局停止条件。
