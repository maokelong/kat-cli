# Local Parquet Fusion example PACK

这个 External PACK 展示本地 Datasource 的最小完整形态：PACK 在顶层
`datasources/` 中定义普通 `LocalParquetProvider`，用 `ds.Schema` 声明表合同，再把
显式 `Mapping[str, Path]` 交给 `ds.open()`。Workflow 直接构造并调用两个 Provider，
最后把查询得到的 eager `ds.Table` 显式交给 `ctx.sql(tables=...)` 融合。

示例刻意不从 PACK 根目录扫描或注册 Provider。只有 Schema 和 `tables` mapping 中
显式声明的名称可被来源 SQL 看到；单个路径既可以是一份 Parquet 文件，也可以是只
包含该表分片的目录。`Catalog.query()` 完成来源内 SQL 后返回可重复读取的 Table；
只有 Workflow 返回的最终 Table 会由 Runtime 发布，融合输入不会自动成为 Output。

## 目录

- `datasources/parquet.py`：PACK 自己定义的 Schema 与普通 Provider 类。
- `workflows/fuse_local_parquet.py`：显式调用两个 Provider，先在来源内
  Join/Filter，再用 `ctx.sql(tables=...)` 融合查询结果。
- `tests/test_fuse_local_parquet.py`：生成临时 Parquet fixture，覆盖具名参数、显式
  可见性、Table 重复读取、单文件与分片目录、跨 Provider 融合。

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

使用 `kat run` 返回的 `run_id` 可以继续查询已发布结果：

```bash
kat query --run <run-id> --sql \
  "SELECT event_id, label, owner_name, score FROM output.main ORDER BY event_id"
```
