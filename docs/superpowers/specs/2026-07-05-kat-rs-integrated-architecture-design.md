# kat-rs 整合架构设计

## 1. 文档目的

本文把以下三份设计合并成一版更完整的 kat-rs 目标架构：

- `docs/superpowers/specs/2026-07-03-kat-rs-complete-design.md`
- `docs/superpowers/specs/2026-07-05-pack-authoring-architecture-design.md`
- `docs/superpowers/specs/2026-07-05-binding-expansion-mechanism-design.md`

本文解决的是架构边界、对象模型、数据流和可验证契约，不是实现排期。若本文与三份输入文档存在表达粒度差异，以本文的分层模型为整合口径；原文档中的细节可继续作为局部设计依据。

核心结论：

```text
Pack Resource Model 由 Manifest、Business Pack、Resource Library 组成
Manifest 是 pack/resource/operator 的发现索引
Business Pack 是作者层业务问题包，可包含 pack 私有资源
pack expansion 是 daemon Orchestrator 装载 run 时的确定性展开步骤
execution snapshot / closure snapshot 是 daemon 为 run 生成并沉淀的机器审计快照
kat-rs-skill 提交 pack ref + inputs，并只基于 evidence 生成报告推断
```

## 2. 架构定位

kat-rs 是本地 trace/log 分析系统，核心价值是把异构输入转换为可复现、可查询、可审查的事实，并让分析报告基于 evidence 形成。

目标数据流：

```text
trace/log files
  -> kat-rs-datasource format/domain decode
  -> Arrow / Parquet / DataFusion dataset
  -> pack selection or authoring
  -> kat-rs-daemon REST/OpenAPI run request (pack ref + inputs)
  -> daemon Orchestrator pack expansion
  -> run-local execution snapshot / closure snapshot
  -> daemon workflow/operator execution
  -> daemon-owned run facts / diagnostics / evidence
  -> kat-rs-skill report with cited evidence
```

kat-rs 不是：

- LLM agent runtime。
- 任意代码插件平台。
- 把根因判断硬编码进 daemon core 的专家系统。
- 用自然语言上下文替代机器状态的 workflow 引擎。
- 让 CLI 与 REST/OpenAPI 维护两套业务契约的工具集合。

kat-rs 可以让人或 LLM 参与问题理解、pack 选择、Business Pack 生成和报告写作；但 pack expansion、确定性执行、状态登记和 evidence 生成必须留在 daemon 与 datasource 的机器边界内。

## 3. 总体分层

目标架构按五层组织：

```mermaid
flowchart TB
    U["用户问题"] --> S["kat-rs-skill<br/>理解问题、选择或生成 pack、提交 run、写报告"]

    S --> P["Pack Resource Model<br/>Manifest / Business Pack / Resource Library"]
    S --> D["kat-rs-daemon<br/>REST/OpenAPI、Orchestrator pack expansion、workflow 调度、operator 执行"]
    P --> D
    D --> C["run-local execution snapshot / closure snapshot<br/>flattened workflow、typed resources、context bindings、output mapping、digests"]
    D --> F["kat-rs-datasource<br/>format decode、Arrow/Parquet、catalog、DataFusion SQL"]
    D --> E["run facts / diagnostics / evidence"]
    E --> S
    S --> R["分析报告<br/>Facts / Inferences / Uncertainty / Next steps"]
```

各层职责：

| 层 | 主要职责 | 不承担 |
| --- | --- | --- |
| kat-rs-skill | 理解用户问题、选择或生成 pack、提交 `pack ref + inputs`、读取 evidence、生成报告 | 不直接执行 daemon operator，不提交预展开 runtime wiring，不绕过 evidence 输出 inference |
| Pack Resource Model | 组织 Manifest、Business Pack、Resource Library 三类资源 | 不作为 daemon 插件系统，不等同于 run-local execution snapshot |
| Manifest | 提供 pack catalog、table/resource list、atomic operators 等发现索引 | 不承载资源实现、runtime wiring 或执行契约 |
| Business Pack | 表达业务问题、输入、依赖、主流程、public artifacts、brief、examples 和 private resources | 不承担共享资源库职责，不手写 context topic、binding wiring 或 digest |
| Resource Library | 沉淀 reusable flows、query、grep、summaries 和 schemas | 不承载 pack 私有逻辑，除非经过复用提升 |
| pack expansion | 由 daemon Orchestrator 在 run 装载阶段展平 Business Pack，生成 runtime `context.subscribes` / `context.publishes` 和快照 | 不从 SQL/prose/命名惯例推断业务语义 |
| execution snapshot / closure snapshot | 承载 daemon 本次 run 可执行的机器契约和审计快照 | 不作为作者主界面，不作为 REST 顶层输入 |
| kat-rs-daemon | 装载 Pack Resource Model、执行 pack expansion、执行 workflow/operator、维护 run facts/evidence、暴露 REST/OpenAPI | 不调用 LLM，不解释自然语言策略，不扩展任意代码 |
| kat-rs-datasource | 解码输入、物化列式事实、注册 catalog、执行 DataFusion SQL | 不理解 pack/run/evidence，不承载诊断策略 |

## 4. 当前代码边界

当前 workspace 的 crate 对应目标架构如下：

| Crate | 当前职责 | 目标架构位置 |
| --- | --- | --- |
| `kat-rs-cli` | Clap 入口、日志、server 启停、OpenAPI 输出 | 本机 daemon 的薄入口；不承载业务参数解释 |
| `kat-rs-daemon` | loopback Axum server、REST DTO、datasource registry、identity、并发加载协调 | 扩展为 REST/OpenAPI 资源面、Pack Resource Model 装载、pack expansion、workflow 执行、run facts/evidence 写入层 |
| `kat-rs-datasource` | 输入适配、domain decode、Arrow/Parquet、DataFusion catalog/query | 保持事实底座；不理解 pack/run/evidence |

当前 `resources/` 目录已经体现 Pack Resource Model 的雏形：

```text
resources/
  manifest.yaml
  packs/openharmony-critical-task-extraction/
    pack.yaml
    flow.yaml
    brief.yaml
    examples/
  openharmony/
    flows/
    grep/
    query/
    summaries/
```

这套布局应作为后续 Pack Resource Model 设计的基线，而不是再引入另一套 runtime-first pack 目录。

## 5. Pack Resource Model 作者层

### 5.1 目标

Pack Resource Model 面向 pack 作者、reviewer 和选择 pack 的 skill。它应让人不阅读 daemon runtime IR，也能理解：

- 这个 pack 解决什么业务分析问题。
- 调用者必须提供哪些业务输入。
- 依赖哪些事实表。
- 主流程分几步。
- 产出哪些 public artifacts。
- 默认 brief 如何展示结果。
- 哪些 examples 可以作为 smoke/golden input。

Pack Resource Model 不应暴露 routine runtime wiring。Business Pack 的主阅读路径应短，默认优先读：

```text
pack.yaml
flow.yaml
brief.yaml
examples/*.yaml
```

### 5.2 推荐目录

```text
resources/
  manifest.yaml
  packs/
    <business-question>/
      pack.yaml
      flow.yaml
      brief.yaml
      examples/
      private/
  <domain>/
    flows/
    grep/
    query/
    summaries/
    schemas/
  generic/
    flows/
    query/
```

`resources/manifest.yaml` 是 Manifest，用于帮助人或 LLM 发现可选 pack、事实表、资源和 atomic operators。它不是 daemon execution manifest，也不是全量文件索引。

`resources/packs/<business-question>/` 是一个 Business Pack 目录。它可以包含 `private/`，但默认通过 imports 引用共享 Resource Library。私有资源一旦被多个 pack 复用，应提升到共享 Resource Library。

`resources/<domain>/<type>/...` 是 Resource Library，按领域和资源类型组织 reusable flows、query、grep、summaries 和 schemas。

### 5.3 Pack Resource Model

Pack Resource Model 包含三个稳定子模型：

- Manifest：发现索引，列出 pack catalog、table/resource list 和 atomic operators，帮助 skill 或 reviewer 选择资源。
- Business Pack：业务问题包，包含 `pack.yaml`、`flow.yaml`、`brief.yaml`、`examples/` 和 pack-local `private/` 资源。
- Resource Library：可复用资源库，包含跨 pack 复用的 flows、query、grep、summaries 和 schemas。

这三个子模型属于作者层资源表达。daemon Orchestrator 在收到 run 请求后装载它们，并在 pack expansion 阶段生成本次 run 的 execution snapshot / closure snapshot。

### 5.4 Manifest

Manifest 面向选择和生成，不参与直接执行。它可以包含非执行 summary，但不重复表列、不重复资源 input/output schema，也不影响 expansion、execution、evidence 或 provenance。

示例形态：

```yaml
schema_version: 1
kind: manifest

operators:
  grep:
    summary: Locate candidate rows with bounded predicates.
  query:
    summary: Run deterministic relational transforms.
  summaries:
    summary: Extract structured evidence from run-local artifacts.

tables:
  process:
    summary: Process facts decoded from an OpenHarmony trace.

resources:
  flows:
    openharmony.flows.locate_thread_by_process:
      summary: Select a subject thread from a process name pattern.
      path: openharmony/flows/locate_thread_by_process.yaml

packs:
  openharmony.critical_task_extraction:
    summary: Extract ranked task-level latency contributors from an OpenHarmony thread window.
    path: packs/openharmony-critical-task-extraction/pack.yaml
```

### 5.5 pack.yaml

`pack.yaml` 只描述业务问题契约：

- `pack.id`、`title`、`domain`。
- `inputs`：调用者必须提供的业务输入。
- `requires.tables`：事实表依赖。
- `imports`：稳定资源名到本 pack 本地 alias 的映射。
- `entry_flow`：入口 flow。
- `outputs.artifacts`：public artifacts 与生命周期。

规则：

- `inputs` 中声明的值全部由调用者提供，不写 `required`、`optional` 或 `default`。
- 不属于调用者业务问题的阈值、迭代次数和策略值放在 flow constants 或 atomic resource 中。
- `requires.tables` 只列事实表；列级要求由 flow 或 atomic resource 声明，展开时汇总。
- `imports` 只在 `pack.yaml` 中声明，`flow.yaml` 使用 alias，不散落路径。
- 稳定资源名不带版本；pack expansion 生成的 snapshot 记录 resolved path 和 digest。
- `outputs.artifacts` 只定义外部可依赖的 artifact，不把所有内部 step output 都暴露出去。

### 5.6 flow.yaml

`flow.yaml` 是作者层业务编排。入口 flow 默认只写顺序业务 steps：

```yaml
id: critical_task_extraction

constants:
  require_main_thread: true
  task_min_duration_ns: 1000000
  max_task_count: 12

steps:
  - id: locate_target_thread
    uses: flow
    resource: locate_thread_by_process
    output: target_thread

  - id: locate_target_window
    uses: flow
    resource: locate_window_by_markers
    output: target_window

  - id: extract_critical_tasks
    uses: query
    resource: extract_task_candidates
    output: task_candidates
```

规则：

- `uses: flow` 引用另一个作者层 flow。
- `uses: query`、`uses: grep`、`uses: summaries` 等调用 daemon 原子能力。
- flow 引用图必须是静态 DAG，禁止循环和运行时动态选择 flow。
- `flow.yaml` 不写 `bind`、carrier、runtime topic、digest 或 `context.subscribes` / `context.publishes`。
- 若某个资源需要在不同业务角色下复用，第一版通过更具体的 atomic resource YAML 或更具体的 flow 表达，不引入 flow-level alias/bind。

### 5.7 Atomic Resource YAML

atomic resource YAML 是一个具体原子能力调用形态的唯一作者层文件。它同时承载：

- `uses`：daemon operator。
- inline implementation：SQL、grep 条件或 summary 规则。
- `requires`：表列依赖。
- `context.subscribes`：该资源执行前需要的 context slots。
- `context.publishes`：该资源 commit 后发布的 context slots。
- `output`：working table、artifact 或 evidence 契约。

示例：

```yaml
id: openharmony.query.select_thread_from_process_matches
uses: query

requires:
  tables:
    thread:
      columns: [id, itid, tid, name, ipid, is_main_thread]

context:
  subscribes:
    candidate_process_ipid:
      carrier: scalar
    require_main_thread:
      carrier: scalar
  publishes:
    subject_thread_itid:
      carrier: scalar
      from:
        column: itid

output:
  table: target_thread

sql: |
  SELECT ...
```

规则：

- 资源内部只能通过 `ctx.<slot>` 使用订阅值。
- 资源不能直接读取 runtime topic 名、版本号、前序 step id、flow output 名或 pack input 对象。
- 改动 context 契约时必须同步检查 inline implementation、`output` 和发布列。
- 第一版不再把同一个原子操作拆成 `resource.yaml + .sql` 两个 review 文件；整个 YAML 及其 inline implementation 一起记录 digest。

### 5.8 brief.yaml

`brief.yaml` 是 public artifacts 的结构化 view spec。它不是 flow step，不是 daemon operator，也不是 evidence source。

规则：

- `from` 只能引用 `pack.yaml.outputs.artifacts` 中声明过的 public artifact。
- brief 只声明字段投影、排序、limit 和截断提示。
- run 完成后由 daemon/API 按 brief spec 生成默认结构化 view。
- skill 可以消费 brief 写报告，但 report inference 仍必须引用 evidence/provenance。

### 5.9 examples

`examples/*.yaml` 是可运行的典型实例，只绑定 `inputs`：

```yaml
pack: openharmony.critical_task_extraction

inputs:
  process_name_pattern: '(^|\.)tencent\.wechat$|^com\.tencent\.wechat$'
  start_marker_pattern: 'HandleLaunchAbility.*com\.tencent\.wechat'
  end_marker_pattern: 'UIVsyncTask.*firstDrawFrame\s*[:=]\s*1'
```

examples 服务四个目标：

- 给人看如何使用。
- 给 LLM 看如何实例化。
- 给测试做 smoke/golden input。
- 给 reviewer 确认业务场景没有被硬编码进通用 pack。

## 6. Pack Expansion 展开层

### 6.1 目标

pack expansion 是 daemon Orchestrator 在 run 装载阶段执行的确定性展开步骤。它从 Manifest 定位 Business Pack 和 Resource Library，解析 `pack.yaml`、entry flow、private resources 与 shared resources，展平 flow DAG，并从 atomic resource YAML 生成 runtime `context.subscribes` 与 `context.publishes`，最终写入本次 run 的 execution snapshot / closure snapshot。

展开器不解释业务自然语言，不读取 SQL 文本推断业务语义，不根据列名或 step id 猜测 binding。

### 6.2 Context Slot

context slot 是作者层给原子能力上下文参数起的业务事实名，例如：

```text
process_name_pattern
subject_thread_itid
target_window
current_anchor
task_min_duration_ns
```

context slot 只用于给原子能力提供执行坐标。需要审查、展示、排序、聚合、报告引用或跨步骤保留明细的内容，应进入 working table、public artifact 或 evidence，而不是 context slot。

设计规则：

- slot 优先保存后续步骤实际消费的直接值，例如 `ipid`、`itid`、`tid`、`ts`、`dur`、`name`、`interval.start/end`。
- `row_ref` 不是默认 carrier。只有后续步骤确实需要把某一行作为审计或 provenance 坐标，而不是立即消费该行字段时，才使用 row_ref。
- artifact、brief 和 evidence 不通过 context slot 传递。
- 运行模型中不保留与 context slot 并行的 raw `params`。

### 6.3 输入与常量

`pack.yaml.inputs` 展开为初始 context slot publications：

```yaml
inputs:
  process_name_pattern:
    type: regex
  start_marker_pattern:
    type: regex
```

flow constants 或 atomic resource constants 也通过 context slot 发布订阅进入原子能力，例如：

```text
require_main_thread
task_min_duration_ns
max_task_count
```

### 6.4 展开流程

展开按以下顺序执行：

1. 根据 run 请求中的 pack ref 读取 Manifest，定位 Business Pack。
2. 解析 `pack.yaml`、imports、entry flow、private resources 和所有被引用共享资源。
3. 校验 flow reference graph 是静态 DAG。
4. 将所有 `uses: flow` 完全内联，生成 flattened atomic invocation list。
5. 将 `pack.yaml.inputs` 与 flow/resource constants 生成初始 context slot publications。
6. 按 flattened atomic invocation list 顺序处理每个原子能力调用。
7. 读取对应 atomic resource YAML 的 `context.subscribes`，解析到当前 scope 中已发布的唯一 slot。
8. 生成 runtime `context.subscribes`，并把 `ctx.<slot>` 渲染为确定性模板绑定。
9. 记录 inline implementation、resource digest、expanded content 和 context binding 结果。
10. 根据 atomic resource YAML 的 `context.publishes` 生成 runtime `context.publishes`。
11. 根据 `output`、pack public outputs 和 brief spec 生成 output mapping。

绑定解析发生在展平后的原子能力层级，不发生在 flow-to-flow 边界。

### 6.5 Context Slot 解析规则

普通顺序 flow scope 中：

- 订阅的 slot 必须已经由初始输入、常量或前序原子能力发布。
- 订阅必须解析到唯一已发布 slot；缺失或歧义是结构性失败。
- 同名 slot 默认只能发布一次；重复发布是结构性失败。
- 订阅或发布的 carrier 必须与 atomic resource YAML 声明一致。
- `ctx.*` 只能引用 `context.subscribes` 声明过的 slot。

显式 loop/state scope 中：

- 允许同名 slot 被循环体重复发布为新版本。
- 后续订阅解析到当前 scope 的 latest 版本。
- 只有 loop/state 明确声明的推进 slot 可以使用 latest 语义。
- 版本链、producing step 和来源 refs 由 daemon run facts 记录。

因此，`latest` 是显式状态推进的 runtime 语义，不是普通作者层 flow 的隐式覆盖规则。

### 6.6 展开期结构性失败

以下情况应在 pack expansion 或 snapshot 校验阶段失败：

- `flow.yaml` 出现 flow-level `bind` 或 runtime context wiring。
- `uses` 不是 daemon 原子能力或 `flow`。
- 原子能力 step 缺少 `resource`。
- resource 的 `uses` 与 step `uses` 不一致。
- atomic resource YAML 缺少 inline implementation。
- `context.subscribes` 引用未发布 slot。
- 普通 scope 中同名 slot 重复发布。
- `ctx.*` 引用未声明 slot。
- `context.publishes.from` 引用不存在的输出列。
- 前序已确定对象只发布 row_ref，导致后续 query 仅为补字段而回事实表 join。
- artifact 或 evidence 试图通过 context slot 发布。
- carrier 使用 daemon 不支持的类型。

## 7. Run Execution Snapshot / Closure Snapshot

execution snapshot / closure snapshot 是 daemon 在 pack expansion 后为本次 run 生成的结构化机器快照。它不是 Business Pack 目录，不是作者主界面，也不是 REST `/runs` 的顶层输入。

snapshot 至少包含：

```text
pack identity snapshot
entry workflow definition
flattened atomic invocation list
transitive typed resources
inline implementation expanded content
resource paths and digests
requires_initial_context schema
initial context bindings
runtime context.subscribes / context.publishes
public output mapping
brief view spec reference or snapshot
schema versions
```

snapshot 的目标：

- 保证同一 dataset、同一 Pack Resource Model 快照、同一输入生成相同机器契约。
- 让 daemon 后续执行不依赖实时资源目录扫描。
- 让 run 在 Business Pack 或 Resource Library 后续变化后仍可审查。
- 让资源展开、模板绑定和 digest 冲突可被复现和定位。

daemon 后续执行只读取本次 snapshot 显式引用的结构化内容，不重新扫描 pack，不自动发现未引用资源，不解释自然语言策略文档。

## 8. kat-rs-daemon Runtime

### 8.1 资源面

REST/OpenAPI 是 daemon 对外暴露的唯一业务功能面。CLI 如果存在，只是本机 server 生命周期和 OpenAPI 输出的薄入口。

目标资源面至少包括：

```text
POST /v1/runs
GET /v1/runs/{runId}
GET /v1/runs/{runId}/evidence
GET /v1/runs/{runId}/brief
POST /v1/grep
POST /v1/query
POST /v1/summaries
```

主分析链路应通过 `/runs` 提交 pack ref、dataset ref 和 inputs，由 daemon Orchestrator 装载 Pack Resource Model、执行 pack expansion 并调度 workflow，不绕过 workflow 直接拼装 operator 调用。受控的 `grep/query/summaries` 资源动作可保留为检查和低层查询入口，但不成为 skill 主链路的替代协议。

### 8.2 Run

run 是一次分析执行记录，绑定：

- dataset ref。
- pack expansion 生成的 execution snapshot / closure snapshot。
- entry workflow invocation。
- step status。
- working tables。
- context bindings。
- diagnostics。
- evidence。
- public output mappings。

run 不保存用户自然语言问题、LLM 对话或 pack 策略 prose。自然语言问题由 skill 持有；run 只保存机器可审查事实。

### 8.3 Workflow 与控制结构

workflow 是 pack-defined 的确定性业务分析过程，但由 daemon 解释 schema、控制原语和 operator 调用。

workflow 可以包含：

- 顺序 steps。
- branch。
- bounded loop。
- append-only accumulator。
- context subscriptions/publications。
- public outputs。

workflow 不能：

- 动态生成 step list。
- 动态生成 operator name。
- 执行任意脚本。
- 调用 LLM。
- 依赖自然语言 summary 或用户对话上下文分支。

branch 只能基于 daemon-visible structured machine facts：

- row count。
- context binding 是否存在。
- context set 是否为空。
- scalar binding。
- capability status。
- diagnostic code。
- evidence metric。
- loop iteration index。

### 8.4 原子能力

daemon-owned operator 第一版收敛为：

```text
grep
query
summaries
sequence.*
interval.*
graph.*
```

规则：

- operator 不互相调用。
- operator 组合必须在 workflow 中显式表达。
- pack 不能定义新 operator。
- 领域诊断名称留在 pack workflow 和 artifact 语义中，不泄漏为 daemon capability name。

`grep` 用于少量候选定位，输出 matches working table，不成为第二套 SQL DSL。

`query` 承载 DataFusion SQL 可表达的关系型变换，输出有名 working table。

`summaries` 从表、diagnostics、workflow trace 或 capability output 中摘录小事实并写入 evidence，不写自然语言 summary，不输出 inference。

`sequence.*` 承载有序事件配对和状态区间还原等通用算法。

`interval.*` 承载区间 overlap、intersect、subtract、merge、gaps、coverage 等通用计算。

`graph.*` 承载 build、traverse、path_search 等通用图计算。pack 先用 query 构建业务 node/edge 表。

### 8.5 Binding Mechanism

daemon 内部维护 typed context topic store。它是 runtime 执行机制，不是用户直接选择的分析能力。

carrier 第一版收敛为：

| carrier | 形态 | 边界 |
| --- | --- | --- |
| `scalar` | JSON scalar | 不支持 array/object/blob/nested context |
| `row_ref` | table + `_kat_row_id` | run-local，不使用业务主键或文件 offset |
| `table_ref` | registered table name | 不包含 filter/projection/order |
| `interval` | numeric start/end | 不包含 unit、clock、anchor、provenance |

runtime 语义：

- step 执行前解析 `context.subscribes`。
- step 成功 commit 后登记 `context.publishes`。
- 每次 publication 都形成不可变 binding version。
- daemon 可维护 topic latest 指针，但 run facts 必须保留版本链。
- 失败 step 的 staged publications 不进入 committed store。

context 是轻量执行坐标，不是完整 provenance。为什么选择某个 context，由 producing step、workflow trace、diagnostics 和 evidence 解释。

### 8.6 Working Table、Public Artifact 与 Evidence

working table 是 run-local 中间事实，可被后续 step 引用。

public artifact 是 `pack.yaml.outputs.artifacts` 显式声明、并由展开后 output contract 对齐的外部输出。

brief 是 public artifact 的默认结构化 view。

evidence 是报告可引用的小型结构化事实，包含 facts、metrics、refs 和 producing step。

规则：

- working table 默认不发布到 dataset catalog。
- public artifact 不自动成为 evidence。
- diagnostics 不自动成为 evidence。
- context binding 不自动成为 evidence。
- 若报告需要引用 diagnostic、context publication 或 artifact 内容，必须由 summaries 显式摘录成 evidence。

### 8.7 Run Facts

run facts/evidence 是 daemon 内部机器事实集合，append-only 保存：

```text
run identity
dataset ref
closure snapshot
entry workflow invocation
top-level status
step records
workflow nested trace
working tables
context binding versions
context publication records
context sets
diagnostics
handled diagnostics
evidence records
public output mappings
brief view records
```

每个 step 使用 per-step staging + commit：

```text
reserve outputs
-> execute operator
-> validate outputs
-> commit working tables / diagnostics / evidence / context publications
-> expose committed outputs to later steps
```

如果 step 中途失败，staging 产物不进入 run query context。已 commit 的前序事实不回滚。

### 8.8 Diagnostics 与失败

失败分为结构性失败和数据性 diagnostics。

结构性失败立即 fail-fast：

- schema 校验失败。
- resource 缺失。
- digest 不一致。
- manifest ref 不存在。
- table/column 不存在。
- output name 冲突。
- carrier type 不支持。
- `_kat_row_id` 等保留列被覆盖。
- public output 映射缺失或 kind 不匹配。

数据性情况不自动失败：

- grep 无匹配。
- query row count 为 0。
- sequence 无配对。
- frontier 为空。
- graph 无 target path。
- loop 到达 max_iterations。

这些情况应生成 diagnostics，由 branch、loop termination 或 summaries 显式处理。handled diagnostics 仍进入 trace，但不自动让 workflow 完成状态降级。

## 9. kat-rs-datasource 事实底座

datasource 负责：

- `.htrace` container 读取。
- profiler envelope framing 与 decoder registry。
- ftrace/native hook domain decode。
- Langfuse legacy JSONL.GZ 读取。
- Arrow RecordBatch / MemTable 物化。
- Parquet dataset writer。
- dataset catalog 读写。
- DataFusion SQL 查询。

datasource 边界：

- 不理解 run、pack、workflow、context 或 evidence。
- 不保存报告语义。
- 不硬编码关键路径、首帧、卡顿、内存泄漏等诊断策略。
- 不把 source/raw/direct 表提前转换成不可组合 JSON blob。

新增输入格式或 profiler plugin 时，改动应限制在格式读取、domain decode、必要 codegen、Arrow projection 和测试，不污染 daemon runtime。

## 10. kat-rs-skill 与报告模型

kat-rs-skill 是面向用户问题的入口，负责：

1. 理解用户问题、目标和约束。
2. 根据 Manifest 和已有 Business Packs 选择合适 pack。
3. 必要时生成新的 Business Pack 或 pack variant。
4. 通过 REST/OpenAPI 提交 pack ref、dataset ref 和 inputs。
5. 由 daemon Orchestrator 装载 Pack Resource Model 并执行 pack expansion。
6. 读取 run status、brief、diagnostics 和 evidence。
7. 生成面向用户的报告。

报告可以包含：

- Facts：直接来自 evidence。
- Inferences：基于 evidence 的推断。
- Uncertainty：证据不足、diagnostics、数据缺口或未覆盖路径。
- Next steps：后续可提交的新 pack/workflow invocation。

报告不得把 daemon 未产出的自然语言判断伪装成 evidence。每个关键 inference 都必须能追溯到 evidence record。

## 11. 关键不变量

1. Pack Resource Model 是作者层资源表达，不是 daemon 插件系统。
2. Business Pack 是业务问题包，private resources 在未复用前保持 pack-local。
3. REST/OpenAPI 主链路提交 pack ref、dataset ref 和 inputs，不提交预展开 runtime wiring。
4. daemon Orchestrator 执行 pack expansion，并生成本次 run 的 execution snapshot / closure snapshot。
5. snapshot 是机器契约和审计快照，不是作者主界面。
6. pack expansion 不从 SQL、列名、step id 或 prose 推断业务语义。
7. 所有影响 operator 执行的输入和常量都进入 context slot 发布订阅模型。
8. 运行模型中不保留第二套 raw `params`。
9. context slot 只承载轻量执行坐标，不承载 public artifact、brief 或 evidence。
10. evidence 是报告 inference 的唯一事实支撑入口。
11. diagnostics 只有被 summaries 显式摘录后才成为 evidence。
12. working table、context publication 和 evidence 只有 step commit 后才进入 run facts。
13. run facts append-only，已 commit 历史不回写、不覆盖。
14. pack 不能定义 operator、carrier type、runtime control primitive 或任意代码执行能力。
15. CLI 不维护与 REST/OpenAPI 平行的业务契约。

## 12. 首个可验证切片

目标架构可以分阶段实现，但第一条验证链路应尽量证明主路径，而不是只堆局部对象。

建议首个可验证切片：

```text
已有 dataset
  -> resources/packs/openharmony-critical-task-extraction
  -> POST /v1/runs(pack ref + dataset ref + inputs)
  -> daemon Orchestrator 装载 Manifest / Business Pack / Resource Library
  -> pack expansion 生成 execution snapshot / closure snapshot
  -> daemon 执行 query working tables
  -> daemon 执行 summaries evidence
  -> GET /v1/runs/{id}/evidence
  -> skill/report 引用 evidence
```

该切片至少验证：

- Business Pack 不含 runtime binding wiring。
- Manifest 可发现 pack catalog、table/resource list 和 atomic operators。
- Business Pack 可引用 Resource Library，也可包含 pack-local private resources。
- flow 可展平成原子能力调用列表。
- atomic resource YAML 生成 context subscriptions/publications。
- execution snapshot / closure snapshot 记录资源 path、digest 和 expanded content。
- daemon 只执行 snapshot 显式引用资源。
- query 产出 working table。
- summaries 产出 evidence。
- evidence 可追溯 producing step 与 refs。
- brief 只消费 public artifacts。
- 报告 inference 可逐条引用 evidence。

非首个切片内容：

- 完整 `sequence.*`、`interval.*`、`graph.*` 算法族。
- persistent derived artifact 发布到 dataset catalog。
- 复杂 loop/state scope。
- 远程、多用户、鉴权或 TLS。
- 任意 pack marketplace 或插件安装机制。

## 13. 验收标准

架构层验收：

- 人只读 Business Pack 主路径即可理解业务问题、输入、依赖、主流程、输出和 brief。
- `pack.yaml` 中没有 runtime path、digest、carrier、context topic 或 binding wiring。
- `flow.yaml` 中没有 `bind`、runtime topic、carrier 或 digest。
- 每个原子能力调用的上下文订阅和发布都来自 atomic resource YAML。
- 同一输入、同一 Pack Resource Model 快照和同一 dataset 生成相同 execution snapshot binding wiring。
- public artifact、brief 和 evidence 不依赖 context slot 作为结果总线。
- daemon run facts 能解释执行时使用的 workflow、资源展开内容、context publication 和 evidence refs。

运行层验收：

- 同一 dataset、同一 snapshot、同一 initial context 产生相同 run facts。
- skill 的主链路通过 REST/OpenAPI 提交 run 和读取 evidence。
- working table 和 context publication 只有 commit 后才进入 run query context。
- evidence 能追溯到 producing step 和来源 refs。
- operator 之间不存在隐藏互调。
- pack workflow 的业务语义不泄漏为 daemon operator name。
- report inference 可以逐条引用 evidence。

## 14. 关键取舍

### 14.1 Business Pack 与 execution snapshot 分离

作者层追求可读、可 review；runtime 层追求确定性和可审计。把 Business Pack 与 run-local execution snapshot 分离，可以避免让 pack 作者面对大量 routine wiring，同时保留 daemon 所需的 digest、binding 和输出映射。

### 14.2 binding 由 atomic resource YAML 派生

绑定语义放在具体 atomic resource YAML，而不是 flow 主线或集中映射表中。这样 review 一个原子能力时可以同时看到 operator、实现、上下文依赖和输出契约，避免从 SQL 或命名惯例推断业务含义。

### 14.3 context 是执行坐标，不是证据容器

context slot 只传递后续 operator 需要的小型值或引用。需要审查和报告的内容必须落成 table、artifact 或 evidence。这避免 context 变成隐式全局状态或 LLM 记忆。

### 14.4 evidence 是报告事实入口

report 可以有 inference，但 inference 必须引用 evidence。daemon 不输出自然语言根因判断，skill 不把自己的判断伪装成机器事实。

### 14.5 daemon 保持通用 operator 集

daemon 提供 grep/query/summaries/sequence/interval/graph 等通用能力。领域问题如 critical path、first frame、wake-up source 留在 pack workflow 和 artifact 语义中，避免 daemon core 被业务诊断名称污染。

## 15. 待后续设计的问题

以下问题不阻塞本文主架构，但后续实现前需要单独设计：

- execution snapshot / closure snapshot 的完整 JSON/YAML schema。
- `/v1/runs` 的 REST DTO、状态机、pack ref + inputs 结构和持久化边界。
- pack expansion 的 crate/module 边界：daemon 内部模块还是独立 crate。
- brief view 的 API 返回格式和分页/截断语义。
- evidence record 的稳定 schema 与 refs 结构。
- loop/state scope 的作者层表达和 runtime 状态推进细节。
- persistent artifact 写入 dataset catalog 的生产者元数据。
- run facts 的内存结构、落盘格式和生命周期。
