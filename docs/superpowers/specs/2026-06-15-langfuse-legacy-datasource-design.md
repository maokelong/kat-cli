# Langfuse legacy datasource 设计

## 要解决的问题

`kat-rs` 需要增加一个本地 Langfuse blob export 查询入口，让用户用 SQL 分析 self-hosted Langfuse 导出的 legacy `observations/*.jsonl.gz` 和 `traces/*.jsonl.gz` 文件。

当前确认的真实导出结构是 legacy 模式：有 `observations/`、`traces/`、`scores/`，没有可用的 `observations_v2/` 数据。第一版只处理 observations 和 traces，因为它们已经能覆盖 trace 与 observation 的主要时序、输入输出、用户、会话和名称分析。

## 来源依据

Langfuse 官方文档说明 blob export 有三种 source mode：

1. 推荐的新模式写入 `observations_v2/` 和 `scores/`，每条 observation 内联 trace context。
2. legacy 模式写入 `traces/`、`observations/`、`scores/`。
3. 混合模式同时写入上述文件。

legacy 模式下，`observations/` 只包含 observation-level 字段，不包含 `user_id`、`session_id`、`tags` 等 trace-level 字段；需要通过 `observations.trace_id = traces.id` join。官方字段说明也明确 `input` 和 `output` 是字符串 payload，可能是纯文本或 JSON，可能很大。

参考：

- https://langfuse.com/docs/api-and-data-platform/features/blob-storage-export-fields
- https://langfuse.com/docs/api-and-data-platform/features/export-to-blob-storage
- https://langfuse.com/docs/observability/data-model

## 不做什么

1. 不支持 Langfuse API 拉取。
2. 不支持 COS、S3 或其他对象存储直连。
3. 不支持 `observations_v2/`。
4. 不支持 `scores/`。
5. 不支持目录扫描、多文件 glob、自动发现最新文件或按时间窗口合并。
6. 不提供 `langfuse_parse_errors` 表。
7. 不提供 timeline、quality、rollup 等派生表。
8. 不解析 `metadata`、`usage_details`、`cost_details`、`input`、`output` 等 JSON 字符串或对象为额外列。
9. 不截断、摘要或脱敏 `input` / `output`。
10. 不改变 hitrace datasource 的行为。

## 最小交付

CLI 增加一个 Langfuse source：

```bash
kat-rs query \
  --source langfuse \
  --observations-file <observations.jsonl.gz> \
  --traces-file <traces.jsonl.gz> \
  --sql "<sql>"
```

`--source hitrace` 继续使用现有形式：

```bash
kat-rs query --source hitrace --file <trace.hitrace> --sql "<sql>"
```

Langfuse source 注册两张原始表：

| 表名 | 来源 | 说明 |
| --- | --- | --- |
| `langfuse_observations` | `observations/*.jsonl.gz` 中的单个文件 | legacy observation-level 原始字段 |
| `langfuse_traces` | `traces/*.jsonl.gz` 中的单个文件 | legacy trace-level 原始字段 |

用户通过 SQL 自己 join：

```sql
select o.trace_id, t.name as trace_name, count(*) as observation_count
from langfuse_observations o
join langfuse_traces t on o.trace_id = t.id
group by o.trace_id, t.name
```

## 架构

实现复用现有 `TraceDatasource` 和 `query_json(sql)`，同时新增一个很薄的 Langfuse format 模块，与当前 `hitrace.rs` 的“输入格式适配器”角色保持一致。`kat-rs` 不新增 Langfuse 专用 JSON parser，不定义完整 Langfuse Rust struct。

```text
kat-rs-cli
  -> parse query args
  -> TraceDatasource::from_langfuse_legacy(observations_path, traces_path)
  -> langfuse::legacy_json_tables(observations_path, traces_path)
  -> DataFusion SessionContext
  -> register_json("langfuse_observations", observations_path, JsonReadOptions)
  -> register_json("langfuse_traces", traces_path, JsonReadOptions)
  -> query_json(sql)
```

新增文件：

| 文件 | 职责 |
| --- | --- |
| `crates/kat-rs-datasource/src/formats/langfuse/mod.rs` | Langfuse legacy format 的表名和文件到表的映射 |
| `crates/kat-rs-datasource/src/query.rs` | 创建 DataFusion context，注册 format 模块返回的表，执行 SQL |

当前代码已经把输入格式适配器放在 `formats/` 下，因此 Langfuse 也放入 `formats/langfuse`。本次仍不扩大到 catalog、sink 或 domain 边界重构。

DataFusion 已在项目依赖图中，用于 SQL 查询。Langfuse JSONL/GZ 读取应复用 DataFusion/Arrow JSON datasource：

- 使用 `JsonReadOptions`。
- 设置 `file_extension(".jsonl.gz")`。
- 设置 gzip compression。
- 若 DataFusion gzip 读取需要 feature gate，则在 workspace 的 `datafusion` 依赖上启用 `compression` feature。

这样项目代码只保留表名、CLI 参数和错误上下文等 glue code。

## 参数规则

`QueryArgs` 可以继续作为单个 query 子命令参数结构，但需要表达 source-specific 参数：

| source | 必需参数 | 无效或缺失参数行为 |
| --- | --- | --- |
| `hitrace` | `--file`、`--sql` | 缺少 `--file` 时由 CLI 报错 |
| `langfuse` | `--observations-file`、`--traces-file`、`--sql` | 缺少任一 Langfuse 文件参数时报错 |

第一版可以保留 `--file` 字段在 help 中可见；但运行时必须避免 `--source langfuse --file ...` 被误认为有效输入。若实现成本不高，优先用 clap 的 `required_if_eq` / `conflicts_with` 约束表达；否则在 `run_query` 中返回明确错误。

## 字段与类型策略

第一版不手写 schema。字段和类型由 DataFusion JSON reader 从输入文件推断。

理由：

1. 这是最小可交付切片。
2. legacy export 字段较多，手写 schema 容易遗漏，也会过早固化字段契约。
3. 用户当前需求是保留完整原始字符串并通过 SQL 查询，不需要字段规范化。

风险：

1. 如果某些字段在样本行中全是 null 或跨行类型不一致，DataFusion schema inference 可能不稳定。
2. 如果真实 Langfuse export 出现嵌套 object/array，SQL 使用方式可能依赖 DataFusion 对 nested JSON 的支持。

第一版只通过 fixture 覆盖已确认的核心字段：`id`、`trace_id`、`type`、`name`、`start_time`、`end_time`、`input`、`output`、`user_id`、`session_id`。后续如需要稳定字段契约，再增加显式 schema。

## 错误处理

1. 文件不存在、打不开、不是 gzip、不是合法 JSONL、DataFusion schema 推断失败：命令失败，返回非 0 退出码。
2. SQL 引用不存在的表或字段：沿用 DataFusion 错误。
3. observations 与 traces 中的 id 不匹配：不预检，SQL join 自然返回匹配结果。
4. `input` / `output` 太大：不特殊处理；用户通过 SQL 自己选择字段。
5. 不提供 `langfuse_parse_errors`，因为 `kat-rs` 不是清洗型 ETL；输入结构错误应直接失败。
6. 错误输出不得包含凭据、对象存储 secret、Langfuse API key 等敏感信息。

## 测试计划

1. datasource 测试构造最小 `observations.jsonl.gz` 和 `traces.jsonl.gz`，验证能 join：

   ```sql
   select o.id, t.name
   from langfuse_observations o
   join langfuse_traces t on o.trace_id = t.id
   ```

2. datasource 测试验证 `input` / `output` 返回完整字符串，不被截断。
3. CLI 测试验证 `--source langfuse --observations-file ... --traces-file ... --sql ...` 输出 JSON rows。
4. CLI 或 datasource 错误测试覆盖坏 gzip 或坏 JSONL，确认失败且不 panic。
5. 现有 hitrace datasource 和 CLI 测试继续通过，证明 `--source hitrace --file ...` 行为未变。

最终验证命令：

```bash
cargo test --workspace
```

## 验收标准

1. `kat-rs query --source langfuse --observations-file <file> --traces-file <file> --sql <sql>` 可以查询两张 Langfuse legacy 原始表。
2. 可以用 SQL join `langfuse_observations.trace_id = langfuse_traces.id`。
3. `input` / `output` 完整保留为查询结果中的字符串。
4. 无 `langfuse_parse_errors`、timeline、quality、rollup 等派生表。
5. 坏输入直接失败，不 panic。
6. `cargo test --workspace` 通过。
