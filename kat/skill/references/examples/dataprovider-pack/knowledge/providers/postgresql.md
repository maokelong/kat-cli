# PostgreSQL Provider 作者知识

`PostgreSQLProvider(service=...)` 使用一个 libpq service 名称保存服务级连接配置；每次
`query(sql, database=..., params=...)` 再显式选择 Database。因此同一 Workflow 可以用
同一个 Provider 实例依次查询同一服务上的多个 Database，不需要为 Database 建立静态绑定。
用户名、密码和连接 URI 属于 libpq 配置，不应成为 Workflow 参数或写入日志。

`query` 把 SQL 完整提交给 PostgreSQL。`params` 只接受一组位置参数，对应 SQL 中的
`$1`、`$2` 等占位符；表名、列名和 SQL 片段不能作为参数绑定。Provider 在只读事务中
完整读取 ADBC 结果，关闭 reader、cursor 和 connection 后返回 eager `dp.Table`。后续
反复读取、写 Parquet 或参与 DataFusion 融合不会重新执行远端 SQL。

本示例 Workflow 使用以下来源关系：

- telemetry Database：`observation(thread_id, observed_at, cpu_usage)`；
  `thread_registry(thread_id, process_id)`，两表按 `thread_id` 关联。
- control Database：`process_registry(process_id, process_name)`。
- 本地 Parquet：`thread_placement(thread_id, cpu)`。

融合关系是 `observation.thread_id = thread_registry.thread_id =
thread_placement.thread_id`，以及 `thread_registry.process_id =
process_registry.process_id`。`observed_at` 是来源时钟值；Workflow 显式增加
`clock_domain` 后才能表达时钟语义，不能把不同 domain 的裸整数直接比较。

ADBC `NUMERIC` 会无舍入地规范为 `decimal128(38, 18)`，超出该合同会失败；带时区的
timestamp 规范为 `timestamp(ns, tz="UTC")`。`TIMESTAMP WITHOUT TIME ZONE` 没有
足够的绝对时间语义，来源 SQL 必须先按领域规则显式转换。

扩展此 Provider 时，应继续把远端可执行的过滤、聚合和同库 JOIN 留在 SQL 中；只有
跨 Database 或与本地表融合时，才把各次 eager 查询结果交给 `dp.DataFusionProvider`。
