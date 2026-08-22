# 受限网络环境配置

## 目标机要求

- Windows 10/11 x86-64。
- PowerShell 7.3 或更高版本，可使用 `pwsh -NoProfile -File` 启动短命进程。脚本拒绝更低版本，并固定 `$PSNativeCommandArgumentPassing='Standard'`，避免 SQL 参数中的双引号被 Windows Legacy 规则删除。
- devkit 目录可读，`data-home/` 已存在并可写。
- 只允许到目标 PostgreSQL 地址与端口的必要网络连接；运行时不需要互联网、Python 包索引或 HTTP 代理。
- PostgreSQL 管理员已经提供专用只读账号，并明确授权可执行的 schema、table、view 和 function。

devkit 自带完整 Windows Platform Payload 和私有 Python Host。不要把私有 `python.exe` 加入 `PATH`，也不要用系统 pip 修改它。

## 凭据生命周期

`Invoke-LiveValidation.ps1` 没有 password 参数。它先清除继承的全部 `PG*`，在无凭据状态完成 PACK inspection，之后才通过 SecureString prompt 输入密码。密码只被转换到当前进程的 `PGPASSWORD`，供 `kat.exe` 和它启动的私有 Python Host 继承。远程 Run 子进程一返回，脚本就立即清除全部 `PG*` 并释放 BSTR/SecureString，再在无数据库凭据状态执行本地 Output Query；`finally` 仍覆盖所有失败路径。

这能避免把密码写入普通命令历史或文件，但环境变量不是 secret store：密码在 KAT 和 Python 子进程运行期间仍以明文存在于进程环境，具有同用户调试权限或管理员权限的进程可能读取它。请只在受控机器上使用专用、可轮换的只读凭据，并保持会话短命。

禁止以下做法：

- 在 PACK、SQL、PowerShell 脚本、`.env` 或配置文件中写密码；
- 使用 `setx PGPASSWORD ...` 或修改 PowerShell Profile；
- 把密码放进 DSN、Workflow `--sql` 或任意命令行参数；
- 在有真实密码的会话中输出整个环境、启用调试环境转储或运行未知 PACK；
- 用真实凭据执行未经 review 的 `kat test`。PACK 测试与生产 Workflow 一样是受信任本地代码，不是沙箱。

## 脚本设置的环境变量

| 变量 | 来源与含义 |
|---|---|
| `KAT_DATA_HOME` | 固定为 devkit 的绝对 `data-home/` 路径 |
| `PSYCOPG_IMPL` | 固定为 `binary`，禁止静默回退到其他实现 |
| `PGHOST` | `-DatabaseHost`；在 `verify-full` 下也是证书身份 |
| `PGHOSTADDR` | 可选 `-HostAddress`；绕过 DNS，仅决定连接 IP |
| `PGPORT` | `-DatabasePort`，默认 5432 |
| `PGDATABASE` | `-DatabaseName` |
| `PGUSER` | `-DatabaseUser` |
| `PGPASSWORD` | SecureString prompt，仅当前进程 |
| `PGSSLMODE` | `-SslMode`，默认 `verify-full` |
| `PGSSLROOTCERT` | `-CaCertificate` 的绝对路径 |
| `PGSSLCERTMODE` | 固定 `disable`；本 devkit 不使用 mTLS 客户端证书 |
| `PGSSLMINPROTOCOLVERSION` | 固定 `TLSv1.2` |
| `PGCONNECT_TIMEOUT` | `-ConnectTimeoutSeconds`，默认 10 秒 |
| `PGAPPNAME` | 固定 `kat-postgresql-pack-devkit` |
| `PGCLIENTENCODING` | 固定 `UTF8` |

脚本不继承 `PGSERVICE`、`PGSERVICEFILE`、`PGPASSFILE`、`PGOPTIONS`、`PGSSLKEY` 或其他未批准的 `PG*`，避免目标机上的旧配置改变连接与权限语义。

## TLS 与 CA

真实远程数据库默认使用：

```text
PGSSLMODE=verify-full
PGSSLROOTCERT=<明确提供的 CA PEM 绝对路径>
```

`verify-full` 同时校验 CA 信任链和数据库身份：

- 用 DNS 名连接时，`-DatabaseHost` 必须匹配服务端证书；
- 直接用 IP 时，证书必须包含匹配的 IP SAN；
- 无 DNS 但证书使用 DNS 名时，`-DatabaseHost` 传证书 DNS 名，`-HostAddress` 传实际 IP。

CA 证书通常不是秘密，但它决定信任根，必须通过受控渠道传输。脚本只验证 CA 路径存在，不会自动认证其来源；运行前必须用 `Get-FileHash -Algorithm SHA256` 计算 CA digest，并与数据库管理员通过另一条可信渠道提供的 SHA-256 比较。CA 与 digest 同目录、同压缩包或同一消息传输不构成独立验证。不要为了临时连通而静默降级 TLS。只有在数据库所有者明确接受风险时才显式选择 `verify-ca`、`require` 或 `disable`，并把该例外记录到验证证据中。

本 devkit 面向账号密码认证，不支持 mTLS 客户端私钥。如果真实环境要求 mTLS，需要另建受审设计；不要把私钥复制进 PACK 或 devkit。

## KAT Data Home

SQL 会进入 Operation log 和 Run Manifest；PostgreSQL 返回的小 rowset 会成为 Run Output；`kat query` 结果会显示在终端。因此 `data-home/` 不是普通缓存目录，而是可能包含敏感业务数据的持久目录。

目标机应当：

- 把 devkit 放在已存在、ACL 仅授权用户可访问的目录；
- 优先使用组织批准的磁盘加密；
- 避免桌面同步盘、公共共享盘和自动上传目录；
- 按数据保留策略处理 Run、日志、测试报告与终端 capture；
- 不把 `data-home/`、真实 CA 路径、连接信息或运行输出提交回源码仓库。

即使设置了 `KAT_DATA_HOME`，损坏的默认 KAT `config.json` 仍可能使 CLI 在选择 Data Home 时失败。应修复该配置文件，而不是清空环境或绕过错误重试。

## 数据库授权边界

KAT 不解析、限制、重写或审计远程 SQL。只读账号是唯一 SQL 授权边界，并不天然阻止被授予的副作用函数。数据库管理员应按真实需要最小化：

- database CONNECT；
- schema USAGE；
- 目标 table/view SELECT；
- function EXECUTE；
- CREATE、TEMP、DDL、DML 与管理权限。

`queries/smoke.sql` 返回的 `transaction_read_only` 是连接事实，不替代权限审计。开发者仍需确认所用账号与目标数据库符合组织的只读授权要求。

## 完整性与版本

受限环境应先把 ZIP 的 SHA-256 与独立可信渠道提供的外置 `.sha256` 比较，再解压。ZIP 内的 `SHA256SUMS` 只负责解压后的逐文件完整性；能够同时替换文件和清单的攻击者不会被内置清单识别。

`Verify-Devkit.ps1` 在运行任何 devkit 可执行文件前检查 `SHA256SUMS`。清单中的路径必须是 devkit 内相对路径，重复、越界、缺失、错误 hash 或未列出的交付文件都会失败；运行后产生的 `data-home/` 内容除外。

随后脚本验证：

- CPython 3.14.6；
- Psycopg 3.3.4；
- `pq.__impl__ == "binary"`；
- bundled libpq 18.3；
- PACK inspection 的 `status=success`；
- `query-postgresql` 的 `required_tables=[]`；
- `sql` 是必填 string 参数。

任何失败都应停止，不要在受限目标机在线下载、求解或替换 wheel。修改 `pack/` 后原始清单不再匹配是预期行为，不能通过随手重写清单把修改伪装成原始交付。
