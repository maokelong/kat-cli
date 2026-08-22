---
status: accepted
---

# PostgreSQL common 使用进程级 libpq 环境

公共 PostgreSQL common 第一版不建立 Connection Profile，也不接收 connection 名称或连接参数；它通过不带显式连接参数的 Psycopg 调用读取 Workflow Host 继承的标准 `PGHOST`、可选 `PGHOSTADDR`、`PGPORT`、`PGDATABASE`、`PGUSER`、`PGPASSWORD`、`PGSSLMODE`、可选 `PGSSLROOTCERT`、`PGCONNECT_TIMEOUT` 与 `PGCLIENTENCODING`。连接配置和秘密不进入 Source Knowledge Package、PACK、Workflow arguments 或 Run Manifest。

一个短命 Workflow Host 进程因此只有一套有效 PostgreSQL 连接环境；多个 Source Knowledge Package 可以描述该实例中的不同逻辑模型，但同一 Workflow 第一版不能同时连接两个不同 PostgreSQL 实例。切换实例通过在另一次 `kat run` 前设置另一套进程环境完成，未来出现真实多连接需求时再引入显式配置模型。
