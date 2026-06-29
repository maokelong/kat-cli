# Strategy To Pack Protocol

本协议指导 LLM 将专家 strategy 转换为 pack coverage review、能力缺口列表，以及在 CLI contract 支持时的最小 pack authoring 修改。它不调用 probe，不直接查询 trace，不生成绕过 pack runtime 的临时 SQL。

## 输入

LLM 必须同时读取：

- User Question。
- Selected Strategy。
- CLI authoring contract，例如 `crates/kat-rs-cli/docs/pack-authoring-contract.md`。
- Pack resource contract，例如 `packs/openharmony-core/pack-contract.md`。
- Existing pack YAML / SQL：`pack.yaml`、`derived`、`queries`、`rules`、`analyses`。
- Pack root，例如 `packs/openharmony-core`。
- Pack mapping 文档，若存在。
- CLI analysis id，例如 `openharmony.critical_path`。
- Run inputs：db、target process、marker、run id、run root。

## 输出

LLM 在回复或 review 文档中表达：

- strategy digest。
- CLI capability fit review。
- pack resource reuse review。
- pack coverage matrix。
- gap list。
- authoring change list，若需要修改 pack。
- analyze verification command preview。
- CLI command preview。
- artifact review notes preview（运行产物审阅说明草案）。

这些输出是审阅材料。artifact review notes preview 不是可执行 checklist，也不是 CLI run 的 source of truth。

## Strategy Digest

Strategy digest 必须包含：

- `intent`：本次分析要回答的问题。
- `inputs`：db、pack、analysis、target process、marker、run id、run root。
- `root_object`：根对象如何由 pack/analysis 产生。
- `evidence_needs`：需要哪些事实。
- `control_model`：sequence、branch、frontier 或 graph walk。
- `report_contract`：Facts、Inferences、Uncertainty 要求。
- `boundaries`：局部边界和全局停止条件。

digest 不落入 `state.json`，不替代 `plan.json`。

## Capability-Gated Authoring

LLM 只有在 CLI authoring contract 明确支持时，才能生成或修改 pack YAML / SQL。

生成顺序必须是：

```text
strategy evidence need
  -> CLI capability contract
  -> existing pack resource contract
  -> minimal pack YAML / SQL change
  -> kat-rs-cli analyze run artifacts
```

`kat-rs-cli analyze` 是必需验证路径；静态 parse/lint/check 只能作为补充，不能替代 run artifacts。当前不要暗示存在独立 `validate` 命令。

对每个 strategy evidence need，先回答：

1. CLI 当前是否支持生成该事实所需的 transform kind、query safety、rules primitive、analysis step、graph predicate 或 binding root。
2. 目标 pack 是否已有可复用 derived/query/rule/analysis。
3. 如果已有资源，只修改 analysis 或 provider 消费关系。
4. 如果没有资源但 CLI 支持，新增最小 derived/query/rule。
5. 如果 CLI 不支持，记录原子能力缺口，不生成 YAML。

资源 authoring 边界：

| Resource | Allowed Basis | Forbidden |
| --- | --- | --- |
| `derived/*.yaml` | 专家证据需求 + CLI transform kind + pack 现有事实表。 | CLI 未支持的 kind；无法 materialize 的输出。 |
| `queries/*.sql` | `sql.view` 的确定性表转换。 | 探索 SQL、一次性 probe、未列入 `safety.allowedTables` 的表引用。 |
| `rules/*.yaml` | CLI rules/extractor primitive 能消费的稳定分类或字段提取数据。 | 策略判断、复杂执行逻辑、report 文案。 |
| `analyses/*.plan.yaml` | 已存在或可生成的事实表 + CLI analysis / graph 能力。 | 不存在的 derived 表、未声明的 predicate / binding / step。 |

## Pack Coverage Matrix

对每个 strategy item，检查：

- 是否有 derived table 产生事实。
- 是否有 query 或 rules 表达确定性转换。
- 是否有 analysis step 消费该事实。
- 是否有 graph provider 表达候选边或依赖关系。
- 是否有 evidence/report 字段支撑最终说明。

推荐表格：

| Strategy item | Pack resource | Analysis step | Evidence source | Status | Gap |
| --- | --- | --- | --- | --- | --- |

`Status` 只能使用：

- `covered`：现有 pack resource、analysis step 和 evidence/report 已表达该 strategy item，等待 CLI run 验证。
- `partial`：现有 pack 覆盖部分事实或部分判断。
- `unwired`：现有 pack resource 已存在，但当前 analysis 未消费，或 report/evidence 未输出。
- `missing`：现有 pack/CLI 无法表达。

## Gap 规则

gap 必须是最小能力需求，不是临时实现方案。每个 gap 至少说明：

- 缺少的事实或判断。
- 当前 strategy 为什么需要它。
- 应优先补 derived table、rules、analysis step、graph provider、report renderer 还是 CLI 能力。
- 需要的输入表或 artifact。
- 验证方式。

禁止在 gap 中附带临时 SQL、临时 probe 或一次性脚本。

当 gap 来自 authoring 能力不足时，必须标明最小缺口类型：

- `transform`：缺少 transform kind 或 transform 字段能力。
- `query_safety`：现有 SQL safety 无法表达必要表访问。
- `rules_primitive`：现有 rules/extractor primitive 无法消费目标规则。
- `analysis_step`：缺少 analysis step kind。
- `graph_predicate`：缺少 graph predicate 或 binding root。
- `graph_provider`：现有 graph walk 无法表达候选边。
- `report_support`：report renderer 无法表达必要输出。

## 映射边界

- Rust runtime primitive 只承担通用表转换、graph walk 和 report rendering 能力。
- CLI authoring contract 是 LLM 生成 YAML / SQL 的能力边界。
- Pack resource contract 是 LLM 复用或扩展 pack 的资源边界。
- 专家策略选择留在 strategy、pack、analysis plan 或 skill review 文档中。
- CLI run artifacts 是事实源。
- CLI 生成的 review artifact 只做审阅视图。
