# Pack Authoring Contract

本文档描述 `kat-rs-cli` 当前可执行的 pack authoring 契约，面向 LLM 和人类 reviewer。LLM 生成或修改 `derived`、`queries`、`rules`、`analyses` 前，必须先确认目标能力在本文档中声明。

本文档不是运行产物。CLI 执行 source of truth 是 pack YAML / SQL；事实 source of truth 是 `kat-rs-cli analyze` 产出的 run artifacts。

## 通用规则

- pack root 必须包含 `pack.yaml`。
- `pack.yaml` 中的文件引用必须是 pack root 内的相对路径，不能使用绝对路径、根路径或 `..`。
- transform id 必须唯一。
- analysis id 必须唯一。
- LLM 不得生成临时 SQL、临时 probe、临时代码或手写 run artifacts 绕过 pack runtime。
- 新增 pack 资源必须能通过 `cargo run -p kat-rs-cli -- analyze ...` 触发解析和执行校验。
- 传入 SQLite identifier quoting 的标识符必须非空，且只包含 ASCII 字母、数字或下划线，即 `[A-Za-z0-9_]+`。这包括 `output.table`、payload extractor 的 `source_table` / `payload_column` / `marker.column` / 输出字段名、rules.classify 的 source table / id column / text field、读取 SQLite rows 时的 table 和 column 名。SQL 自己创建、后续还会被 runtime 引用的 column alias 也应遵守该约束。
- 完整 analyze 命令模板：

```powershell
cargo run -p kat-rs-cli -- analyze --db <trace.db> --pack <pack-root> --analysis <analysis-id> --target-process <process-name> --marker <marker-name> --run-id <run-id> --run-root <run-root>
```

## Manifest

`pack.yaml` 支持字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | string | pack id，只允许 ASCII 字母、数字、`-`、`_`。 |
| `name` | string | 可选显示名。 |
| `schemas` | list[path] | 可选 schema 文件列表；当前只校验文件存在。 |
| `derived` | list[path] | derived transform YAML 文件列表。 |
| `queries` | list[path] | SQL 文件列表；当前只校验文件存在，执行时由 `sql.view` 引用。 |
| `analyses` | list[path] | analysis plan YAML 文件列表。 |
| `rules` | list[path] | rules YAML 文件列表。 |

## Derived Transform

所有 `derived/*.yaml` 都必须包含：

| 字段 | 说明 |
| --- | --- |
| `id` | transform id，也是某些 rules/extractor 配置的匹配 key。 |
| `kind` | transform kind。当前只支持 `sql.view`、`payload.extract_fields`、`rules.classify`、`marker.extract_bracket_fields`。 |
| `inputs` | 输入表列表或映射。输入表必须存在，或由 pack 中另一个 transform 产生。 |
| `output.table` | 输出 derived table 名称。 |
| `output.schema` | 输出 schema id。 |
| `output.semantic` | 可选语义标签。 |
| `materialize` | 可选 metadata / 记录用途；当前不改变 eager/lazy 执行、输出表名或持久化策略。 |
| `safety.allowedTables` | transform 允许读取的表集合。 |

### `sql.view`

用途：用 SQLite SQL 生成 derived table。

必填字段：

```yaml
id: example_view
kind: sql.view
inputs: [source_table]
sql: queries/example_view.sql
output:
  table: example_view
  schema: example.view.v1
safety:
  allowedTables: [source_table]
```

约束：

- `sql` 必须是 pack root 内相对路径。
- SQL 模板只支持 `${param_name}` 参数插值，`param_name` 读取 top-level analysis params。
- 缺失或 null 参数渲染为 SQL NULL；bool、number、string 渲染为 SQLite scalar literal。
- array / object 参数不支持，LLM authoring 应视为非法输入。
- 新增或修改 `sql.view` 时，`safety.allowedTables` 必须非空，并且必须列出 SQL 引用的全部外部表。
- runtime 可能为兼容旧资源允许空 `allowedTables`，但这是兼容行为，不是新资源 authoring 许可。
- SQL 中的 CTE alias 不计入外部表引用。
- SQL 只能作为确定性表转换交付，不能承载一次性探索逻辑。

### `payload.extract_fields`

用途：从 payload 文本字段中的 comma-delimited `key=value` 字符串提取命名整数字段，配置来自 `rules/*.yaml` 的 `extractors.<transform_id>`。

Transform 示例：

```yaml
id: first_draw_payload
kind: payload.extract_fields
inputs: [callstack]
output:
  table: first_draw_payload
  schema: marker.first_draw_payload.v1
safety:
  allowedTables: [callstack]
```

匹配 extractor 示例：

```yaml
extractors:
  first_draw_payload:
    source_table: callstack
    payload_column: marker_payload
    marker:
      column: name
      equals: firstDrawFrame:1
    fields:
      start_ts: layoutMeasureDurationStartTimestamp
      end_ts: layoutMeasureDurationEndTimestamp
```

约束：

- `extractors.<transform_id>` 必须存在。
- extractor `source_table` 必须在 transform `inputs` 中。
- `safety.allowedTables` 必须非空，并且包含 extractor `source_table`。
- `fields` 必须非空。
- source rows 会先经过 `payload_column IS NOT NULL` 过滤。
- 每个 `fields` 条目从 `payload_column` 中查找配置 key，截取到下一个逗号或字符串结尾，并生成 `CAST(... AS INTEGER)` 输出；非空 payload 内未找到 key 时输出 NULL。
- 当前输出列只包含 `fields` 配置的 output field，不会 SELECT source `*` 或保留 source row columns。
- 该 transform 只适用于逗号分隔的标量整数 payload，不支持任意字符串、嵌套结构或富 payload 解析。
- `marker` 可选；若存在，只支持 `column` + `equals`。

### `rules.classify`

用途：按字符串包含规则生成分类表。

Transform 示例：

```yaml
id: thread_identity
kind: rules.classify
inputs: [thread]
output:
  table: thread_identity
  schema: thread.identity.v1
safety:
  allowedTables: [thread]
```

Rules 示例：

```yaml
rules:
  irq_thread:
    field: thread_name
    contains: udk-irq
  io_thread_candidate:
    field: thread_name
    any:
      - fsverity
      - cdecrypt
    exclude:
      - hmfs_txn
```

约束：

- transform 必须恰好声明一个输入表。
- `safety.allowedTables` 必须非空，并且包含该输入表。
- pack 中必须恰好有一个非空 `rules` ruleset。
- 同一个 `rules.classify` transform 中所有规则必须使用同一个 `field`。
- `contains` 添加一个包含匹配项；`any` 添加多个包含匹配项；`exclude` 可为字符串或字符串数组。匹配是 case-insensitive contains matching：runtime 对 source field 使用 `LOWER(...) LIKE`，并把规则文本转为 lower-case。
- 当前输出 id column 固定为 `itid`，因此 source table 必须有 `itid`。

### `marker.extract_bracket_fields`

用途：从 `callstack.name` 中提取 bracket marker 字段，当前用于 first draw window。

示例：

```yaml
id: first_draw_window
kind: marker.extract_bracket_fields
inputs:
  - callstack
  - thread
  - process
source:
  table: callstack
  column: name
  contains: "${params.marker}"
fields:
  start_ts: layoutMeasureDurationStartTimestamp
  end_ts: layoutMeasureDurationEndTimestamp
  vsync_id: vsyncID
filters:
  process_name: "${params.target_process}"
output:
  table: first_draw_window
  schema: marker.first_draw_window.v1
safety:
  allowedTables: [callstack, thread, process]
```

约束：

- `inputs` 必须精确声明 `callstack`、`thread`、`process`。
- `safety.allowedTables` 必须包含 `callstack`、`thread`、`process`。
- `source.table` 必须是 `callstack`，`source.column` 必须是 `name`。
- `source.contains` 支持 `${params.*}` 和 `${state.*}` 字符串模板，解析后不能为空。
- `fields` 必须包含 `start_ts`、`end_ts`、`vsync_id`。
- `filters` 当前只支持 `process_name`，值必须是字符串模板。

## Rules 文件

`rules/*.yaml` 支持两个顶层字段：

```yaml
rules:
  class_name:
    field: thread_name
    contains: text
    any: [text_a, text_b]
    exclude: [text_c]
extractors:
  transform_id:
    source_table: callstack
    payload_column: marker_payload
    marker:
      column: name
      equals: firstDrawFrame:1
    fields:
      output_field: payloadKey
```

`rules` 被 `rules.classify` 消费；`extractors` 被 `payload.extract_fields` 消费。不要把策略判断、report 文案或复杂执行逻辑放进 rules 文件。

## Analysis Plan

`analyses/*.plan.yaml` 支持：

```yaml
id: openharmony.critical_path
inputs:
  target_process:
    required: true
  marker:
    default: firstDrawFrame:1
steps:
  - id: seed_root
    kind: evidence.render
    from: first_draw_window
  - id: walk_dependencies
    kind: graph.walk
    root:
      fromState: root
    providers: []
  - id: render_report
    kind: report.render
```

当前 `inputs.required` / `inputs.default` are metadata only：它们只被反序列化为 plan metadata。当前 CLI runtime params 仅限 `kat-rs-cli analyze` 注入的 `target_process` and `marker`。在 YAML 中新增 analysis input does not automatically create a CLI flag、应用 default 或校验 required input。Pack authors must not rely on new analysis inputs until CLI param plumbing exists；否则 SQL/templates may receive NULL，或运行时缺少对应 binding。

当前 step kind：

- `evidence.render`
- `graph.walk`
- `report.render`

`graph.walk` 支持：

- `root.fromState`
- `limits.maxDepth`
- `limits.maxNodes`
- `limits.maxEdgesPerNode`
- provider `id`（必填 provider id）
- provider `input.table`
- provider `match`
- provider `expand.node.sameAs`
- provider `expand.node.fields`
- provider `select.limit`
- provider `select.orderBy`
- provider `select.dedupeBy`
- provider `output.relation`
- provider `output.evidence.tables`
- provider `output.annotations`

### Required provider shape

`graph.walk` provider 必填字段：

- provider `id`
- provider `input.table`
- provider `match`
- provider `expand.node`
- provider `output.relation`

默认或可选字段：

- `limits.maxDepth`，默认 3。
- `limits.maxNodes`，默认 50。
- `limits.maxEdgesPerNode`，默认 3。
- provider `select`，默认空选择配置。
- provider `output.evidence.tables`，字段默认空列表；写 graph evidence 时，空列表 falls back to provider input.table（即 provider `input.table`）。
- provider `output.annotations`，默认空映射。
- `expand.node.sameAs` 和 `expand.node.fields` 是 alternative node expansion forms，不会合并；如果 `sameAs` 解析到值，runtime 直接返回该值并忽略 `fields`。

Provider 示例：

```yaml
providers:
  - id: parent_slice
    input:
      table: sched_slice
    match:
      temporal.overlaps:
        left:
          start: row.ts
          end: row.end_ts
        right:
          start: source.start_ts
          end: source.end_ts
    expand:
      node:
        fields:
          sliceId: row.id
          threadId: row.utid
    select:
      limit: 3
      orderBy:
        - expr: row.dur_ns
          desc: true
      dedupeBy: [row.id]
    output:
      relation: overlaps
      evidence:
        tables: [sched_slice]
      annotations:
        durMs:
          value: row.dur_ns
          scale: 0.000001
```

当前 predicate：

- `all`
- `any`
- `not`
- `eq`
- `neq`
- `gt`
- `gte`
- `lt`
- `lte`
- `exists`
- `temporal.pointWithin`
- `temporal.overlaps`

### Predicate Shape

Predicate YAML 使用以下形状：

```yaml
all:
  - eq: [row.kind, { literal: main }]
  - exists: row.ts
any:
  - gt: [row.dur_ns, { literal: 1000000 }]
  - temporal.pointWithin:
      point: row.ts
      window:
        start: source.start_ts
        end: source.end_ts
not:
  exists: row.cancelled_ts
neq: [row.state, { literal: Sleeping }]
gte: [row.end_ts, source.start_ts]
temporal.overlaps:
  left:
    start: row.start_ts
    end: row.end_ts
  right:
    start: source.start_ts
    end: source.end_ts
```

- `all` / `any` 接收 predicate 数组。
- `not` 接收单个 predicate。
- `eq`、`neq`、`gt`、`gte`、`lt`、`lte` 接收两个 binding expression。
- `exists` 接收一个 binding expression。
- `temporal.pointWithin` 接收 `{ point, window: { start, end } }`。
- `temporal.overlaps` 接收 `{ left: { start, end }, right: { start, end } }`。

当前 binding root：

- `source`
- `row`
- `facts`
- `state`
- `params`
- `node`

字符串只有等于受支持 root，或以 `root.` 形式作为前缀时，才会被解析为 binding path；有歧义的字面量使用 `{ literal: ... }`：

```yaml
eq: [row.dominant_state, { literal: Running }]
```

annotation 支持普通 binding 或缩放值：

```yaml
topSpanDurMs:
  value: row.exclusive_dur_ns
  scale: 0.000001
```

## 生成顺序

LLM authoring 必须按以下顺序：

1. 从专家 strategy 提取 evidence need。
2. 查本文档确认 CLI 是否支持目标 resource kind。
3. 查目标 pack 的 `pack-contract.md`；如果目标 pack 尚未建立 contract，先补 contract，再 author pack YAML / SQL。
4. 只有现有资源不能满足 evidence need 时，新增最小 `derived`、`query` 或 `rule`。
5. 事实表存在或可生成后，再新增或修改 `analysis`。
6. 更新 `pack.yaml` 引用。
7. 必须用 `kat-rs-cli analyze` 产出的 run artifacts 验证；静态 parse/lint/check 只能作为补充，不能替代 run artifacts。

## 当前能力缺口

当前 contract 不支持以下能力，LLM 不应生成假 YAML：

- Runnable 调度等待细分、CPU 竞争、优先级、绑核。
- blocked context inheritance。
- Binder / lock 细节。
- IO / `udk-irq` 作为 graph provider 的停止边界。
- report renderer 自定义章节和策略化文案。
- analysis plan 中的自定义 Rust provider。
