# PostgreSQL and Parquet Fusion example PACK

这个 External PACK 展示 PACK 自有 Datasource Provider 的两条完整用户链：

- `query-observations` 直接把一次 PostgreSQL 查询形成的 eager `ds.Table` 作为
  `main` Output；
- `fuse-observations` 使用同一个 `PostgreSQLProvider` 依次查询同一 service 下的
  telemetry、control 两个 Database，再把两个远端 Table 与本地 Parquet Table
  显式交给 `ctx.sql(tables=...)` 融合。

Provider 都是 `datasources/` 下由 PACK 拥有的普通 Python 类。KAT 不发现、构造或
包装 Provider，也不自动注册来源查询结果。只有 Workflow 返回的 Table 会发布为
Run Output；仅作为融合输入的 Table 不产生 Output 文件。

## 数据与资源边界

`PostgreSQLProvider` 使用 libpq service 和每次 query 显式给出的 Database 建连。
每次 query 都创建独立连接，在服务端开启只读事务，通过 ADBC 绑定位置参数并完整
读取结果。Provider 在返回 eager `ds.Table` 前已经关闭 reader、cursor，回滚事务并
关闭 query-local connection；后续读取、融合和 Output 发布不会再次执行远端 SQL。

ADBC 结果在进入 `ds.from_arrow()` 前遵守以下规范化边界：

- PostgreSQL `NUMERIC` 必须能够无舍入地表示为固定
  `decimal128(38, 18)`；超出 precision、range 或需要舍入时立即失败；
- 有绝对时间语义的 PostgreSQL timestamp 规范为
  `timestamp(ns, tz="UTC")`；
- `TIMESTAMP WITHOUT TIME ZONE` 没有绝对时间语义，Provider 不猜测本地时区或
  UTC。Workflow 必须按领域规则在来源 SQL 中显式转换，否则查询失败。

连接 URI、用户和凭据不是 Workflow 参数。源码不会读取或记录 service file、
password file 的内容。

## 目录

- `datasources/postgresql.py`：普通 `PostgreSQLProvider`，负责 ADBC 查询、只读事务、
  类型规范化、资源关闭与错误脱敏；
- `datasources/parquet.py`：用 `ds.Schema` 和 `ds.open()` 显式打开
  `sched_switch.parquet` 的薄 Provider；
- `workflows/query_observations.py`：直接返回 PostgreSQL Table；
- `workflows/fuse_observations.py`：在 PostgreSQL 内完成 Join、Filter、Aggregate，
  再与本地调度区间显式融合；
- `tests/`：连接外部提供的真实 PostgreSQL fixture，验证参数、类型、只读事务、
  单 command、资源关闭、单源 Output 和完整融合。

## PostgreSQL 配置

生产形态只要求 libpq 能解析一个最小权限只读 service。service file 管理地址、
用户、TLS 等连接策略，password file 或外部凭据机制管理秘密；Database 由
Workflow 参数显式选择，并覆盖 service 中可能存在的默认 Database。测试 service
还必须设置正数 `connect_timeout`，避免不可达 fixture 无限等待。

```bash
export PGSERVICEFILE=/absolute/path/to/pg_service.conf
export PGPASSFILE=/absolute/path/to/pgpass
```

`kat inspect` 只加载 PACK 声明，无需数据库可用。

## 验证

PACK tests 需要测试环境事先提供同一 service 上的两个 Database：

- telemetry Database：
  - `thread_registry(thread_id BIGINT PRIMARY KEY, process_id BIGINT NOT NULL)`；
  - `observation(thread_id BIGINT NOT NULL, observed_at BIGINT NOT NULL,
    cpu_usage DOUBLE PRECISION NOT NULL)`；
  - `write_guard(value INTEGER NOT NULL)`，供只读事务负向测试使用。
- control Database：
  - `process_registry(process_id BIGINT PRIMARY KEY, process_name TEXT NOT NULL)`。

只读 service 身份必须只具备上述表的 `SELECT` 权限且没有高权限角色属性。另一个
仅用于测试的 writer service 身份需要拥有 fixture 写权限、schema DDL 权限和读取
`pg_stat_activity` 的权限；Provider 仍会把它约束在服务端只读事务中。测试不会
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

## 单源运行

`query-observations` 直接返回 PostgreSQL Table，不启动本地 Fusion Session，也不会
第二次执行来源 SQL：

```bash
kat run \
  --pack postgresql-parquet-fusion \
  --workflow query-observations \
  --pack-dir ./examples/packs/postgresql-parquet-fusion \
  -- \
  --service readonly_service \
  --database telemetry \
  --start-ns 100 \
  --end-ns 220

kat query \
  --run <run-id> \
  --sql "SELECT * FROM output.main ORDER BY observed_at, thread_id"
```

## PostgreSQL 与 Parquet 融合

`trace_root` 是包含 `sched_switch.parquet` 的目录。该表必须具有 nullable-compatible
int64 列 `cpu`、`next_thread_id`、`timestamp`，并保证 `(cpu, timestamp)` 唯一。

```bash
kat run \
  --pack postgresql-parquet-fusion \
  --workflow fuse-observations \
  --pack-dir ./examples/packs/postgresql-parquet-fusion \
  -- \
  --service readonly_service \
  --telemetry-database telemetry \
  --control-database control \
  --trace-root /absolute/path/to/trace \
  --start-ns 100 \
  --end-ns 220
```

Workflow 严格按 Python 调用顺序完成 telemetry query、control query、本地 Parquet
query，最后才执行只读本地融合。最终只发布 `main`；`telemetry`、`processes` 和
`switches` 都是 call-local 融合输入。

```bash
kat query \
  --run <run-id> \
  --sql "SELECT * FROM output.main ORDER BY observed_at, thread_id"
```
