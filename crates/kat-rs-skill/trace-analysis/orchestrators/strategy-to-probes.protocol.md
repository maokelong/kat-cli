# Strategy To Probes Protocol

本协议指导 LLM 将专家 strategy 转换为本次 run 的 CLI registry probe 调用计划和 checklist。

## 输入

LLM 必须同时读取：

- User Question
- Selected Strategy
- Probe Registry: `crates/kat-rs-cli/trace-registry/`
- Trace Context，若已有
- Current Analysis State，若已有
- Checklist Template: `orchestrators/checklist.template.md`

## 输出

初始化分析时必须生成到调用者当前工作目录：

- `.kat/runs/<run_id>/question.md`
- `.kat/runs/<run_id>/analysis_state.json`
- `.kat/runs/<run_id>/checklist.md`
- `.kat/runs/<run_id>/evidence.jsonl`

按需生成：

- `.kat/runs/<run_id>/report.draft.md`
- `.kat/runs/<run_id>/probe_requirements.md`

`probe_requirements.md` 只记录 registry 缺失能力的最小需求和代码变更建议，不记录每次运行生成的 probe 代码。

## 转换流程

1. 从 strategy 中提取目标、输入、核心经验、判断规则和终止条件。
2. 生成 strategy digest，明确分析目标、根对象、证据需求、控制模型、状态对象和终止范围。
3. 将 strategy 拆成 evidence needs，每个 evidence need 必须能由 probe 证明或反证。
4. 对每个 evidence need，优先选择 `crates/kat-rs-cli/trace-registry/` 中已有 CLI registry probe。
5. 若现有 probe 不足，输出最小 probe 需求和 CLI registry 变更建议；后续代码变更应进入 `crates/kat-rs-cli/trace-registry/` 以及必要的 operators。
6. 根据 strategy digest、evidence needs 和已选 probe 生成 checklist。
7. checklist 只表达 LLM 可见的简单编排：调用哪个 probe、输入来自哪里、完成条件是什么、evidence 如何引用、LLM 在哪个点做选择。
8. 复杂分支、递归栈、候选排序、环检测、覆盖率和复杂数据结构必须写入 `.kat/runs/<run_id>/analysis_state.json`，不得塞进 checklist。
9. 执行后若需要继续探索，LLM 必须基于新 evidence 追加派生 checklist step，或回到全局决策点选择下一步。

## Strategy Digest 生成规则

Strategy digest 是 strategy 到 checklist 的中间表示。它不必单独落盘，但 checklist 生成前必须先在推理中形成，必要时写入 checklist 的“策略摘要”区。

必须包含：

- `intent`: 本次分析要回答的问题，例如“目标线程为什么没有继续推进”。
- `root_object`: 根进程、根线程、根事件或根时间窗口的解析方式。
- `required_inputs`: trace、时间窗口、目标对象、阈值和策略默认参数。
- `evidence_needs`: 可由 probe 产出的事实需求列表。
- `control_model`: `sequence`、`decision` 或 `frontier` 等外层控制结构。
- `state_model`: 需要写入 `.kat/runs/<run_id>/analysis_state.json` 的跨轮状态。
- `decision_points`: LLM 必须基于 evidence 做选择的位置。
- `boundary_conditions`: 局部边界条件及其作用范围。
- `global_exit`: 可以结束全局分析的条件。
- `report_contract`: 最终报告必须包含的事实、推断和不确定性。

终止条件必须先分层：

- `candidate_boundary`: 只终止当前候选或当前边，例如 `max_depth`、环、特殊线程、单条等待链不闭合、当前窗口无状态片段。
- `global_exit`: 结束整个分析，例如用户问题已可回答、覆盖率达到策略要求、候选池耗尽、关键输入缺失导致无法继续。

禁止把 `candidate_boundary` 直接写成全局 `stop_reason`。

## Evidence Need 拆解规则

Evidence need 是一句可以由 probe 证明或反证的事实需求。

示例：

- “目标进程是否存在 firstDrawFrame marker。”
- “某线程在窗口内有哪些 thread_state 片段。”
- “当前等待片段是否存在 waker。”
- “当前 critical path depth 是否触发局部边界条件。”

Evidence need 不应写成最终诊断。

禁止：

- “找出根因。”
- “判断是否 IO 卡顿。”
- “证明 worker 是关键路径。”

## 专家策略到 Checklist 的映射规则

生成 checklist 时，按以下映射执行：

- strategy 的目标和报告要求，映射到 checklist 顶部策略摘要和最终 `synthesize` step。
- strategy 的输入，映射到初始化 step，例如 trace 检查、根对象解析、时间窗口确认。
- strategy 的核心经验，先拆成 evidence needs，再映射到 `call-probe` step。
- strategy 的专家判断规则，映射到 `llm-review` step 的 `Branch Rules`，不得写成无 evidence 的结论。
- strategy 的循环或递归经验，映射成一个清晰的 LLM 决策点，以及 `.kat/runs/<run_id>/analysis_state.json` 中的状态模型。
- strategy 的终止条件，先区分 `candidate_boundary` 和 `global_exit`，再分别写入局部边界规则或全局退出规则。
- strategy 的不确定性要求，映射到每个相关 step 的 `Branch Rules` 和最终报告规则。

控制流生成规则：

- 顺序执行：使用连续顶层 Step，按序推进。
- 浅层选择：使用 `llm-review` step 显式选择一个方向，并按需追加后续 step。
- 后续探索：使用 `append-step` 基于 evidence 派生新 step。
- 候选池探索：不得生成无限嵌套 step；应把 frontier、visited、coverage、selected item 写入 `.kat/runs/<run_id>/analysis_state.json`。
- 全局退出：只在 `llm-review` 或 `synthesize` 前由 LLM 基于 evidence 和 state 判断。

## Probe 选择规则

优先级：

1. 使用已验证 CLI registry probe。
2. 使用已有 probe 的参数化组合。
3. 若缺能力，输出最小 registry 变更需求。

只有当以下条件成立时，才建议新增 registry probe：

- 现有 probe 无法表达该 evidence need。
- 该逻辑属于确定性查询或局部派生。
- 该 probe 可以通过输入/输出 schema 描述。
- 该 probe 能以只读方式执行。

## Registry 变更需求

缺少 probe 能力时，LLM 只能记录最小需求，不在 run 中生成执行脚本。

最小需求必须包含：

- 建议 probe id。
- 事实需求和适用场景。
- 输入字段、输出 evidence schema 和 status 语义。
- 需要读取的表或 trace 能力。
- 安全约束和最大输出规模。
- 建议 operator 行为。
- 需要补充的 fixture 或测试契约。

相关代码变更应进入 `crates/kat-rs-cli/trace-registry/` 以及必要的 Rust operators；进入代码变更前必须有 issue/SDD 或 PR 说明现成方案为何不适用。

## Checklist 生成规则

Checklist 必须基于 `checklist.template.md`。

每个可执行 step 必须包含：

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

允许的 step 类型只有：

- `call-probe`: 调用一个 CLI registry probe。
- `llm-review`: LLM 基于 evidence 和 state 做判断。
- `append-step`: 基于 evidence 派生后续 step。
- `synthesize`: 输出报告。

Checklist 推荐包含“策略摘要”区，用于提醒后续 LLM：

- 本次分析目标。
- 根对象和时间窗口。
- 主证据链。
- 需要维护的 `.kat/runs/<run_id>/analysis_state.json` 字段。
- 候选边界和全局退出条件的区别。

Checklist 不允许表达：

- 完整递归算法。
- 多层嵌套循环体。
- 大段伪代码。
- 无 evidence 的根因判断。

Checklist 生成后必须自检：

- 每条 strategy 核心经验是否至少对应一个 evidence need 或 LLM 决策点。
- 每个 evidence need 是否有 registry probe 来源，或明确记录了最小 registry 变更需求。
- 每个 `call-probe` step 是否只调用一个 probe。
- 每个 strategy 终止条件是否标明了局部候选边界或全局退出。
- 是否存在没有 evidence 支撑的事实或根因表述。

## Checklist 派生规则

执行过程中，如果现有 checklist 不足以继续，LLM 可以追加派生 step。

派生 step 必须说明：

- 来源 step。
- 使用了哪些 evidence。
- 为什么需要追加。
- 下一步调用哪个 registry probe。
- 新 probe 输入从哪里来。
- 何时停止继续派生。

示例：

```md
## Step 4.1. 扩展下一层关键路径

Derived From: `Step 4`
Evidence Used: `ev.critical_path.thread_state_profile.001`
Reason: `new_candidate_edges` 包含新的 waker，且当前候选未触发局部边界。
Type: call-probe
Probe: `critical_path.thread_state_profile`
Inputs:
- `itid`: 672
- `depth`: 1
- `visited_edges`: from `.kat/runs/<run_id>/analysis_state.json`
Done When:
- evidence 包含 `edge_boundary_hints`、`selected_edge_update` 或 `new_candidate_edges`
Evidence Ref: `ev.critical_path.thread_state_profile.002`
```

## Analysis State 更新规则

LLM 每轮执行后必须更新 `.kat/runs/<run_id>/analysis_state.json`：

- 当前 strategy。
- 已选择的 registry probes。
- 当前 hypotheses。
- 已探索对象。
- 已访问边或候选集合，若适用。
- 当前候选的 `status`、`terminal_reason` 或 `explanation_kind`，若适用。
- next step candidates。
- global stop reason，只有当问题已可回答、候选池耗尽或关键输入缺失导致无法继续时才写入。

复杂状态不写入 checklist，写入 `.kat/runs/<run_id>/analysis_state.json`。

## Probe 调用规则

每个 `call-probe` step 必须通过 CLI 调用：

```sh
kat-rs probe run --probe <probe_id> --source sqlite --file <trace_or_db_path> --params-file .kat/runs/<run_id>/<probe>.params.json --run-dir .kat/runs/<run_id>
```

trace/db 输入通过 `--file <trace_or_db_path>` 传入；probe 参数先写入 `.kat/runs/<run_id>/<probe>.params.json`，再通过 `--params-file` 传入。

CLI 输出的 evidence 必须追加到 `.kat/runs/<run_id>/evidence.jsonl`，并由 LLM 在下一步读取和解释。

## 报告规则

最终报告必须分为：

- 事实：只能引用 evidence。
- 推断：说明由哪些 facts 支撑。
- 不确定性：说明缺失表、缺失 marker、缺失 waker 或 probe limitations。

禁止将 probe 的局部判断直接写成最终根因。
