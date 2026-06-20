# Pack derived/runs dataset layout 设计

## 背景

Issue #45 将 kat-rs 的长期方向收敛为：

```text
trace columnar data engine + semantic transform pack runtime
```

PR #59 已交付第一版本地列式 dataset 持久化内核：source materializer 将 `.htrace` 和 Langfuse legacy 输入写成 Parquet，根 `catalog.json` 记录 SQL 逻辑表名到 dataset 内相对路径的映射，`TraceDatasource::from_dataset(...)` 再从 catalog 注册 Parquet 表供 DataFusion 查询。

#60 曾承接 dataset 持久化后续项，但 #74/#75、#76/#77、#78/#79 已经把 REST materialization、direct dataset query 和 dataset lifecycle 闭环交付完成。`derived/` 和 `runs/` 不再属于 #60 的 REST/API lifecycle 范围；它们属于 #45 的 pack transform 与 analysis runtime 方向。

本设计为 #82 的最小切片：定义 pack derived tables 如何落在 dataset 之上，analysis runs 如何独立记录执行状态，并避免把未验证的 pack runtime 细节提前固化进 dataset layout。

## 要解决的问题

本设计要解决：

1. 让一个 dataset 能统一发现和查询 source tables 与 derived tables。
2. 明确 `derived/` 的最小目录和 catalog 边界。
3. 明确 `runs/` 不属于 dataset 目录，而是独立 analysis run store。
4. 明确 source of truth：表注册、schema、transform 语义、analysis state 分别由谁承担。
5. 明确哪些 metadata 不进入第一版 catalog。
6. 给出小于完整 pack runtime 的最小实现切片和验证方式。

## 不做什么

1. 不实现完整 pack runtime、transform DAG compiler 或 analysis runtime。
2. 不实现任意用户脚本、Rust/WASM extension 或插件系统。
3. 不改现有 dataset REST API 的资源形态。
4. 不把 Markdown checklist 当机器执行协议。
5. 不把 JSON evidence 或 query result 当中间事实层；中间事实层保持 Arrow/Parquet/DataFusion 表。
6. 不提前设计跨版本 catalog migration。当前本地 dataset 仍是可重建产物，结构不匹配时拒绝并要求重建。
7. 不在第一版 catalog 中写入 row count、schema、统计信息、输入 provenance、run/evidence/report 引用或完整 pack manifest。

## 方案选择

采用 **统一 dataset catalog + 独立 runs store**。

备选方案及取舍：

1. 统一根 `catalog.json` 登记 source 与 derived tables。本方案产品模型最清楚：用户打开一个 dataset，就能看到这个 dataset 内所有可查询表；query/inspect/DataFusion 注册也只有一个表注册源。本设计选择它。
2. source catalog 与 derived catalog 分离。这样可以少改 #59 的临时 catalog 结构，但用户和 runtime 需要理解两套表发现机制，后续 query、inspect、pack 输出发现都会更复杂。
3. 把 derived outputs 放进 `runs/<run_id>/`。这样单次 analysis 目录自包含，但 derived table 本质上是可复用列式事实层，不应绑定到某次 analysis execution。

项目尚未发布，没有历史兼容债务；因此可以破坏当前 catalog 结构。破坏性变更的目标不是保留临时合同，而是建立更好的长期对象模型：dataset 是统一 catalog 管理的可查询列式事实资产，run 是独立 execution record。

## 对象模型

本设计区分两个产品对象：

- `dataset`：可查询、可重建的列式事实资产，包含 source tables 和 derived tables。
- `run`：一次 analysis 执行记录，通过 `datasetRef` 引用 dataset，不属于 dataset 生命周期。

目录形态：

```text
<dataset>/
  catalog.json
  tables/
    hitrace.sched_switch.parquet
  derived/
    <pack_ref>/
      <transform_id>.<output_table>.parquet

<kat data dir>/
  runs/
    <run_id>/
      plan.json
      state.json
      evidence.jsonl
      report.md
```

`tables/` 保存 ingestion 产生的 source tables。这里的 source tables 包括 raw/direct 查询面，例如 `profiler_plugin_data`、sched direct tables、native hook direct/raw tables、`langfuse_observations` 和 `langfuse_traces`。

`derived/` 保存 pack transform 物化出的列式表。derived table 是 dataset 的一部分，因为它基于该 dataset 的 source/derived 表确定性生成，可以被 DataFusion 继续查询和复用。

`runs/` 位于平台数据目录下的独立 run store。run 引用 dataset、pack、analysis 和参数，但不是 dataset 的组成部分。dataset 删除或移动后，run 可能不可重放；已有 `evidence.jsonl` 和 `report.md` 仍可审阅。

## Catalog 合同

`catalog.json` 是 dataset 的唯一表注册源。查询侧只读根 catalog 注册表，不扫描 `tables/` 或 `derived/` 推断表。

第一版 catalog 结构：

```json
{
  "tables": [
    {
      "name": "sched_switch",
      "path": "tables/hitrace.sched_switch.parquet",
      "kind": "source"
    },
    {
      "name": "thread_state_segments",
      "path": "derived/openharmony-core-abcd1234/thread_state_segments.parquet",
      "kind": "derived",
      "producer": {
        "packRef": "openharmony-core-abcd1234",
        "transformId": "thread_state_segments"
      }
    }
  ]
}
```

字段语义：

- `tables[].name` 是 SQL 逻辑表名，必须在 dataset 内唯一。
- `tables[].path` 是 dataset 内相对路径，不能是绝对路径，不能包含 `..`，不能逃逸 dataset 根目录。
- `tables[].kind` 第一版只接受 `source` 和 `derived`。
- `tables[].producer` 只允许出现在 derived table 上。
- `producer.packRef` 是目录安全的 pack materialization id。第一版只要求它稳定、可比较、可放进路径，并能区分不同 pack 内容；不在本设计中绑定具体 hash 算法。
- `producer.transformId` 来自 pack transform spec，用于说明该 derived table 由哪个 transform 产生。

根 catalog 不写 `version`。当前 dataset 是可重建本地产物，项目也未承诺跨版本 layout 兼容；加入 `version` 会提前暗示迁移和兼容义务。reader 严格按当前结构解析，未知字段或结构不匹配时拒绝，并要求重新 materialize 或重算 derived tables。

## Derived layout

derived table 物理路径按 pack materialization 分组：

```text
<dataset>/
  derived/
    <pack_ref>/
      <transform_id>.<output_table>.parquet
```

`<pack_ref>` 用于隔离不同 pack 或不同 pack 内容版本的输出，避免互相覆盖。`<transform_id>` 来自 pack transform spec。`<output_table>` 是输出表名的目录安全映射；真正的 SQL 逻辑表名仍以 catalog 中的 `tables[].name` 为准。

一个 transform 输出多张表时，catalog 登记多条 `kind: "derived"` table entry。它们可以共享同一个 `producer.packRef` 和 `producer.transformId`，但 `name` 和 `path` 必须不同。

第一版不支持 append、增量更新、derived invalidation、跨 pack dependency resolution 或自动 replace。同名 table 已存在时，写入 derived table 应失败，除非后续独立设计显式 replace 语义。不要静默覆盖 catalog 中已有表。

derived 写入采用 best-effort 发布流程：

1. 从 source/derived tables 读取输入并执行 transform。
2. 将输出写入 `derived/<pack_ref>/...parquet`。
3. 校验新 Parquet 文件可读。
4. 校验更新后的 catalog 可注册 source 和 derived tables。
5. 发布 catalog entry。

如果 catalog 更新失败，孤儿 derived 文件可以忽略或由后续清理流程删除；它不是事实层 source of truth。只有被根 `catalog.json` 登记且通过 reader 校验的表才是可查询事实。

## Runs store

analysis run 使用独立 store：

```text
<kat data dir>/
  runs/
    <run_id>/
      plan.json
      state.json
      evidence.jsonl
      report.md
```

`plan.json` 是 run 执行计划的 source of truth，至少应表达 `runId`、`datasetRef`、`packRef`、`analysisId` 和参数。`datasetRef` 引用 dataset locator，不复制 dataset catalog 或表数据。

`state.json` 是机器执行状态的 source of truth，用于记录 analysis runtime 的步骤、frontier、visited、决策状态、错误状态等。具体状态字段属于后续 analysis runtime 设计，不在 #82 固化。

`evidence.jsonl` 是证据账本。每行可以记录小证据摘要、SQL、表名、参数、行范围或 artifact 引用，但不能保存可继续组合查询的大中间结果。只要后续步骤还需要查询或复用，中间结果就应落成 derived table 或后续设计中的临时列式表。

`report.md` 是人类可读输出。未来如果保留 `checklist.md`，它也只能由 plan/state/evidence 渲染，不是机器执行协议。

## Source of truth

source of truth 分层如下：

| 内容 | Source of truth |
| --- | --- |
| dataset 内有哪些可查询表 | 根 `catalog.json` |
| 表 schema | Parquet metadata |
| source table 物理数据 | `tables/*.parquet` |
| derived table 物理数据 | `derived/<pack_ref>/*.parquet` |
| derived table 语义 | pack manifest / transform spec |
| analysis 执行计划 | `runs/<run_id>/plan.json` |
| analysis 机器状态 | `runs/<run_id>/state.json` |
| 小证据账本 | `runs/<run_id>/evidence.jsonl` |
| 人类报告 | `runs/<run_id>/report.md` |

JSON query result 和 JSON evidence 只能是事实摘要或引用，不是中间事实层。中间事实层保持 Arrow/Parquet/DataFusion 表。

## Metadata 边界

第一版 catalog 只保存表注册所需信息：

- `tables[].name`
- `tables[].path`
- `tables[].kind`
- derived table 的 `tables[].producer.packRef`
- derived table 的 `tables[].producer.transformId`

第一版 catalog 不保存：

- root `version`
- `rowCount`
- `columns`
- `schemaFingerprint`
- min/max、null count、分区统计等 stats
- 输入文件路径、mtime、大小、hash
- 完整 transform DAG
- SQL 文本
- rule/classifier 内容
- pack manifest 副本
- run id
- evidence 引用
- report/checklist 路径
- `createdAt`
- `createdBy`
- generator version

这些信息不是当前表注册闭环必须项。后续如果需要 provenance、stats、schema compatibility 或 migration，应以独立 issue/SDD 设计，不从本切片的 catalog 里顺手扩张。

## 读取与查询

`TraceDatasource::from_dataset(path)` 后续读取流程：

1. 读取根 `catalog.json`。
2. 校验 catalog 结构、表名唯一、path 不逃逸 dataset、Parquet metadata 可读。
3. 对每个 table 按 `tables[].name` 注册 `tables[].path` 指向的 Parquet 文件。
4. SQL 查询仍由 DataFusion 执行。

source table 和 derived table 对 query 层都是 DataFusion 表。`kind` 和 `producer` 用于 inspect、debug 和后续 pack/runtime 决策，不改变 SQL 注册方式。

## 错误处理

第一版错误应区分：

- catalog 结构不符合当前合同；
- catalog 包含未知字段；
- table name 重复；
- table path 是绝对路径、包含 `..` 或逃逸 dataset 根目录；
- table path 指向的 Parquet 文件缺失或 metadata 不可读；
- `kind` 不是 `source` / `derived`；
- source table 携带 `producer`；
- derived table 缺少 `producer.packRef` 或 `producer.transformId`；
- derived table name 已存在；
- catalog 更新后无法重新注册 dataset。

旧结构不兼容时不做迁移。用户应重新 materialize dataset 或重算 derived tables。

## 最小实现切片

后续第一刀不实现完整 pack runtime，只验证 layout、catalog 和查询闭环：

1. 破坏性升级 `catalog.json` reader/writer，从 `{ tables: [{ name, path }] }` 改为 `{ tables: [{ name, path, kind, producer? }] }`。
2. source materialize 写 `kind: "source"`。
3. 增加 datasource crate 内部 derived writer helper：写 Parquet 到 `derived/<pack_ref>/...parquet`，校验后更新根 catalog。
4. 同名 table 已存在时失败，不静默覆盖。
5. `TraceDatasource::from_dataset` 从同一个根 catalog 注册 source 和 derived tables。
6. dataset inspect 展示 source 和 derived table 的 `kind`；可选展示 derived producer，但不展示 schema、row count 或 provenance。
7. 不实现 `runs/` runtime，只在本 SDD 明确独立 run store 边界。

用于验证的 derived transform 应产生可继续查询的列式表，而不是 JSON evidence。可以从 fixture source table 投影或过滤出一张 derived table，证明 derived 表进入 catalog 后可由 DataFusion 查询。

## 验证

自动化验证：

1. source dataset materialize 后，根 `catalog.json` 只包含 `tables`，source entry 有 `name`、`path`、`kind: "source"`，没有 root `version`。
2. 写入 fixture derived table 后，Parquet 位于 `derived/<pack_ref>/...parquet`，catalog 新增 `kind: "derived"` 和 `producer.packRef` / `producer.transformId`。
3. `TraceDatasource::from_dataset` 能查询 source table 和 derived table。
4. 同名 derived table 写入失败。
5. catalog 未知字段、重复表名、绝对 path、`..` path、缺失 Parquet、非法 `kind`、非法 `producer` 都被拒绝。
6. 本切片不创建 `runs/`，不把 run id、evidence、report 路径写入 dataset catalog。

SDD/PR 验收：

1. 说明 #82 属于 #45 的 pack transform / analysis runtime，不属于 #60 REST/API lifecycle。
2. 说明 `runs/` 是独立 run store，通过 `datasetRef` 引用 dataset。
3. 说明 Markdown checklist/report 不是机器执行协议。
4. 说明 JSON evidence/query result 不是中间事实层。
5. 说明第一刀小于完整 pack runtime，只交付 derived layout、catalog 注册和查询闭环。

提交前基础验证：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python .github/scripts/test_pr_guard.py
git diff --check
```
