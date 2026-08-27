# Local Parquet Fusion example PACK

这个 External PACK 展示本地 Datasource 的最小完整形态：PACK helper 接收显式
`Mapping[str, Path]`，把这些 Parquet 文件或单表分片目录注册到自己的私有
DataFusion Session；`Provider.query()` 将来源内 SQL 结果本地化并自动注册，随后
Workflow 用 `ctx.sql()` 融合来自两个 Provider 的表。

示例刻意不扫描目录发现表。只有传给 `parquet.provider(..., tables=...)` 的名称可被
来源 SQL 看到；文件路径、SQL 方言、参数转换和私有 Session 生命周期都属于 PACK
Datasource。KAT Runtime 只管理 query 结果名称、Parquet backing、融合 catalog 与
Output 发布。来源查询通过 `RecordBatchReader` 逐 batch 交付，不先 `collect()` 成
整张 Arrow Table。

## 目录

- `helpers/datasources/parquet.py`：PACK 自己实现的本地 Parquet executor。
- `workflows/fuse_local_parquet.py`：先在一个来源内 Join/Filter，再融合另一个
  Provider 的结果。
- `tests/test_fuse_local_parquet.py`：生成临时 Parquet fixture，覆盖具名参数、显式
  可见性、单文件与分片目录、跨 Provider 融合。

## 验证

在完整 KAT Skill deployment 中，从仓库根目录运行：

```bash
kat inspect --pack local-parquet-fusion \
  --pack-dir ./examples/packs/local-parquet-fusion

kat test --pack-dir ./examples/packs/local-parquet-fusion
```

生产运行需要三份已有 Parquet 输入；`labels` 可以是一个同 Schema 分片目录：

```bash
kat run \
  --pack local-parquet-fusion \
  --workflow fuse-local-parquet \
  --pack-dir ./examples/packs/local-parquet-fusion \
  -- \
  --events-path /absolute/path/events.parquet \
  --labels-path /absolute/path/labels \
  --owners-path /absolute/path/owners.parquet \
  --minimum-score 10
```

Workflow 返回单个 `main` Output。它包含 `event_id`、`label`、`owner_name` 和
`score`；结果按 `event_id` 稳定排序。
