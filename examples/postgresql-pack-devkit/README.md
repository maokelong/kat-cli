# PostgreSQL External PACK 离线开发包

这个目录是最终 Windows x86-64 devkit 的源模板。组装后的 devkit 可以搬到网络受限的 Windows 环境，在不安装系统 Python、uv、pip、Rust、MSVC 或 Docker 的情况下编辑并验证 PostgreSQL External PACK。

devkit 只证明 External PACK 到 PostgreSQL、Run Output 和 Output Query 的开发闭环。它不是 PostgreSQL Datasource，不提供 `kat import postgresql`，也不把远程数据库伪装成 Dataset。

## 组装后的目录

```text
<devkit>/
├── README.md
├── ENVIRONMENT.md
├── DEVKIT-MANIFEST.json
├── SHA256SUMS
├── offline-locks/
│   ├── postgresql-pack-devkit-inputs.json
│   └── requirements-postgresql-windows.lock.txt
├── scripts/
│   ├── Verify-Devkit.ps1
│   └── Invoke-LiveValidation.ps1
├── queries/
│   └── smoke.sql
├── skill/
│   ├── SKILL.md
│   └── scripts/targets/windows-x86_64/
│       ├── kat.exe
│       └── python/python.exe
├── pack/
│   ├── pack.toml
│   ├── workflows/
│   └── tests/
└── data-home/
```

`skill/` 包含已经加入 Psycopg 的 Windows Platform Payload。`pack/` 是可编辑的 External PACK 源码。`data-home/` 保存 Operation log、PACK test report、Run Manifest 和 Run Output。

## 使用顺序

### 1. 解压后先验证原始交付物

先从受信渠道取得 ZIP 的外置 `.sha256`，用 `Get-FileHash -Algorithm SHA256` 比较 ZIP；外置 hash 应与 ZIP 分开传递或发布。ZIP 内的 `SHA256SUMS` 能发现解压后的损坏，但如果攻击者可以同时替换文件和清单，它本身不提供来源认证。

解压后，在修改 `pack/` 前，从新的短命 PowerShell 7.3+ 进程运行：

```powershell
pwsh -NoProfile -File .\scripts\Verify-Devkit.ps1
```

验证脚本先检查根级 `SHA256SUMS`，随后检查私有 Host 的 CPython、Psycopg binary 和 libpq，最后在清空全部继承 `PG*` 的环境中执行 PACK inspection。它不会请求数据库密码，也不会连接数据库。

`SHA256SUMS` 描述组装时的原始文件。修改 PACK 后 hash 变化是预期行为；需要重新确认原始交付物时，请重新解压一份或与受控准备机的源码比较，不要把修改后的 hash 冒充原始清单。

### 2. 编辑 PACK 或 SQL

- Workflow 入口位于 `pack/workflows/`。
- PACK test 位于 `pack/tests/`。
- 默认真实验证 SQL 位于 `queries/smoke.sql`，也可以用 `-SqlFile` 指定另一个 UTF-8 文件。
- Workflow 的 `required_tables=[]`，因此不创建 Dataset，也不运行 `kat import`。
- 远程 SQL 原样交给 PostgreSQL。必须自行保证它只返回一个小 rowset；KAT 不解析、限制、改写或自动添加 `LIMIT`。

SQL 是 Workflow input，会进入 Operation log 和 Run Manifest。不要在 SQL 中写入密码、token、DSN 或其他秘密。

### 3. 对真实 PostgreSQL 运行完整闭环

默认 TLS 模式是 `verify-full`，因此需要数据库服务端证书的 CA PEM 文件。先把 CA 的 SHA-256 与数据库管理员通过另一条可信渠道提供的 digest 比较；CA 和 digest 不能只从同一压缩包、目录或消息取得：

```powershell
$actualCaHash = (Get-FileHash -Algorithm SHA256 -LiteralPath 'D:\approved-ca\postgresql-root.crt').Hash.ToLowerInvariant()
if ($actualCaHash -ne '<数据库管理员独立提供的 SHA-256>') {
  throw 'PostgreSQL CA SHA-256 mismatch'
}
```

随后运行：

```powershell
pwsh -NoProfile -File .\scripts\Invoke-LiveValidation.ps1 `
  -DatabaseHost '<证书中的数据库 DNS 名或 IP>' `
  -DatabasePort 5432 `
  -DatabaseName '<数据库名>' `
  -DatabaseUser '<只读账号>' `
  -CaCertificate 'D:\approved-ca\postgresql-root.crt'
```

如果受限网络没有 DNS，但证书使用 DNS 名，分别传入证书身份和实际连接 IP：

```powershell
pwsh -NoProfile -File .\scripts\Invoke-LiveValidation.ps1 `
  -DatabaseHost '<证书中的数据库 DNS 名>' `
  -HostAddress '<数据库 IP>' `
  -DatabaseName '<数据库名>' `
  -DatabaseUser '<只读账号>' `
  -CaCertificate 'D:\approved-ca\postgresql-root.crt'
```

脚本严格按以下顺序执行：

1. 清除继承的全部 `PG*`；
2. 在没有数据库凭据时执行并校验 `kat inspect` 的 JSON Response；
3. 通过交互式 SecureString prompt 输入密码；
4. 仅在当前短命进程中设置白名单 `PG*`；
5. 执行真实 `kat test`；
6. 不传 Dataset 执行 `kat run`；
7. 从成功 Response 取得 Run ID 和 `postgresql_result` 元数据；
8. 立即清除全部 `PG*`，释放 BSTR 与 SecureString；
9. 在无数据库凭据状态执行 `kat query` 读取 `output.postgresql_result`；
10. 在 `finally` 中再次清理任何失败路径残留。

脚本把进度写到 stderr，stdout 只写一个 `status=success` 的 JSON 摘要。两份脚本都固定 `$PSNativeCommandArgumentPassing='Standard'`，确保 SQL 中的双引号按原参数传给 KAT。查询结果可能包含敏感业务数据，stdout capture 和 `data-home/` 都应按实际数据等级保护。

只有数据库所有者已经明确接受无 TLS 的本地隔离实例，才可显式运行以下开发例外；不要把它用于真实远程数据库：

```powershell
pwsh -NoProfile -File .\scripts\Invoke-LiveValidation.ps1 `
  -DatabaseHost '127.0.0.1' `
  -DatabaseName '<本地测试数据库名>' `
  -DatabaseUser '<本地只读测试账号>' `
  -SslMode disable
```

## 修改 smoke SQL

`queries/smoke.sql` 只读取当前 database、user、server version、事务只读状态以及当前连接的 TLS 状态。开发其他查询时建议另建 SQL 文件：

```powershell
pwsh -NoProfile -File .\scripts\Invoke-LiveValidation.ps1 `
  -DatabaseHost '<数据库证书身份>' `
  -DatabaseName '<数据库名>' `
  -DatabaseUser '<只读账号>' `
  -CaCertificate 'D:\approved-ca\postgresql-root.crt' `
  -SqlFile 'D:\approved-sql\my-query.sql'
```

PACK 会把所有非 NULL 值投影为 UTF-8 string，保留 NULL，并依靠 cursor metadata 为零行结果保留列结构。这是开发包的简单合同，不是完整的 PostgreSQL-to-Arrow 类型映射。

环境与安全配置见 [ENVIRONMENT.md](ENVIRONMENT.md)。
