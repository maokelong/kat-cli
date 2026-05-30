---
name: harmony-trace-analysis
description: 使用确定性的 htrace 原子能力、领域知识库和 topdown 策略分析鸿蒙/Perfetto-compatible 性能 trace。Use when Codex needs to analyze .htrace, .pftrace, or Perfetto-compatible traces for HarmonyOS performance issues including cold start, scheduling/kernel latency, storage, memory, CPU contention, blocking, IO wait, strategy selection, deterministic replay YAML generation, or evidence-based performance reports.
---

# 鸿蒙 Trace 分析

## Codex 调用入口

- 当用户显式提到 `$harmony-trace-analysis`，或要求 Codex 分析鸿蒙、HarmonyOS、Perfetto-compatible、`.htrace`、`.pftrace` 性能 trace 时，使用本 skill。
- 本目录即 skill root。所有 `--skill-root <skill-root>` 参数都指向包含本 `SKILL.md` 的目录。
- Codex 需要优先遵守本文件的流程约束，再读取本 skill 的 `config/`、`atomics/`、`knowledge/`、`references/` 和 `strategies/`。

## 操作原则

- 先证据后策略：先运行 overview atomics，基于当前 trace 的真实信号写 Topdown Brief，再选择或生成策略。
- 保持机制与策略分离：atomic、profile、knowledge、strategy、replay 均从本 skill 目录加载，不把分析策略硬编码到代码或临时脚本。
- 只加载必要资源：按领域读取对应 profile、knowledge 和 reference，不批量展开无关目录。
- 不复用旧结论：除非用户明确要求对比历史结果，不把 `validation/` 中旧产物当作当前 trace 证据。
- 保持确定性：CLI 和 atomic 只负责查询、落证据和判定明确规则；LLM 只负责 Topdown Brief、策略选择、分支判断和报告表达。

## 环境配置硬约束

- 每次使用本 skill 分析 trace 前，先确认环境：`htrace version` 可运行，且 `HTRACE_TRACE_PROCESSOR` 指向存在的 `trace_processor_shell` 或 `trace_processor`。
- 若 `htrace` 不可用，优先使用本 skill 包内 `bin/windows-x64/htrace.exe`；Windows 下若仍不可用，执行 `powershell -ExecutionPolicy Bypass -File <skill-root>\install.ps1` 安装并重新检查。
- 若 `HTRACE_TRACE_PROCESSOR` 未设置或路径不存在，优先使用本 skill 包内 `bin/windows-x64/trace_processor_shell.exe`，并为当前会话设置 `$env:HTRACE_TRACE_PROCESSOR`；Windows 下可同时用 `install.ps1` 写入用户环境。
- 环境未检查通过前，不得执行 perfetto engine 的 atomic/replay；若当前平台不是 Windows 且包内无对应二进制，先要求用户提供该平台的 `htrace` 和 `trace_processor_shell` 路径。
- 在报告或中间进度中，明确当前使用的 `htrace` 路径和 `HTRACE_TRACE_PROCESSOR` 路径，便于复现。

## Trace 查询硬约束

- 分析过程中不得直接调用 `trace_processor_shell.exe` 或 `trace_processor` 执行 `-Q`、`-q`、SQL 字符串或 SQL 文件。
- 唯一允许触发 trace processor 的入口是 `htrace atomic run`、`htrace replay run` 和 `htrace replay batch`；trace processor 只能作为 `HTRACE_TRACE_PROCESSOR` 后端由 `htrace` 间接调用。
- SQL 必须写在 `atomics/<domain>/*.yaml` 中，通过 atomic id 调用；不得把 SQL 硬编码到 shell 命令、临时脚本或最终报告中。
- 当用户、策略、replay 或上一阶段明确指定 atomic id 时，必须原样执行该 atomic id，不得擅自替换为 `trace_sanity_check`、overview atomic 或“相似” atomic。
- 如果指定 atomic 没有出现在 skill 文件采样列表中，先用 `htrace atomic list --skill-root <skill-root>` 确认 id，再读取对应 `atomics/<domain>/<id>.yaml`。
- 若现有 atomic 不能回答问题，先生成或修改 YAML atomic/strategy draft 并请求用户审核；不得为了“先查一下”绕过 atomic 机制。
- `trace_processor_shell` 的加载日志、`Trace health issues`、`column N = ...` 等 stderr 信息不是用户可直接消费的分析证据。必须以 `htrace` 的 JSON 输出、atomic artifact 和报告中的证据链为准。
- 如果发现自己已经直接执行了 trace processor，立即停止该路径，向用户说明这是执行约束违规，并使用对应 atomic 或新增 atomic 重新执行。

## 运行状态硬约束

- 每轮行动先定位 run：优先使用用户指定的 run 目录；其次读取 `.last-run`；若没有可恢复 run，先收集 `htrace run init` 必填输入（trace、question，可选 domain、target-process），再创建新 run。
- 定位 run 后第一条流程命令必须是 `htrace run go <run-dir> --json`。只以 `go` 返回的 `current_stage`、`next_action`、`stage.allowed_actions`、`stage.allowed_artifacts`、`findings` 作为当前行动依据。
- 当 `go.next_action=blocked` 时，先向用户说明中文阻塞项，不执行 atomic、replay 或报告写入。
- 阶段关键动作前可以额外执行 `htrace run guard <run-dir> --action <action> --json`；`guard` 是辅助检查，`go` 是主入口。
- 不得用 PowerShell 的 `$?` 或 shell exit code 代替 guard 语义判断；必须解析 JSON 中的 `allowed` 字段。新版 `htrace` 会在 `allowed=false` 时返回非 0，但流程判断仍以 JSON 字段为准。
- 写入阶段产物后执行 `htrace run validate <run-dir> --json`；只有没有 error finding 时，才调用 `htrace run advance` 推进阶段。
- `load_profile` 和 `strategy_selection` 是 decision-only 阶段：当前 CLI 通过 `run advance --decision ...` 写入 profile/strategy 决策，并在 advance 内部校验该阶段。不要在这些阶段制造必然失败的 pre-advance validate；应直接用带 decision 的 `run advance` 完成该阶段。若 `run advance` 失败，必须停止。
- 禁止手写 `run-state.yaml`，禁止手动把阶段标记为 completed，禁止绕过 `htrace run advance`。
- `run-state.yaml` 是事实源，`progress.md` 是用户可读进度；禁止把 `validation/` 旧产物当作当前 run evidence。

## 阶段可见性硬约束

- 不得只把阶段进度写入 `run-state.yaml` 或 `progress.md`；每次开始、继续、阶段切换、长耗时 atomic 执行前后，都必须用中文向用户展示当前阶段。
- 每次展示阶段时使用固定结构，方便用户判断分析走到哪里：

```text
【阶段】{index}/8 {key}：{name}
【已完成】<已完成阶段或关键证据>
【正在做】<本次将执行的 atomic/action>
【允许动作】{allowed_actions}
【允许产物】{allowed_artifacts}
【阻塞项】{findings 或 无}
【下一步】{next_action}
```

- 用户说“继续”时，先输出一次阶段面板，再执行任何分析命令。
- 每个耗时 atomic 执行前，先说明它要回答的证据问题；执行后用 1-3 行总结输出是否改变后续分支。
- 如果上下文压缩导致不确定当前状态，先从 `.last-run`、`run-state.yaml` 和 `progress.md` 恢复，再展示阶段面板，不得凭记忆跳阶段。

## 资源导航

- Profile 和路由：`config/role-router.yaml`、`config/profiles/*.yaml`。
- 原子能力：`atomics/<domain>/*.yaml`。
- Approved strategies：`strategies/approved/*.md`。
- Generated draft strategies：`strategies/generated/`。
- 领域知识：`knowledge/<domain>/*.md`，只读取当前领域需要的文件。
- 策略审核格式：需要生成 draft strategy 时读取 `references/strategy-review.md`。
- 报告格式：写最终报告前读取 `references/report-format.md`，并遵守中文摘要、目标进程选择、CPU contention 因果证据、Validation notes 和具体下一步分支要求。
- E2E 可观测性：运行 E2E、replay 或汇总报告前读取 `references/e2e-observability.md`，记录命令级和 atomic/replay 级耗时、退出码、输出字节数、trace 大小、工具版本与性能限制。
- 自进化流程：用户要求进化、改进或多轮评测本 skill 时读取 `references/evolution-loop.md`。

## 阶段流程

每轮先调用 `htrace run go <run-dir> --json`，按当前阶段的 `allowed_actions` 和 `allowed_artifacts` 行动；阶段产物写入后先 `validate`，再 `advance`。decision-only 阶段按上文例外处理：用 `run advance --decision` 写入决策，不运行必然失败的 pre-advance validate。

| 阶段 | 目标 | 完成门禁 |
| --- | --- | --- |
| `collect_input` | 确认 trace、问题、领域、进程和时间范围。 | 输入事实已确认。 |
| `load_profile` | 加载或路由 profile。 | `advance --decision` 写入选中的 profile id。 |
| `overview_atomics` | 运行 overview atomics。 | `evidence/overview/*.json` 或 `*.csv`。 |
| `topdown_brief` | 基于 overview evidence 写 Topdown Brief。 | `artifacts/topdown-brief.md`。 |
| `strategy_selection` | 选择 approved strategy 或请求审核 draft strategy。 | run decision 记录策略选择。 |
| `deep_analysis` | 按策略执行深度 atomics。 | `evidence/deep/*.json` 或 `*.csv`。 |
| `replay_generation` | 生成确定性 replay/signature。 | `artifacts/replay.yaml` 或 `artifacts/signature.yaml`。 |
| `final_report` | 输出最终中文分析报告。 | `artifacts/final-report.md`。 |

## 命令约定

优先使用已构建的 `htrace`；可直接调用 PATH 中的 `htrace`，也可调用 `<skill-root>\bin\windows-x64\htrace.exe`。

环境检查和设置：

```powershell
htrace version
Test-Path $env:HTRACE_TRACE_PROCESSOR
$env:HTRACE_TRACE_PROCESSOR="<skill-root>\bin\windows-x64\trace_processor_shell.exe"
```

常用命令：

```powershell
htrace run init --out runs --trace <trace> --question "<用户问题>" --domain <domain> --target-process <process> --json
htrace run go <run-dir> --json
htrace run validate <run-dir> --json
htrace run guard <run-dir> --action <action> --json
htrace run advance <run-dir> --from <stage> --to <stage|completed> --decision "<阶段完成说明或选中的 profile/strategy id>" --json
htrace profile route --skill-root <skill-root> --question "<用户问题>"
htrace atomic run --skill-root <skill-root> --engine perfetto <atomic-id> --trace <trace> --param key=value --json
htrace replay run <replay.yaml> --skill-root <skill-root> --trace <trace> --engine perfetto --json
htrace replay batch <replay.yaml> --skill-root <skill-root> --trace <trace-a> --trace <trace-b> --jobs 2 --engine perfetto
```

PowerShell 中必须这样检查 guard：

```powershell
$guardJson = htrace run guard <run-dir> --action <action> --json
$guard = $guardJson | ConvertFrom-Json
if (-not $guard.allowed) {
  throw $guard.reason
}
```

禁止命令：

```powershell
trace_processor_shell.exe -Q "SELECT ..."
trace_processor_shell.exe -q query.sql trace.htrace
<skill-root>\bin\windows-x64\trace_processor_shell.exe -Q "SELECT ..." <trace>
```

## 分支与策略

- 先使用 profile 的 overview atomics 定位问题形态，再选择 strategy。
- 优先选择 `strategies/approved` 中 domain 匹配、allowed atomics 覆盖问题的策略。
- 若必须生成 draft strategy，先写明目的、适用范围、非目标、allowed atomics、阶段逻辑、期望证据、风险和报告要求，然后请求用户审核。
- Draft strategy 默认只用于当前 run；不要自动晋升为 approved。
- 后续阶段是否执行取决于前序 evidence，不按固定清单机械跑完整策略。

## Replay 要求

- Replay YAML 用于在其他 trace 上复现同类问题判定，只记录确定性步骤。
- 当前 CLI 的 replay schema 必须使用字符串 `problem_signature` 和 `source_strategy`，不得把 `problem_signature` 写成对象。所有 `params` 值写成字符串，避免 YAML 数字类型与 CLI 的字符串参数模型不匹配。

```yaml
problem_signature: cold_start_sched_latency_v1
source_strategy: cold-start-scheduler-topdown
steps:
  - atomic: trace_sanity_check
    params: {}
  - atomic: sched_latency_overview
    params:
      process_name: "SettingsAbility"
      start_ts: "244822650000"
      end_ts: "256372458000"
```

- 对其他 trace 使用 replay/signature 时，先重新 capture 目标进程和时间窗口，再执行依赖窗口的步骤。
- 不把某次 trace 的固定 `start_ts/end_ts` 原封不动套到其他 trace 上。
- 若当前 CLI 只支持重放执行而不支持 assertions 判定，明确说明限制，并把判定依据写入最终报告。

## 证据与报告

- 每个结论必须引用 atomic 输出字段或 artifact 路径。
- 面向用户或评审阅读的 Markdown 产物必须使用中文，包括 `summary.md`、`topdown-brief.md`、`strategy-selection.md`、`deep-analysis-summary.md`、`final-report.md` 和 handoff/round report 中的人工总结；命令日志、JSON、CSV、YAML 字段名可以保留原始英文。
- CPU contention 结论必须区分窗口运行负载证据/竞争者证据与目标进程受影响的因果证据，不得把存在窗口负载直接写成目标根因。
- 若 `sched_latency_overview` 返回的字段不能证明线程身份，不得把返回行直接表述为“目标主线程”；应写成“`sched_latency_overview` 返回线程”或“目标进程相关线程”，并说明线程归属限制。
- 最终报告必须先按 `references/report-format.md` 和 `references/e2e-observability.md` 自检，覆盖中文摘要、目标进程可复核选择、负证据、Validation notes、性能可观测性证据和具体下一步分支。
- Validation notes 必须列出 `final_report validate`、`completed validate`、`final_report advance` 的 command-output stdout/stderr artifact 具体路径，或等价命令记录 stdout/stderr artifact 具体路径，并同步写明对应 command id/序号或简短命令文本（如 `034 validate-final_report`、`035 advance-final_report-to-completed`、`036 validate-completed`）；stderr 为空时也必须注明 `stderr artifact exists and is empty` 或 `stderr artifact 存在且为空`。
- 中文最终报告中空 stderr 优先写为 `stderr artifact 存在且为空`；header-only atomic 输出写为“仅表头、无数据行”，避免误读为命令未返回。
- 报告中区分事实、推断和不确定性。
- Trace 缺少 `process.name`、`process.start_ts` 等字段时，说明回填方法和风险。
- 批量分析多份 trace 时，控制 `--jobs`，避免在 16G 内存机器上同时加载过多 trace。
