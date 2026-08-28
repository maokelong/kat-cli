# PostgreSQL and Parquet Fusion example PACK

这个 External PACK 展示一个 Workflow 如何依次查询同一 PostgreSQL 服务上的两个
Database，把每个远端结果先本地化，再与本地 Parquet 查询结果通过 `ctx.sql()`
融合。PostgreSQL 与 Parquet 都是 PACK 自己实现的 Datasource；KAT Runtime 只管理
query result backing、自动注册、融合和最终 Output 发布。

PostgreSQL executor 使用 libpq service 名和显式 Database 建连。每次 query 都创建
独立连接，在服务端开启只读事务，通过 ADBC prepare/bind/result-stream 路径返回
`RecordBatchReader`；Runtime 完整消费后，executor 关闭 reader、statement，回滚
事务并关闭连接。连接 URI、用户和凭据不是 Workflow 参数。

## 目录

- `helpers/datasources/postgresql.py`：PACK 自己实现的 PostgreSQL ADBC executor。
- `helpers/datasources/parquet.py`：显式映射本地 Parquet 表的私有 DataFusion
  executor。
- `workflows/fuse_observations.py`：先本地化两个 Database 和本地调度区间，再融合
  成唯一的 `main` Output。
- `tests/`：连接外部提供的真实 PostgreSQL fixture，验证参数、类型、只读事务、
  单 command、资源关闭和完整融合。

## PostgreSQL 配置

生产形态只要求 libpq 能解析一个最小权限只读 service。service file 管理地址、
用户、TLS 等连接策略，password file 或外部凭据机制管理秘密；Database 由
Workflow 参数显式选择，并覆盖 service 中可能存在的默认 Database。
测试 service 还必须设置正数 `connect_timeout`，避免不可达 fixture 无限等待。

```bash
export PGSERVICEFILE=/absolute/path/to/pg_service.conf
export PGPASSFILE=/absolute/path/to/pgpass
```

示例源码不读取或记录这些文件，也不包含固定服务地址或凭据。`kat inspect` 只加载
PACK 声明，无需数据库可用。

## 验证

PACK tests 需要测试环境事先提供同一服务上的两个 Database：

- telemetry Database：
  - `thread_registry(thread_id BIGINT PRIMARY KEY, process_id BIGINT NOT NULL)`；
  - `observation(thread_id BIGINT NOT NULL, observed_at BIGINT NOT NULL,
    cpu_usage DOUBLE PRECISION NOT NULL)`；
  - `write_guard(value INTEGER NOT NULL)`，供只读事务负向测试使用。
- control Database：
  - `process_registry(process_id BIGINT PRIMARY KEY, process_name TEXT NOT NULL)`。

只读 service 身份必须只具备上述表的 `SELECT` 权限且没有高权限角色属性。另一个
仅用于测试的 writer service 身份需要拥有 fixture 写权限、schema DDL 权限和读取
`pg_stat_activity` 的权限；executor 仍会把它约束在服务端只读事务中。测试不会
创建、等待或销毁数据库服务。

融合测试使用以下确定数据：

- `thread_registry`：`101→10`、`102→20`、`103→30`；
- `process_registry`：`10→renderer`、`20→system-server`；
- observation 主结果：`(101,100,0.25)`、`(102,150,0.5)`、
  `(102,200,0.75)`；fixture 还包含窗口边界、缺失 thread/process 和未知调度区间
  的行，用于证明 inner join 与半开窗口语义；
- `write_guard` 初始包含一行。

将测试环境的 service 和 Database 名通过非敏感环境变量交给 PACK tests：

```bash
export KAT_TEST_POSTGRES_READONLY_PROFILE=readonly_service
export KAT_TEST_POSTGRES_WRITER_PROFILE=writer_fixture_service
export KAT_TEST_POSTGRES_TELEMETRY_DATABASE=telemetry
export KAT_TEST_POSTGRES_CONTROL_DATABASE=control
export KAT_TEST_POSTGRES_SECRET_SENTINEL='<测试 password file 中的实际密码>'

kat inspect \
  --pack postgresql-parquet-fusion \
  --pack-dir ./examples/packs/postgresql-parquet-fusion

kat test --pack-dir ./examples/packs/postgresql-parquet-fusion
```

两个 Database 名必须不同，secret sentinel 必须与 `PGPASSFILE` 的测试密码一致；
它只用于验证响应、日志与产物没有泄漏凭据。缺少或误配这些测试配置会直接失败，
不会静默跳过真实 PostgreSQL 合同。

## 运行

生产运行只使用只读 service。`trace_root` 是包含 `sched_switch.parquet` 的目录；该表
至少包含 int64 类型的 `cpu`、`next_thread_id`、`timestamp`，并保证
`(cpu, timestamp)` 唯一。

```bash
kat run \
  --pack postgresql-parquet-fusion \
  --workflow fuse-observations \
  --pack-dir ./examples/packs/postgresql-parquet-fusion \
  -- \
  --profile readonly_service \
  --telemetry-database telemetry \
  --control-database control \
  --trace-root /absolute/path/to/trace \
  --start-ns 100 \
  --end-ns 220
```

Workflow 严格按 Python 调用顺序完成 telemetry query、control query、本地 Parquet
query，最后才执行只读本地融合。最终只发布 `main`；`telemetry`、`processes` 和
`switches` 都只是当前 Run 内的本地融合输入。

```bash
kat query \
  --run <run-id> \
  --sql "SELECT * FROM output.main ORDER BY observed_at, thread_id"
```
