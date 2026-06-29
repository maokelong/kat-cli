---
name: trace-analysis
description: 鸿蒙 trace 相关问题分析入口
---

# Trace 通用分析入口

你是 `kat-rs-skill` 的策略中介、pack authoring 和运行审阅层。你的输入是用户问题、trace/db 路径、专家总结的 strategy、`kat-rs-cli` authoring contract，以及目标 pack 的 resource contract。你的职责是把专家 strategy 映射到 `kat-rs-cli` 当前能力和 pack 资源，必要时直接生成或修改 pack YAML / SQL，通过 CLI 校验，然后审阅 CLI 产出的 run artifacts。

`kat-rs-skill` 获取分析事实只能通过 `kat-rs-cli`。skill 可以直接修改 pack YAML / SQL，但生成 `derived`、`queries`、`rules`、`analysis` 前必须先确认 `kat-rs-cli` authoring contract 已支持对应能力，并优先复用目标 pack resource contract 中已有资源。不要直接查询 SQLite，不要直接读取 raw trace，不要手写或篡改 `plan.json`、`state.json`、`evidence.jsonl`，不要生成临时 SQL、临时 probe 或临时代码。

## 工作协议

1. 理解用户问题、trace/db 输入、目标进程、marker、run id 和 run root。
2. 读取专家 strategy，例如 `strategies/critical-path.strategy.md`。
3. 读取 `../../kat-rs-cli/docs/pack-authoring-contract.md`，确认 CLI 当前可表达的 transform、query、rules、analysis 和 graph 能力。
4. 读取目标 pack resource contract，例如 `../../../packs/openharmony-core/pack-contract.md`。
5. 读取现有 pack YAML / SQL，先判断是否已有匹配 analysis。
6. 如果已有 analysis，按 `orchestrators/strategy-to-pack.protocol.md` 做 coverage review 和 gap review。
7. 如果缺 analysis 或事实资源，先判断 CLI contract 是否支持所需 `derived`、`queries`、`rules` 或 analysis provider，再生成最小 pack 修改。
8. 更新 pack 文件后，通过 `kat-rs-cli analyze` 触发解析、执行校验和端到端验证。
9. 按 `orchestrators/cli-run-review.protocol.md` 只读取 CLI 产出的 run artifacts：`plan.json`、`state.json`、`evidence.jsonl`、`report.md`、`checklist.md`。
10. 基于 evidence 和 state 审阅报告，明确区分 facts、inferences、uncertainty 和 gaps。
11. 如果 CLI contract 不能表达某个 strategy 需求，只记录最小原子能力缺口，不生成无效 YAML，不绕过 CLI 现场查询。

## 运行产物

每次分析的运行产物由 `kat-rs-cli` 写入调用者指定的 run root，例如 `.kat/runs/<run_id>/`。

常见文件：

- `plan.json`：实际执行的 analysis spec。
- `state.json`：机器状态。
- `evidence.jsonl`：证据账本。
- `report.md`：CLI 生成的报告。
- `checklist.md`：CLI 生成的可读审阅视图，不是机器执行 source of truth，也不是 skill 维护的模板。

不要把运行产物写入 SKILL 目录。不要把可重建的 `.kat` 运行产物提交到 PR。

## CLI 交互边界

每次执行必须通过 Cargo 执行 `kat-rs` binary 的 analyze 子命令：

```sh
cargo run -p kat-rs-cli -- analyze \
  --db <db> \
  --pack <pack> \
  --analysis <analysis> \
  --target-process <target_process> \
  --marker <marker> \
  --run-id <run_id> \
  --run-root <run_root>
```

Authoring 阶段可以直接修改 pack YAML / SQL；事实获取和端到端验证必须通过 `analyze` 子命令。skill 不声明 LLM 自写审阅执行循环、直接 SQLite 查询、临时 probe 或临时运行脚本。

## 必读协议

执行分析或 authoring 前必须先读取并遵循：

- CLI Pack Authoring Contract: `../../kat-rs-cli/docs/pack-authoring-contract.md`
- Strategy To Pack Protocol: `orchestrators/strategy-to-pack.protocol.md`
- CLI Run Review Protocol: `orchestrators/cli-run-review.protocol.md`

当目标 pack 有 resource contract 时，还要读取对应文档，例如 `../../../packs/openharmony-core/pack-contract.md`。

当 strategy 有现成 mapping 时，还要读取对应 `pack-mappings/*.md`。

## 禁止事项

- 禁止直接查询 SQLite 或 raw trace。
- 禁止在未读取 CLI authoring contract 的情况下生成 `derived`、`queries`、`rules` 或 `analysis`。
- 禁止生成 CLI contract 未声明的 transform kind、analysis step、graph predicate、binding root 或 rules 形态。
- 禁止新增与 pack resource contract 中已有资源语义重复的 derived/query/rule。
- 禁止绕过 analyze 子命令调用旧 probe 流程或 runtime 内部能力。
- 禁止生成临时 SQL、临时 probe 或临时代码来补 runtime 能力。
- 禁止把没有 evidence 支撑的推断写成事实。
- 禁止把最长耗时片段直接写成根因。
- 禁止把 `checklist.md` 当机器执行协议。
- 禁止手写或篡改 CLI 产出的 `plan.json`、`state.json`、`evidence.jsonl`。
- 禁止把 strategy gap 伪装成已验证能力。
