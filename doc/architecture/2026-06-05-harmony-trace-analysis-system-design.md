# Harmony Trace Analysis System Design - Compact AI Development Spec

状态：精简开发规格

日期：2026-06-05

来源：`doc/architecture/2026-06-05-harmony-trace-analysis-system-design.md`

用途：

- 作为 AI / 开发者实现鸿蒙 trace 分析系统时的主输入。
- 保留原设计中的架构边界、Schema/API 契约、证据规则、流程、测试与验收约束。
- 删除讨论过程、重复解释和长示例；原文作为展开说明和样例来源。

如果本文件与原讨论稿冲突：

1. 本文件中的“硬约束 / 禁止 / 必须”优先作为开发约束。
2. 原文中的示例、字段展开、领域样例可作为参考，但不得推翻本文件的边界。
3. 未明确的业务规则、SLA、领域阈值必须标为 TBD，不得由 AI 猜测。

## 1. 系统目标与范围

系统目标：

```text
让 LLM / Skill 具备可控的 topdown trace 分析能力。
把确定性查询、配置校验、证据落盘、摘要、受限读取、证据审计交给 Rust htrace-service。
用 workspace files 持久化分析流程状态。
用 evidence / artifact / receipt 保证结论可追溯。
```

系统不是冷启动专用系统。冷启动只作为 reference domain，用来说明配置、transform/composite、playbook 和验收样例。同一框架必须支持：

```text
丢帧
内存
runnable latency
binder blocking
IO
lock
storage
其他 HarmonyOS trace 问题
```

交付分期：

```text
Phase 1:
  Checklist-orchestrated 单 trace 可控分析闭环。

Phase 2:
  Playbook 生成、发布、batch workspace、批量聚合与 review workspace。
  Phase 2 启用前必须补齐容量、并发、存储、保留、失败隔离和成本边界。
```

## 2. 不可违反的硬约束

### 2.1 总边界

```text
单 Rust htrace-service。
htrace-service 是无 workflow 状态的 evidence/capability service。
Skill / LLM 负责专家语义判断和 Checklist 编排。
workspace files 负责分析流程状态持久化。
htrace-service 负责确定性执行、校验、证据索引、摘要、受限读取。
htrace-parser 只负责查询。
```

禁止：

```text
禁止 LLM 直接调用 htrace-parser。
禁止 LLM 拼 SQL。
禁止 LLM 直接写 evidence/atomics 或 evidence/transformed。
禁止 LLM 绕过 htrace-service 伪造 evidence。
禁止把临时上下文当作唯一运行状态。
禁止 execution 判断根因。
禁止 service 维护 workflow 状态、queue、approval、RunStateMachine 或 ItemStateMachine。
禁止 service 接收 next checklist steps 或自动推进 workflow。
禁止 service 把自然语言 when 条件变成复杂规则引擎。
禁止 Checklist status 被当成 trace 事实来源。
禁止 report 引用未落盘的临时推理。
```

### 2.2 状态与事实边界

```text
Checklist 是 workflow 状态，不是 trace 事实。
analysis-context.json 是恢复上下文，不是 trace 事实。
evidence-map.json 是引用索引，不是 trace 事实。
ExecutionReceipt / EvidenceReceipt / ArtifactReceipt 是审计，不是 trace 事实。
AtomicEvidence 是一手机器事实。
TransformedEvidence 是确定性转换事实。
Summary 是压缩视图，不是唯一事实来源。
Artifact 是解释性产物，必须引用 evidence。
```

### 2.3 root cause / hypothesis gate

root cause 相关护栏统一理解为：

```text
Evidence Sufficiency Gate / Required Evidence Gate
```

该 gate 只校验：

```text
证据引用存在。
证据属于当前 analysis workspace。
证据类型满足最低要求。
证据完整性与截断影响已说明。
高置信结论不能 summary-only。
problem-specific required evidence gate 满足。
counter_evidence_refs 已记录；没有时也必须显式说明。
报告保留 Facts / Inferences / Uncertainties。
```

该 gate 不做专家语义证明，不声称“根因一定正确”。gate 通过只能写成：

```text
证据合同满足。
```

不能写成：

```text
service 已证明因果关系。
```

## 3. 架构与模块职责

逻辑层：

```text
User / Web UI / CLI / Codex Skill
        |
        v
Analysis Workspace Files
        |
        v
htrace-service
        |
        v
htrace-parser
        |
        v
Trace Dataset
```

htrace-service 是模块化单体：

```text
htrace-service
  api
  trace
  capability
  execution
  evidence_store
  artifact
  playbook
  config
```

模块职责：

| 模块           | 负责                                                                                 | 不负责                                                                |
| -------------- | ------------------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| api            | HTTP / CLI 入口、request_id、error envelope、routing                                 | 业务逻辑、直接写库、直接调用 htrace-parser                            |
| trace          | dataset 绑定、schema inspect、htrace-parser adapter、trace metadata                  | 分析结论、根因判断、workflow queue                                    |
| capability     | ConfigRegistry、CapabilityRegistry、profile allowlist、guard、引用校验               | trace 查询、专家判断、下一步分析                                      |
| execution      | atomic query、atomic.postprocess、composite recipe、transform step、ExecutionReceipt | Checklist、报告、根因、workflow 状态、独立 transform API              |
| evidence_store | evidence 文件、SQLite index、summary、bounded read、hash、receipt、reconcile         | 无限读 raw evidence、替 LLM 写结论、保存 workflow 状态机              |
| artifact       | report / summary / revision 等 artifact 的登记、版本、引用校验                       | 保证报告语义正确、绕过 evidence refs                                  |
| playbook       | Playbook 校验、发布、单 trace/chunk evaluation、聚合能力                             | 固化单 trace 结论、维护 batch lifecycle、让 LLM 对每条 trace 自由判断 |
| config         | 配置文件、原子能力                                                                   | evidence 事实                                                         |

execution 内部能力类型：

| 类型             | 职责                                                            | 禁止                                                            |
| ---------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| atomic query     | 从 htrace-parser 查询低级证据；SQL 只能在 atomic.query.template | 跨步骤推理、根因判断、自由 SQL 拼接                             |
| transform step   | 基于已有 evidence 做确定性选择、归一化、聚合、分类、阶段切分    | 独立 capability、独立外部 API、专家判断、报告、绕过 source_refs |
| composite recipe | 固定 recipe 串联 atomic / transform / quality_gate / evaluate   | 等待 LLM、自由规划、长期状态                                    |

定位公式：

```text
atomic 负责取数，也可以通过 postprocess 做轻量结构化。
composite 负责把多个 atomic 和 transform steps 固化成确定性 recipe。
transform step 负责把数据变成可复用、可测试、可批量复现的机器事实。
summary 负责压缩展示。
LLM 负责解释事实。
```

新增普通分析能力时，优先新增或修改 atomic/composite YAML。只有新增通用 transform operator 时才改 Rust。

4+1 架构视图已独立维护在：`doc/architecture/2026-06-05-harmony-trace-analysis-system-4plus1-view.md`。

## 4. Analysis Workspace 与 Checklist 协议

### 4.1 Workspace 文件结构

默认目录：

```text
analysis-runs/<analysis_id>/
```

必须支持：

```text
Checklist.md
analysis-context.json
evidence-map.json
inputs.json
parser.json
evidence/atomics/*.json
evidence/transformed/*.json
evidence/summaries/*.summary.json
artifacts/topdown/*.md
artifacts/deep/*.md
artifacts/reports/*.md
artifacts/revisions/*.md
receipts/executions/*.json
receipts/evidence/*.json
receipts/artifacts/*.json
receipts/guards/*.json
logs/parser/
logs/service/
```

创建 workspace 时必须写入：

```text
Checklist.md
analysis-context.json
evidence-map.json
inputs.json
parser.json
```

恢复 workspace 时必须先读：

```text
analysis-context.json
Checklist.md
evidence-map.json
```

禁止仅凭 LLM 上下文继续分析。

### 4.2 Skill 主循环

```text
collect_user_intent
  -> route_domain_or_problem
  -> create_or_resume_workspace
  -> ensure_dataset_ref
  -> generate_or_load_Checklist.md
  -> read analysis-context.json
  -> read evidence-map.json
  -> select_next_checklist_step
  -> execute_step_via_htrace_service
  -> persist_receipt_and_refs
  -> update_Checklist.md / evidence-map.json
  -> read_summary_or_bounded_evidence
  -> decide_continue_report_block_or_redirect
```

### 4.3 Checklist 状态模型

状态：

```text
todo
in_progress
done
blocked
skipped
revisit
```

允许转换：

```text
todo -> in_progress -> done
todo -> in_progress -> blocked
todo -> skipped
done -> revisit
revisit -> in_progress -> done
blocked -> todo
blocked -> skipped
blocked -> done
skipped -> todo 仅允许用户改道或显式重开
```

状态要求：

```text
done:
  必须记录 evidence/artifact/receipt refs，或明确 no_output_reason。

blocked:
  必须记录 blocker、actor_required、cannot_conclude、recommended_action。

skipped:
  必须记录 reason。

revisit:
  说明为什么需要复查，例如 summary 截断、confidence 低、出现 counter evidence。
```

Markdown 表达：

```markdown
- [ ] S3 phase breakdown
- [~] S4 critical path candidates
- [x] S1 select target process
- [!] S2 startup anchors blocked: touchEventDispatch missing
- [-] CPU contention deep dive skipped by user redirect
- [?] S5 needs revisit after bounded read
```

### 4.4 每步持久化顺序

每次执行 step 必须按固定顺序：

```text
1. 将 Checklist step 标为 in_progress。
2. 调用 htrace-service。
3. 保存 ExecutionReceipt / EvidenceReceipt / ArtifactReceipt。
4. 更新 evidence-map.json 中 step -> refs 映射。
5. 根据 service response 将 step 标为 done / blocked / revisit / skipped。
6. 写入 uncertainties、cannot_conclude、recommended_action 或 next actions。
7. 若用户改道，追加 artifacts/revisions/plan-revisions.md。
```

无效状态：

```text
service 调用成功但 Checklist 不更新，是 Skill 协议错误。
Checklist 标 done 但没有 evidence/receipt refs 或 no_output_reason，是无效 step。
status=blocked 缺 blocker、actor_required、recommended_action，是无效 step。
retry / force rerun 必须在 Checklist 记录原因，并产生新的 receipt。
```

### 4.5 Checklist Step Schema

Checklist 是 Markdown，但每个 step 必须包含结构化字段，便于恢复和工具校验：

```yaml
status: done | blocked | skipped | revisit | in_progress | todo
owner: skill
capability: atomic/<id> | composite/<id> | artifact/<kind> | guard/<id> | user_gate
inputs: {}
outputs:
  evidence: []
  transformed: []
  summaries: []
  artifacts: []
  receipts: []
confidence: {}
uncertainties: []
cannot_conclude: []
next_actions: []
```

强约束：

```text
报告结论不得引用 Checklist status 作为 trace 事实。
Checklist step capability 必须能被 service config 解析，user_gate 除外。
inputs 只能来自 analysis-context、用户输入、已有 evidence/transformed refs 或配置模板。
outputs 只能记录当前 analysis workspace 内的相对 ref。
```

### 4.6 service response 到 Checklist 状态

```text
ok:
  step -> done

partial:
  step -> done 或 revisit，必须记录 uncertainties。

blocked:
  step -> blocked，必须记录 blocker、cannot_conclude、actor_required、recommended_action、receipt_ref。

error:
  step -> blocked，actor_required 通常为 developer / htrace_parser_operator / config_owner。
```

### 4.7 Evidence 读取与上下文卫生

默认上下文只放少量索引信息：

```json
{
  "analysis_id": "...",
  "current_step": "S4",
  "checklist_status": "in_progress",
  "summary_refs": [],
  "artifact_refs": [],
  "blockers": []
}
```

读取规则：

```text
优先读取 summary。
summary 不足时使用 bounded read。
bounded read 必须指定 fields 和 limit_rows。
不得把完整 evidence JSON 放入上下文。
不得把 SQL、HTTP body、htrace-parser stderr 当作分析事实。
读取行为必须产生 read_receipt，并写入 Checklist / evidence-map。
```

## 5. 配置文件契约

### 5.1 配置原则

```text
topdown 文件表达分析路径，不表达 SQL。
atomic 文件表达可执行原子能力，是唯一允许出现 SQL 的配置。
atomic 可带 postprocess。
transform step 表达确定性数据转换，只能写在 atomic.postprocess 或 composite.steps 中。
composite 文件表达确定性 recipe。
strategy 文件表达 Skill 可执行的 Checklist 模板和分支口径。
guard 文件表达 service 可执行护栏，不表达复杂专家系统。
knowledge 文件表达 LLM 专家知识，不被 service 当作规则执行。
template 文件表达 artifact 格式，不表达分析逻辑。
```

### 5.2 配置目录

```text
config/service.yaml
config/users.yaml
config/domains.yaml
config/profiles/*.yaml
config/topdown/domain/*.yaml
config/topdown/problem/*.yaml
config/atomics/**/*.yaml
config/transforms/operators.yaml
config/composites/**/*.yaml
config/strategies/**/*.yaml
config/guards/*.yaml
config/templates/*.md
config/knowledge/**/*.md
config/playbook-templates/*.yaml
```

所有 YAML 通用字段：

```yaml
id: string
kind: string
version: string
description: string
```

通用校验：

```text
id 在同类配置中唯一。
kind 与目录和 schema 匹配。
version 必填。
所有引用必须存在。
所有文件必须参与 config_snapshot_hash。
```

### 5.3 关键配置职责

| 配置                      | 负责                                                                                                 | 禁止 / 约束                                                                                                      |
| ------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| service.yaml              | server、storage、parser_defaults、execution limits、evidence_read、security                          | 不声明 workflow execution_mode；analysis_root/evidence_root 必须受控；allow_admin_api=false 时禁用 admin execute |
| users.yaml                | 辅助 Skill 选择默认领域                                                                              | 不参与 trace 查询或 root cause 判断                                                                              |
| domains.yaml              | 分析领域、默认 profile、domain_topdown                                                               | domain id 唯一；default_domain 存在                                                                              |
| profiles/*.yaml           | allowed_atomics、allowed_transform_operators、allowed_composites、knowledge、guards、resource_policy | 不写 SQL；不写专家判断；不直接声明 next steps；资源策略不能超过 service 全局上限                                 |
| topdown/domain/*.yaml     | 领域级 topdown，只引用 problem_topdown                                                               | 不写 SQL；不直接执行 atomic；不直接输出 root cause                                                               |
| topdown/problem/*.yaml    | 具体问题分析路径，供 Skill 生成 Checklist                                                            | 不写 SQL；不写复杂 when DSL；不直接判定 root cause；不直接读 raw evidence                                        |
| atomics/**/*.yaml         | 原子查询能力，唯一 SQL 入口                                                                          | 不引用 topdown；不写 root cause；不写 LLM guidance；不访问 workflow state                                        |
| transforms/operators.yaml | Rust 内置通用 transform operator 注册表和 schema                                                     | 不是能力目录；普通能力新增不改 Rust                                                                              |
| composites/**/*.yaml      | 确定性 recipe，内部可含 atomic、transform、quality_gate、evaluate                                    | 不写 SQL；不调用 LLM；不维护 workflow；不生成 next steps；不执行任意脚本                                         |
| guards/*.yaml             | 结构、引用、权限、证据门槛校验                                                                       | 不写复杂专家语义；不读取 raw rows；失败返回 blocked，不直接 Failed                                               |
| templates/*.md            | artifact 结构与必填章节                                                                              | 不包含 SQL；不包含固定 root cause 结论                                                                           |
| knowledge/**/*.md         | LLM 专家知识、误判提醒、报告口径                                                                     | 不被 service 解析为规则；不要求 LLM 查 SQL 或调用 htrace-parser                                                  |
| playbook-templates/*.yaml | 可选的内置 Playbook 模板、schema seed 或示例检测器                                                   | 不保存已发布 PlaybookVersion；不得作为 batch lifecycle 状态；evaluator rules 必须结构化                          |

发布后的 `PlaybookVersion` 是 workspace 产物，必须固化在 `playbooks/<playbook_id>/<version>/` 或等价 workspace 路径中，不属于 Config Files。Config Files 只提供基础能力、护栏、资源边界和可选模板；不能把已发布版本、BatchTraceResult 或 review findings 挂在 config 下。

### 5.4 Atomic 契约

atomic 是 execution.atomic 与 htrace-parser 之间的唯一查询契约。

必须包含：

```yaml
id: string
kind: atomic
version: string
domain: string
parser: htrace_parser_query
inputs: {}
resources:
  timeout_ms: int
  max_rows: int
  max_result_bytes: int
query:
  language: sql
  template: string
outputs:
  columns: []
summary:
  kind: string
```

允许 input type：

```text
string
int64
float64
bool
timestamp
duration
string_array
int64_array
```

允许 SQL template filter：

```text
sql_string
sql_like
sql_int
sql_float
sql_bool
sql_int_csv
sql_string_csv
```

atomic 校验：

```text
query.template 中所有变量必须来自 inputs。
所有 filter 必须在白名单。
outputs.columns 非空。
execution.atomic 必须用 outputs.columns 校验 htrace-parser 返回列。
resources 不得超过 profile 和 service 上限。
summary.kind 必须有 summarizer，或使用 generic summarizer。
```

### 5.5 Transform Step 契约

transform step 不作为独立 capability，不暴露独立外部 API，也不单独放能力目录。

只能出现于：

```text
atomic.postprocess
composite.steps[].kind = transform
```

推荐内置 operator：

```text
filter
project
select_one
rank
join
group_by
aggregate
window_aggregate
interval_merge
timeline_segment
threshold_classify
rule_classify
anchor_reconstruct
critical_path_extract
graph_walk
```

transform 校验：

```text
operator 必须在 registry 存在。
operator 必须被 profile.allowed_transform_operators 允许。
inputs 只能引用 inputs、evidence、transformed、analysis 或 self。
rules 必须满足 operator schema。
output.schema 必须能被 JSON schema 表达。
输出 transformed_evidence 必须记录 source_refs。
```

禁止：

```text
transform 写 SQL。
transform 调用 htrace-parser。
transform 写自然语言结论。
transform 判断 root cause。
transform 读取 workflow state。
transform 实现任意脚本执行。
```

### 5.6 Composite 契约

允许 steps kind：

```text
atomic
transform
quality_gate
evaluate
```

composite 校验：

```text
id 唯一。
inputs 完整声明类型。
steps[].id 在 composite 内唯一。
steps[].depends_on 只能引用前置 step。
atomic step 引用的 atomic 存在，并被 profile.allowed_atomics 允许。
transform step 引用的 operator 存在，并被 profile.allowed_transform_operators 允许。
transform inputs / rules 满足 operator schema。
quality_gate.rule.type 在白名单。
outputs 只能引用本 composite 产生的 output_key。
composite execution receipt 必须包含每个 child step 的 receipt_ref。
```

### 5.7 Guard 契约

允许 guard rule type：

```text
require_evidence_kind
require_current_analysis_evidence_refs
require_min_evidence_refs
require_profile_allowlist
require_param_refs_resolved
forbid_summary_only_root_cause
forbid_cross_analysis_evidence
```

guard 只能校验证据形态、引用、权限、最低门槛。不能判断 CPU contention、binder blocking 等专家语义是否成立。

### 5.8 模板引用

允许引用域：

```text
{{ inputs.xxx }}
{{ evidence.output_key }}
{{ transformed.output_key.field }}
{{ analysis.xxx }}
{{ batch.xxx }}
```

引用规则：

```text
引用域必须在白名单。
引用目标必须存在或可由 depends_on 产生。
类型必须与目标 atomic input schema 或 transform operator schema 匹配。
未解析引用导致 blocked，而不是静默传空。
```

禁止：

```text
模板引用任意文件路径。
模板引用环境变量。
模板执行代码。
模板读取 raw evidence rows。
```

### 5.9 ConfigRegistry 流程

service 启动时：

```text
1. 扫描 config root。
2. 按 kind 解析配置。
3. 校验基础 schema。
4. 建立 id registry。
5. 校验跨文件引用。
6. 校验 profile allowlist。
7. 校验 atomic SQL template filter。
8. 校验 transform operator 注册和 operator schema。
9. 校验 composite steps、depends_on、outputs 和 child capability allowlist。
10. 校验 guard rule type。
11. 校验 template 变量。
12. 生成 config_snapshot_hash。
```

analysis workspace 创建时：

```text
绑定当前 config_snapshot_hash。
解析 initial Checklist template。
校验 Checklist template 对应 profile / topdown / inputs。
analysis 执行期间固定使用该 config snapshot。
```

配置热更新：

```text
只影响新 analysis。
不得静默改变已有 analysis。
迁移已有 analysis 必须显式创建 config migration record。
```

## 6. API 契约

### 6.1 API 原则

```text
htrace-service 不提供 workflow 状态推进 API。
htrace-service 不接收 LLM next checklist steps。
htrace-service 不维护 queue / approval / workflow state。
Phase 1 外部主入口是 dataset、atomic、composite、evidence、artifact、guard、config。
Phase 2 额外开放 playbook、cohort、batch preflight、batch aggregate、review sample prepare 等能力型 API。
Skill 根据 Checklist 决定调用顺序。
```

所有写接口建议支持：

```text
Idempotency-Key
```

统一响应 envelope：

```json
{
  "status": "ok | blocked | partial | error",
  "data": {},
  "blockers": [],
  "error": null,
  "receipt_ref": null
}
```

blocked response 必须面向 Skill 可恢复：

```json
{
  "code": "PARSER_SCHEMA_MISMATCH",
  "message": "...",
  "actor_required": "developer | user | htrace_parser_operator | config_owner | skill",
  "cannot_conclude": "cannot use this result as trace evidence",
  "data_retained": true,
  "recommended_action": "...",
  "refs": {
    "receipt_ref": "receipts/executions/sched-latency.json"
  },
  "diagnostics": {}
}
```

错误分层：

```text
blocked:
  业务可恢复阻塞。

partial:
  部分成功或降级。

error:
  系统不可恢复错误。
```

### 6.2 外部 API

System:

```text
GET /health
GET /version
```

Dataset / Trace:

```text
POST /datasets/register
POST /datasets/upload
GET  /datasets
GET  /datasets/{dataset_ref}
GET  /datasets/{dataset_ref}/inspect
POST /datasets/{dataset_ref}/capability-probe
```

Atomic:

```text
POST /atomics/{atomic_id}/execute
```

请求关键字段：

```json
{
  "analysis_id": "...",
  "dataset_ref": "datasets/...",
  "profile": "...",
  "params": {},
  "output_key": "...",
  "idempotency_key": "..."
}
```

执行流程：

```text
guard 校验 atomic/profile/params/dataset capability
  -> execution.atomic 渲染 SQL
  -> htrace-parser query
  -> output schema 校验
  -> evidence 落盘
  -> summary 生成
  -> evidence_index 注册
  -> 返回 ExecutionReceipt
```

Transform:

```text
没有独立外部 API。
只能通过 atomic.postprocess 或 composite.steps 执行。
```

Composite:

```text
POST /composites/{composite_id}/execute
```

Composite 可以返回：

```text
status: ok | blocked | partial
receipt_ref
evidence_refs
transformed_refs
summary_refs
uncertainties
quality_gate results
```

Composite 不返回 next_recommended_steps；下一步由 Skill 决定。

Evidence:

```text
GET  /evidence
GET  /evidence/{evidence_ref}
GET  /evidence/{evidence_ref}/summary
POST /evidence/{evidence_ref}/read
POST /evidence/validate-refs
```

`read` 必须 bounded：

```json
{
  "limit_rows": 20,
  "fields": ["upid", "pid", "process_name", "confidence"]
}
```

Artifact:

```text
GET  /artifacts/{artifact_ref}
POST /artifacts
POST /artifacts/validate
```

service 负责：

```text
校验 evidence_refs 存在。
校验 evidence 属于当前 analysis workspace。
计算 artifact content_hash。
注册 artifact_index。
返回 ArtifactReceipt。
```

Guard / Validation:

```text
POST /guards/evidence-sufficiency/validate
POST /guards/hypothesis-verdict/validate
POST /guards/artifact/validate
POST /guards/request/validate
```

Config:

```text
GET  /config
GET  /config/domains
GET  /config/profiles/{profile_id}
GET  /config/topdown/problem/{problem_id}
GET  /config/atomics/{atomic_id}
GET  /config/composites/{composite_id}
GET  /config/strategies/{strategy_id}
GET  /config/transform-operators
POST /config/strategies/resolve
POST /config/validate
```

`POST /config/strategies/resolve` 只解析并校验 `domain_topdown` / `problem_topdown` 对应的 strategy contract，返回可供 Skill 渲染 Checklist 的契约；service 不负责渲染 Checklist，也不推进 workflow。

Debug / Admin:

```text
POST /admin/atomics/{atomic_id}/execute
POST /admin/config/reload
POST /admin/reconcile
```

Admin API 不能绕过 guard、profile allowlist、路径安全和 output schema 校验。

### 6.3 内部调用方向

```text
api -> trace / capability / execution / evidence_store / artifact / playbook
execution.atomic -> trace / operator_engine / evidence_store
execution.composite -> execution.atomic / operator_engine / capability / evidence_store
artifact -> evidence_store
playbook -> execution / capability / evidence_store
trace / evidence_store / artifact / playbook -> storage repository
```

禁止：

```text
execution 修改 Checklist。
htrace-parser 读取 analysis-context。
artifact 直接写 evidence。
capability 直接写 workspace。
service 自动推进下一步分析。
```

## 7. Evidence、Summary、Artifact 与 Receipt

### 7.1 Evidence 类型

Atomic Evidence：

```text
路径：evidence/atomics/*.json
来源：execution.atomic
地位：一手机器事实
用途：可被 transform、report、bounded read 引用
```

必须字段：

```json
{
  "schema_version": 1,
  "kind": "atomic_evidence",
  "analysis_id": "...",
  "atomic_id": "...",
  "params_hash": "sha256:...",
  "columns": [],
  "rows": [],
  "truncated": false,
  "generated_at": "..."
}
```

Transformed Evidence：

```text
路径：evidence/transformed/*.json
来源：atomic.postprocess 或 composite.steps transform
地位：二手机器事实
用途：可被后续 atomic 参数、report 引用
必须记录 source_refs
```

必须字段：

```json
{
  "schema_version": 1,
  "kind": "transformed_evidence",
  "transform_operator": "...",
  "transform_id": "...",
  "source_refs": [],
  "data": {},
  "generated_at": "..."
}
```

Summary Evidence：

```text
路径：evidence/summaries/*.summary.json
来源：summarizer
地位：上下文压缩视图
不作为高置信根因的唯一事实来源
关键事实必须追溯到 atomic/transformed evidence
```

summary 必须暴露：

```text
source_ref
row_count
truncated
fields_included
omitted_fields
known_limitations
coverage
anomaly_flags
```

若 summary 截断、遗漏关键字段、coverage 不足或有 anomaly_flags，LLM 必须 bounded read 或提交 uncertainty，不能推进高置信根因。

### 7.2 Evidence Index

SQLite `evidence_index` 至少存：

```text
evidence_ref
analysis_id
kind
producer
producer_id
source_refs
row_count
columns
params_hash
content_hash
truncated
created_at
```

用途：

```text
快速列 evidence。
校验 report 引用。
做 bounded read。
判断 atomic + params_hash 是否可复用。
支持 UI 展示证据树。
```

### 7.3 Bounded Read

规则：

```text
LLM 默认只能读 summary。
bounded read 必须指定 limit_rows。
bounded read 必须指定 fields，除非 evidence 很小。
默认禁止读取 rows 全量。
超限返回 blocked。
读取行为返回 read_receipt。
```

### 7.4 Artifact

artifact 类型：

```text
problem_report
domain_summary
next_plan
deep_analysis_summary
plan_revision
final_report
```

artifact metadata：

```json
{
  "artifact_ref": "...",
  "kind": "...",
  "analysis_id": "...",
  "source_step_id": "...",
  "evidence_refs": [],
  "created_by": "llm",
  "content_hash": "sha256:...",
  "created_at": "..."
}
```

所有 artifact 必须区分：

```text
Facts:
  直接来自 atomic/transformed evidence 的事实。

Inferences:
  LLM 基于 facts 和 knowledge 做的推断。

Uncertainties:
  证据不足、窗口不完整、候选不唯一、需要 deep analysis 的部分。

Next Actions:
  建议进入的 next checklist steps。

Hypothesis Verdicts:
  supported | contradicted | insufficient。
```

报告禁止：

```text
无 evidence ref 的事实陈述。
把 service receipt / Checklist 状态当 trace 事实。
把 htrace-parser stderr 当 trace 事实。
只凭 summary 写强根因。
```

### 7.5 Receipt 与恢复

ExecutionReceipt 必须记录：

```text
receipt_id
analysis_id
kind
capability_id
capability_version
dataset_ref
params_hash
sql_template_hash
rendered_sql_hash
parser_profile_hash
attempt_id
attempt_no
status
evidence_ref
summary_ref
row_count
truncated
duration_ms
created_at
```

说明：

```text
attempt_id / attempt_no 只用于单次执行收据关联。
service 不根据 attempt 构造长期 AttemptRecord 或状态机。
EvidenceReceipt 记录 evidence 落盘、hash、schema、summary、index 注册结果。
ArtifactReceipt 记录 artifact 写入、校验、引用和 hash 结果。
相同 atomic + params_hash + dataset_ref 可复用 evidence，但必须返回复用 receipt。
```

不可变规则：

```text
atomic/transformed evidence 默认不可变。
artifact 可以有版本，但旧版本保留。
plan revision 追加写，不覆盖。
用户改道不删除历史 evidence。
```

## 8. 分析流程

### 8.1 通用入口

```text
User / CLI / Web UI / Skill
  -> Skill 收集用户意图和 trace 输入
  -> Skill 创建 analysis workspace
  -> Skill 调用 htrace-service 注册 dataset
  -> Skill 调用 capability-probe
  -> Skill 选择 strategy / checklist template
  -> 写 analysis-context.json
  -> 写 Checklist.md
  -> 写 evidence-map.json
```

输入缺失时：

```text
Checklist step = blocked
blocker = MissingRequiredInput
```

### 8.2 明确问题流程

适用于用户已明确 cold start、frame jank、memory growth、runnable latency、binder blocking、IO wait 等问题。

流程：

```text
1. Skill 识别 strategy。
2. Skill 创建 analysis workspace。
3. Skill 生成对应 Checklist.md。
4. Skill 执行 S0 dataset inspect / capability probe。
5. Skill 按 Checklist 调用 atomic / composite API。
6. transform step 由 atomic.postprocess 或 composite.steps 执行。
7. service 返回 evidence / summary / receipt。
8. Skill 更新 Checklist 和 evidence-map。
9. Skill 在 decision point 写 problem report 或新增后续 checklist steps。
```

### 8.3 不明确问题 / domain scan

适用于“这个 trace 帮我看看哪里慢”。

流程：

```text
1. Skill 根据用户、请求文本、domains.yaml 选择候选 domain。
2. 领域不确定时，Skill 在 Checklist 创建 user gate。
3. 用户确认 domain 或 broad scan。
4. Skill 生成 domain-scan Checklist。
5. Skill 按 priority 执行多个 shallow composites。
6. 每个 problem scan 产出 problem brief / evidence refs / uncertainty。
7. Skill 汇总 domain-summary 和 next-plan。
8. 用户确认后，Skill 追加 deep analysis steps。
```

service 不判断哪个 domain 是根因，也不自动执行 next-plan。

### 8.4 atomic / composite / transform 执行

```text
Skill 选择 Checklist step
  -> step 标记 in_progress
  -> 调用 service API
  -> service guard 校验
  -> service 执行 atomic / composite
  -> composite 内部按 YAML 执行 transform steps
  -> service 写 evidence / summary / receipt
  -> Skill 保存 receipt ref
  -> Skill 更新 evidence-map
  -> step 标记 done / blocked / revisit
```

service 返回 blocked 时，Skill 不能继续假装完成，必须将 step 标 blocked 并记录 cannot_conclude、actor_required、recommended_action。

service 返回 partial 时，Checklist 不新增 `partial` 状态；Skill 必须根据证据完整性把 step 标为 `done` 或 `revisit`，并记录 uncertainties、truncation / degradation 影响和必要的 next_actions。

### 8.5 deep analysis

来源：

```text
Checklist template
用户改道
LLM 根据 problem report 新增 step
strategy 的 next recommended steps
```

成本高或方向不确定时，Skill 必须创建 user gate step。用户 approve 后再追加或解锁 deep steps。

### 8.6 report 流程

报告必须走 artifact 注册和 guard：

```text
Skill 生成 report draft
  -> 调用 /artifacts/validate 或 /artifacts
  -> service 校验 evidence refs
  -> service 校验 evidence sufficiency gate
  -> service 注册 artifact_index
  -> 返回 ArtifactReceipt
  -> Skill 更新 Checklist / evidence-map
```

证据不足：

```text
Checklist step = blocked 或 revisit
blocker = INSUFFICIENT_EVIDENCE_FOR_CLAIM
Skill 新增补证据 steps 或降低结论置信度。
```

### 8.7 用户改道

用户可以随时改道。处理规则：

```text
写 artifacts/revisions/plan-revisions.md。
旧步骤标记 skipped，保留 reason。
新增新方向 checklist steps。
历史 evidence 不删除。
```

### 8.8 blocked 恢复

Blocked 类型：

```text
MissingInput
ConfigInvalid
DatasetUnavailable
DatasetCapabilityMissing
AtomicExecutionFailed
EvidenceInsufficient
EvidenceGateRejected
UserActionRequired
```

恢复：

```text
MissingInput:
  用户补输入 -> 更新 analysis-context.json -> 重跑 step。

DatasetUnavailable:
  重新 register / upload / bind dataset。

DatasetCapabilityMissing:
  降级 strategy 或 step 标 skipped/blocked。

AtomicExecutionFailed:
  retry 当前 step 或改道。

EvidenceInsufficient:
  新增补充证据 step 或降低结论置信度。

EvidenceGateRejected:
  修改 report / hypothesis verdict 后重新校验。

UserActionRequired:
  用户 approve / redirect / skip。
```

### 8.9 推荐端到端流程

```text
1. 用户请求分析 trace 问题。
2. Skill 识别 domain/problem strategy。
3. Skill 创建 analysis workspace。
4. Skill 调用 dataset register / inspect / capability-probe。
5. Skill 生成 Checklist.md。
6. Skill 执行 configured probe atomics。
7. Skill 执行 deterministic composites；composite 内部执行 transform steps。
8. Skill 必要时执行带 postprocess 的 atomics。
9. summarizer 生成 compact summaries。
10. Skill 读取 summaries，必要时 bounded read。
11. Skill 写 problem report。
12. Skill 在 decision point 检查最新用户意图；若用户改道，进入 redirect 分支。
13. redirect 分支：写 artifacts/revisions/plan-revisions.md，旧方向未执行步骤标 skipped 并记录 reason，新增新方向 checklist steps。
14. redirect 分支完成后，从 select_next_checklist_step 继续执行新 Checklist；历史 evidence、artifact、receipt 保留。
15. 若无改道，Skill 根据报告新增 deep analysis steps。
16. 用户确认后，Skill 调用 deep composite / atomics；若用户选择 skip / redirect，按 Checklist gate 结果处理。
17. Skill 写 deep analysis summary。
18. Skill 再次检查用户改道、counter evidence 或 summary coverage；必要时 revisit / redirect / 补证据。
19. Skill 提交 hypothesis verdict 给 guard 校验。
20. Skill 写 final report。
21. Skill 将 Checklist 标记完成。
22. 可选：从 Checklist + evidence-map 生成 playbook/signature draft。
```

改道处理不是一次性异常分支，而是每个 decision point 都必须支持的循环能力。用户改道不得删除历史 evidence；旧步骤只能通过 skipped/revisit/blocked-with-explanation 表达，新路径必须通过 plan-revisions artifact 和新增 Checklist steps 持久化。

## 9. Playbook 与批量分析

本章属于 Phase 2，不阻塞 Phase 1 单 trace 分析闭环。

核心原则：

```text
Playbook 固化的是分析步骤、特征提取、判定门槛和证据 gate。
不是固化单 trace 的结论。
复杂问题允许 llm_assisted playbook，但 LLM 参与点必须显式声明、证据化、结构化和审计化。
Playbook 不复现开放式专家探索，只复现已固化的问题模式检测流程。
```

### 9.1 对象

```text
Playbook
PlaybookVersion
PlaybookStep
FeatureSchema
Evaluator
TraceVerdict
LLMDecisionStep
LLMDecisionArtifact
LLMDecisionReceipt
Cohort
BatchWorkspace
BatchTraceResult
BatchAggregation
GeneralityJudgement
ReviewWorkspace
```

Playbook 包含：

```text
输入要求
执行步骤
参数绑定方式
证据要求
特征提取方式
单 trace 判定规则
批量聚合策略
输出 schema
适用范围
失效条件
execution_mode
reproducibility_level
```

Playbook 不包含：

```text
未声明的 LLM 临场自由决策。
LLM 中途自由新增 next steps。
单 trace 的强行根因结论。
未结构化自然语言 executable when。
不受控 SQL。
```

### 9.2 Playbook 类型与复现等级

Playbook 分三类：

```text
deterministic:
  不调用 LLM。
  使用固定 atomic / transform / feature / evaluator 逐条判断。
  适合批量统计和严格可复现检测。

llm_assisted:
  只在显式 llm_decision step 调用 LLM。
  LLM 基于固定 evidence bundle、prompt_template、output_schema 和 model_policy 做受控判断。
  适合复杂 trace 中需要专家解释、歧义分类或多证据综合判断的问题模式复现。

review:
  只对 unknown / partial_hit / counter_examples 抽样复查。
  用于发现漏检模式、解释反例、生成 playbook v2 或 segment-specific playbook。
  不直接修改原始 BatchTraceResult 或 TraceVerdict。
```

Playbook 顶层必须声明：

```yaml
execution_mode: deterministic | llm_assisted | review
reproducibility_level: deterministic_exact | llm_output_replay | llm_audit_replay | llm_semantic_replay
```

复现等级：

```text
deterministic_exact:
  不依赖 LLM；相同输入、配置、parser、service 版本应得到相同输出。

llm_output_replay:
  不重新调用 LLM，直接使用已保存的 LLMDecisionArtifact 重放原结果。

llm_audit_replay:
  使用相同 evidence bundle、prompt_hash、model_id、model_params 重新调用 LLM，并与原 LLMDecisionArtifact 对照。
  可用于审计，不承诺 bit-level 一致。

llm_semantic_replay:
  允许模型版本变化，但必须记录模型、prompt、schema 和 verdict 差异。
  只能用于探索性复查或 playbook 演进，不能声称严格复现。
```

判定要求：

```text
deterministic playbook 的 TraceVerdict.basis = deterministic。
llm_assisted playbook 的 TraceVerdict.basis = llm_assisted。
review playbook 不直接产出覆盖原 verdict 的最终结论，只产出 review finding。
批量报告必须分开展示 deterministic hit_rate 与 llm_assisted hit_rate。
```

### 9.3 LLM Decision Step

`llm_decision` 是 llm_assisted playbook 中唯一允许调用 LLM 的 step kind。

允许的 step kind：

```text
deterministic playbook:
  atomic
  composite
  transform
  quality_gate
  feature_extract
  evaluate

llm_assisted playbook:
  atomic
  composite
  transform
  quality_gate
  feature_extract
  llm_decision
  evaluate

review playbook:
  review_sample
  atomic
  transform
  llm_decision
  review_finding
```

示例：

```yaml
steps:
  - id: extract_blocking_chain
    kind: composite
    composite: binder_blocking_chain_reconstruction

  - id: classify_blocking_pattern
    kind: llm_decision
    depends_on:
      - extract_blocking_chain
    input_bundle:
      evidence_refs:
        - evidence/transformed/blocking_chain.json
      summary_refs:
        - evidence/summaries/thread_state.summary.json
      bounded_reads:
        - ref: evidence/atomics/binder_transactions.json
          fields:
            - caller_tid
            - callee_tid
            - wait_ms
            - transaction_code
          limit_rows: 50
    prompt_template: prompts/blocking-pattern-classifier.md
    output_schema: schemas/blocking-pattern-verdict.schema.json
    allowed_labels:
      - binder_server_saturation
      - lock_contention
      - io_backpressure
      - insufficient_evidence
      - contradictory_evidence
    required_citations:
      min_evidence_refs: 2
    model_policy:
      model_id: "configured-by-project"
      temperature: 0
      max_output_tokens: 1200
```

`llm_decision` 输入规则：

```text
input_bundle 只能包含当前 workspace 内已落盘 evidence、summary、artifact 和 bounded read result。
bounded_reads 必须显式声明 ref、fields、limit_rows。
LLM 不得读取完整 raw evidence。
LLM 不得直接调用 htrace-parser。
LLM 不得拼 SQL。
LLM 不得访问未进入 input_bundle 的上下文。
prompt_template 和 output_schema 必须版本化并参与 playbook hash。
model_policy 必须记录 model_id、temperature、max_output_tokens 和可接受的模型族约束。
```

`llm_decision` 输出必须是结构化 artifact：

```json
{
  "kind": "llm_decision_artifact",
  "decision_id": "classify_blocking_pattern",
  "label": "binder_server_saturation",
  "confidence": "medium",
  "supporting_evidence_refs": [
    "evidence/transformed/blocking_chain.json"
  ],
  "counter_evidence_refs": [],
  "uncertainties": [
    "callee thread pool size is inferred, not directly measured"
  ],
  "rationale": "bounded short explanation"
}
```

`LLMDecisionReceipt` 必须记录：

```text
decision_id
analysis_id 或 batch_trace_id
playbook_id
playbook_version
input_bundle_hash
prompt_template_ref
prompt_hash
output_schema_ref
output_schema_hash
model_id
model_params
llm_decision_artifact_ref
supporting_evidence_refs
counter_evidence_refs
created_at
```

guard 必须校验：

```text
output schema 合法。
label 在 allowed_labels 内。
evidence refs 存在且属于当前 analysis / batch trace workspace。
supporting_evidence_refs 满足 required_citations。
高置信 verdict 不允许 summary-only。
confidence 不得超过 evidence gate 允许上限。
prompt_hash、schema_hash、input_bundle_hash 已记录。
LLMDecisionArtifact 不得包含未引用证据的事实陈述。
```

`llm_decision` 不能：

```text
新增任意 next steps。
直接写 final report。
直接覆盖 deterministic feature values。
直接修改 TraceVerdict。
隐藏 uncertainty 或 counter evidence。
```

最终 `TraceVerdict` 可以引用 `llm_decision`：

```json
{
  "verdict": "partial_hit",
  "basis": "llm_assisted",
  "machine_features": {},
  "llm_decision_ref": "artifacts/llm-decisions/classify_blocking_pattern.json",
  "reproducibility_level": "llm_audit_replay"
}
```

### 9.4 从 Analysis Workspace 生成 Playbook

流程：

```text
1. 单 trace Checklist 分析完成。
2. 用户选择固化本次分析步骤。
3. Skill 或 playbook_compiler 读取 Checklist、analysis-context、evidence-map、receipts、evidence_index、artifact_index。
4. 提取已完成步骤。
5. LLM 辅助把 trace-specific 参数泛化成模板。
6. 用户确认 keep / optional / drop / manual_review。
7. 定义 feature schema。
8. 定义 evaluator rules。
9. 如果需要 LLM 参与，定义 llm_decision steps、input_bundle、prompt_template、output_schema、allowed_labels、model_policy。
10. 定义 batch aggregation policy。
11. guard 校验 playbook。
12. dry-run 到少量 trace。
13. 发布 PlaybookVersion。
```

发布前置：

```text
evaluator rules 必须结构化。
不得包含未结构化自然语言 executable when。
preconditions / hit_when / partial_when / unknown_when 必须可执行。
llm_decision step 必须有固定 input_bundle、prompt_template、output_schema、allowed_labels、required_citations 和 model_policy。
llm_assisted playbook 必须声明 reproducibility_level。
必须完成 dry-run，并记录 dry_run_trace_count、valid_count、unknown_count、invalid_count。
用户必须确认 scope、preconditions、feature schema、evaluator rules 和适用 cohort。
不能自动把 Checklist 所有步骤都固化。
```

### 9.5 批量执行约束

模式：

```text
interactive_analysis:
  Phase 1 Checklist 探索式分析。

playbook_evaluation:
  对单条 trace 按固定 playbook 执行。
  deterministic playbook 不调用 LLM。
  llm_assisted playbook 只能执行已声明的 llm_decision steps。

batch_workspace:
  Skill / CLI 对 cohort 中多条 trace 执行同一个 playbook。

review_workspace:
  对少量 unknown / partial_hit / counter_examples 抽样复查。
```

playbook_evaluation：

```text
不进入探索式 LLM 等待态。
不允许 LLM 中途自由提交 next steps。
不走探索式用户改道。
所有执行分支来自 playbook evaluator。
preconditions 不满足时 TraceVerdict = invalid 或 unknown。
llm_assisted verdict 必须标记 basis=llm_assisted。
```

batch_workspace：

```text
max_cohort_size 由 project/profile 配置指定。
max_concurrent_playbook_evaluations 受 resource_policy 限制。
必须有 resume/retry budget。
必须定义 evidence/artifact retention policy。
必须估算 storage quota；超限进入 blocked，不静默丢 evidence。
批量进入 running 前必须完成 cohort / quota / concurrency / retention / retry budget preflight。
unknown / invalid 不计入 valid hit_rate，但必须展示。
单 trace 失败不得破坏整个 batch，可记录 unknown / invalid / error。
htrace-service 不提供 batch start / pause / resume API。
```

TraceVerdict 取值：

```text
hit
partial_hit
miss
unknown
invalid
```

GeneralityJudgement 分级：

```text
general_issue
segment_issue
candidate_issue
not_general
inconclusive
```

LLM 在批量分析中只能参与：

```text
从探索式 workspace 辅助生成 playbook。
执行已声明的 llm_decision step。
解释 batch aggregation。
总结 failure modes。
分析 unknown / partial_hit / counter examples。
建议 segment-specific playbook。
生成批量分析报告。
```

LLM 不参与每条 trace 的自由判定；llm_assisted 只表示受控结构化判断，不表示开放式专家探索。

ReviewWorkspace：

```text
不修改原始 BatchTraceResult。
不修改原始 TraceVerdict。
发现只能作为新的 hypothesis、playbook revision 或 segment-specific playbook 输入。
必须引用原始 batch result 和新的 evidence refs。
```

### 9.6 Playbook / Batch API

建议新增：

```text
POST /playbooks/from-analysis
GET  /playbooks
GET  /playbooks/{playbook_id}
POST /playbooks/{playbook_id}/validate
POST /playbooks/{playbook_id}/publish
POST /playbooks/{playbook_id}/evaluate
POST /playbooks/{playbook_id}/evaluate-chunk
POST /playbooks/{playbook_id}/validate-llm-decision
POST /cohorts/validate
POST /batch/preflight
POST /batch/aggregate
POST /batch/review-sample/prepare
```

这些都是能力型 API。`/batch/preflight` 只校验 cohort、quota、concurrency、retention、retry budget 和 resource_policy 是否满足执行前置，不创建或推进 batch lifecycle。batch workspace 的创建、暂停、恢复、进度、重试、结果清单由 Skill / CLI 写 workspace 文件，service 不保存 batch lifecycle state。

## 10. 错误、安全、测试与验收

### 10.1 错误模型

错误必须包含：

```text
code
message
actor_required
cannot_conclude
recommended_action
receipt_ref 或 diagnostic_ref
data_retained
```

### 10.2 安全与路径

```text
所有 workspace 文件必须位于 configured analysis_root 下。
所有 evidence_ref / artifact_ref / receipt_ref 必须是 analysis workspace 内相对 ref。
禁止 ../ 路径逃逸。
禁止绝对路径作为 evidence_ref。
htrace-parser server 必须来自 project allowlist 或本地配置。
Skill 不能直接把用户输入拼进 SQL。
```

路径校验必须：

```text
analysis_root canonicalize。
目标 ref join 后 canonicalize。
确认路径仍在 analysis_root/<analysis_id>/ 下。
```

### 10.3 配置测试

必须覆盖：

```text
加载 users/domains/profiles/topdown/atomics/composites/transforms/operators/strategies/templates/guards。
atomic parser kind 存在。
atomic 输出 schema 非空。
transform operator 注册和 schema。
composite 引用 atomic 存在，transform operator 已注册。
strategy 引用 checklist template 存在。
profile allowed_atomics / allowed_transform_operators / allowed_composites 存在。
problem_topdown 引用的 atomic/composite/transform operator 被 profile 允许。
guard 引用的 required evidence kind 存在。
config_snapshot_hash 稳定。
```

### 10.4 Checklist / Workspace 测试

必须覆盖：

```text
创建 workspace 会生成 Checklist.md、analysis-context.json、evidence-map.json。
Checklist 模板能从 strategy 渲染。
done step 必须有 evidence/receipt refs 或 no_output_reason。
blocked step 必须有 blocker、actor_required、recommended_action。
skipped step 必须有 reason。
恢复分析必须从 workspace 文件读取状态，不依赖 LLM 上下文。
evidence-map refs 必须存在于 evidence_index / artifact_index 或 receipts 目录。
用户改道写 plan-revisions artifact，并保留旧 evidence。
```

### 10.5 Receipt / Storage Crash Recovery 测试

必须覆盖：

```text
temp file 半写不会进入 evidence_index。
index 有记录但文件缺失时 evidence/artifact 标记 invalid。
文件存在但 index 缺失时标记 orphan，不可被 report 引用。
content_hash 不一致时 evidence/artifact 标记 invalid。
恢复 analysis 时 reconcile analysis-context、Checklist、evidence-map、index、文件树和 hash。
execution receipt 包含 attempt_id、attempt_no、params_hash、capability_version、evidence_ref。
相同 atomic + params_hash + dataset_ref 可复用 evidence，但必须返回复用 receipt。
```

### 10.6 execution.atomic 测试

必须覆盖：

```text
未知 atomic 拒绝。
缺 required input 拒绝。
参数类型不匹配拒绝。
SQL string escaping 正确。
sql_like escaping 正确。
sql_int_csv 只接受整数列表。
htrace-parser 返回无 rows 按 atomic contract 处理：允许空结果或 blocked。
htrace-parser rows 非数组拒绝。
输出列不匹配拒绝。
输出列不匹配时返回 expected_columns、actual_columns、parser_version、schema_version。
max_rows 生效。
timeout 生效。
写 evidence 成功后注册 evidence_index。
execution.atomic 不修改 Checklist。
execution.atomic 不生成 next steps。
```

### 10.7 operator / composite / summary 测试

operator engine：

```text
source evidence 不存在时失败。
source schema 不匹配时失败。
每个 reference transform operator 有稳定排序、窗口边界或分类结果测试。
startup reference transform 覆盖 selected_process、startup_anchors、phase_spans。
frame reference transform 覆盖 jank_frame_classification。
memory reference transform 覆盖 growth_segment。
scheduler / blocking reference transform 覆盖窗口聚合和 blocking_slices。
窗口型 transform 不产生负 duration。
anchor / frame / memory source 缺失时输出明确 uncertainty/blocker。
transformed evidence 必须记录 source_refs。
operator engine 不作为外部 API 暴露。
```

execution.composite：

```text
composite steps 按配置顺序执行。
quality gate 失败时返回 blocked 或 partial。
composite 不输出或自动执行 next_recommended_steps。
composite receipt 包含所有 child receipt refs。
```

summary：

```text
summary 大小有上限。
summary 不包含全量 raw rows。
summary 包含 source_ref、row_count、truncated、fields_included、omitted_fields、known_limitations。
summary truncated 或 anomaly_flags 存在时，高置信 root cause 前必须 bounded read 或提交 uncertainty。
不同 evidence kind 使用正确 summarizer。
summary 记录 source_ref。
```

### 10.8 guard 测试

必须覆盖：

```text
atomic 不在 profile allowlist 时拒绝。
transform operator 不在 profile allowlist 时拒绝。
composite 不在 profile allowlist 时拒绝。
params 引用无法解析时 blocked。
report 无 evidence refs 时拒绝。
root cause / hypothesis verdict 只引用 summary 时拒绝。
root cause / hypothesis verdict 缺 required evidence kind 时拒绝。
supporting evidence truncated 且未说明影响时拒绝。
root cause / hypothesis verdict 缺 counter_evidence_refs 字段时拒绝。
root cause / hypothesis verdict 引用其他 analysis workspace 的 evidence 时拒绝。
```

### 10.9 API 集成测试

必须覆盖：

```text
POST /datasets/register 返回 dataset_ref。
GET /datasets/{dataset_ref}/inspect 返回 table/column/row_count。
POST /datasets/{dataset_ref}/capability-probe 生成 dataset_capabilities evidence。
POST /atomics/{id}/execute 写 atomic evidence、summary、receipt。
POST /composites/{id}/execute 可通过 transform steps 写 transformed evidence、summary、receipt。
POST /evidence/{ref}/read 超限返回 blocked。
POST /artifacts 校验 evidence refs 后注册 artifact。
POST /guards/hypothesis-verdict/validate 能返回 passed/rejected。
所有 execute API 支持 Idempotency-Key。
service 不提供 workflow 状态推进接口。
service 不维护 queue。
```

### 10.10 端到端验收

通用 Checklist analysis 端到端：

```text
创建 analysis workspace。
注册 dataset。
生成 Checklist。
执行 configured probe atomics。
执行 deterministic composites。
执行 composite 内 transform steps。
生成 summaries。
Skill 写 problem report。
Skill 新增 deep analysis steps。
执行 deep analysis。
提交 hypothesis verdict。
通过 evidence sufficiency gate。
生成 final report。
Checklist 全部关键步骤 done / skipped / blocked-with-explanation。
```

reference domain 覆盖：

```text
startup reference problem 至少覆盖 path reconstruction。
frame reference problem 至少覆盖 jank classification。
memory reference problem 至少覆盖 growth segment。
scheduler 或 blocking reference problem 至少覆盖 runnable latency / blocking slice。
```

最终验收：

```text
所有 atomic evidence 已落盘。
所有 transformed evidence 有 source_refs。
所有 summaries 有 source_ref。
所有 artifact 引用 evidence refs。
所有关键 service 调用有 receipt。
Checklist 和 evidence-map 可恢复分析进度。
不存在 LLM 直接调用 htrace-parser。
不存在 report 引用临时推理。
```

### 10.11 Playbook / Batch 验收

Playbook：

```text
playbook 引用的 atomic / composite 存在，transform operator 已注册。
playbook 参数引用可解析。
feature schema 与 evaluator rules 字段一致。
hit_when / partial_when / unknown_when 可执行。
未结构化自然语言 when 被拒绝进入 evaluator。
dry-run 输出 valid / unknown / invalid 计数。
playbook version 发布后不可变。
```

LLM-assisted Playbook：

```text
llm_assisted playbook 必须声明 execution_mode=llm_assisted。
llm_assisted playbook 必须声明 reproducibility_level。
llm_decision 只能出现在 llm_assisted 或 review playbook 中。
llm_decision input_bundle 中的 refs 必须属于当前 analysis / batch trace workspace。
llm_decision bounded_reads 必须显式声明 fields 和 limit_rows。
prompt_template、output_schema、input_bundle 都必须参与 hash。
LLMDecisionArtifact 必须满足 output_schema。
LLMDecisionArtifact label 必须在 allowed_labels 中。
LLMDecisionArtifact 必须有 supporting_evidence_refs、counter_evidence_refs、uncertainties。
required_citations 不满足时 guard 拒绝。
高置信 llm_assisted verdict summary-only 时 guard 拒绝。
LLMDecisionReceipt 必须记录 model_id、model_params、prompt_hash、schema_hash、input_bundle_hash。
llm_output_replay 不重新调用 LLM，只复用已保存 artifact。
llm_audit_replay 重新调用 LLM 时必须生成对照结果和差异记录。
llm_semantic_replay 不能标记为严格复现。
llm_decision 不允许新增 next steps、写 final report 或直接覆盖 TraceVerdict。
TraceVerdict.basis 必须正确标记 deterministic 或 llm_assisted。
```

Batch：

```text
Skill / CLI 能为 cohort 中每条 trace 创建 playbook_evaluation。
batch_workspace 遵守 max_cohort_size 和 max_concurrent_playbook_evaluations。
storage quota 超限时 batch_workspace blocked。
service 不提供 batch start / pause / resume API。
deterministic playbook_evaluation 不调用 LLM。
llm_assisted playbook_evaluation 只执行已声明的 llm_decision steps。
LLM 不能在 playbook_evaluation 中途提交自由 next steps。
每条 trace 都输出 TraceVerdict。
unknown / invalid 不计入 valid hit_rate。
aggregation 统计正确。
aggregation 分开展示 deterministic 与 llm_assisted basis。
segment_issue 判断正确。
review_workspace 不修改原始 verdict。
```

## 11. AI 开发提示

当实现细节不明确时，按以下优先级处理：

```text
用户明确要求
本文件硬约束
原讨论稿展开示例
现有代码库模式
TBD / open question
```

AI 开发时必须避免：

```text
为了方便把 workflow state 放回 service。
为了节省步骤让 LLM 直接读 htrace-parser 或拼 SQL。
为了减少文件读写跳过 Checklist/evidence-map 持久化。
为了写漂亮报告引用 summary-only 或临时推理。
为了批量方便让 LLM 对每条 trace 自由判定。
为了包装复杂问题，把 llm_assisted 结果伪装成 deterministic_exact。
为了新增领域直接改 Rust 业务分支，而不是优先扩展 YAML。
```

实现优先级建议：

```text
Phase 1 最小闭环:
  config registry
  dataset register / inspect
  atomic execute
  evidence_store + summary + bounded read
  Checklist workspace protocol
  composite + transform operator engine
  artifact register / validate
  hypothesis verdict guard
  E2E single trace flow

Phase 2:
  playbook compiler
  evaluator
  batch aggregation
  review workspace
```

## 12. 原文覆盖索引

本精简稿覆盖原文信息如下：

| 原文章节                                           | 本文件位置 |
| -------------------------------------------------- | ---------- |
| 1. 背景与设计目标                                  | 1、2       |
| 2. 总体架构与边界                                  | 2、3       |
| 3. 核心模块与职责边界                              | 3          |
| 4. Analysis Workspace、Checklist 与 Skill 编排模型 | 4、8       |
| 5. 配置文件契约与 Schema 设计                      | 5          |
| 6. 模块接口 / API 设计                             | 6          |
| 7. 证据、转换数据、报告产物                        | 7          |
| 8. 完整分析流程                                    | 8          |
| 9. Playbook 与批量分析设计                         | 9、10.11   |
| 10. 测试、错误处理、安全与扩展                     | 10、11     |

原文中的长 YAML / JSON 示例、reference startup 样例、字段展开说明，保留在源文件中作为展开参考；本文件保留开发必须遵守的契约、边界和验收标准。
