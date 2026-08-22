# PostgreSQL External PACK 开发与离线环境

本说明用于在联网或受控 Windows 准备机上组装一个完整开发包，再把单个 ZIP
搬到网络受限的 Windows 环境，连接真实 PostgreSQL 开发、测试和运行 External
PACK。

这条链路不实现 PostgreSQL Datasource，也不使用 `kat import`。`kat import` 的输出
是 KAT Dataset；本 PACK 的 Workflow 声明 `required_tables=[]`，在 `kat run` 中直接
使用 Psycopg 读取远程数据库，再把一个小型结果集发布成 Run Output。`kat query`
查询的是这个已发布的 `output.postgresql_result`，不是远程 PostgreSQL。

## 交付内容与边界

仓库包含三部分可提交源码：

- `examples/packs/postgresql-query/`：可编辑的 PACK、真实数据库测试和 Workflow；
- `examples/postgresql-pack-devkit/`：最终开发包的中文说明、PowerShell 入口和示例 SQL；
- `build/build_postgresql_pack_devkit.py`：从完整 Windows Payload 组装开发包的脚本。

生成后的开发包包含完整 Windows Platform Payload、已安装 Psycopg 的私有 Python Host、
PACK 源码、运行脚本、输入锁和全文件 SHA-256 清单。目标机不需要安装系统 Python、
uv、pip、Rust、MSVC 或 Docker，运行时也不会访问互联网。

开发包只支持 Windows 10/11 x86-64 的预发布验证。正式 Windows 客户端兼容性仍由
[Issue #143](https://github.com/maokelong/kat-cli/issues/143) 跟踪。

## PACK 输入输出合同

PACK 名为 `postgresql-query`，唯一 Workflow 名为 `query-postgresql`：

- 输入：一个必填 `sql: str`，位于 `kat run` 的 `--` 之后；
- Dataset：不接收，`required_tables=[]`；
- PostgreSQL 连接：`psycopg.connect()` 从当前进程的标准 `PG*` 环境变量读取；
- SQL：由 PostgreSQL 原样执行，必须恰好产生一个小 rowset；
- 输出：`postgresql_result`；所有非 NULL 值转为 UTF-8 string，NULL 保留，零行
  结果仍保留列结构。

Workflow 会完整缓冲结果。它不解析 SQL、不自动增加 `LIMIT`、不提供连接池，也不适合
大结果集或类型保真导出。SQL 会进入 KAT Operation log 和 Run Manifest，不能包含密码、
DSN、token 或其他秘密。

## 在联网准备机组装

### 1. 准备构建条件

准备机需要原生 Windows、Python 3.12 或更高版本，以及
`build/requirements-builder.lock.txt` 中锁定的构建依赖：

```powershell
py -3.14 -m pip install --require-hashes `
  -r .\build\requirements-builder.lock.txt
```

还需要以下输入：

1. 与当前 KAT Runtime 合同兼容的完整 `windows-x86_64` Payload；目录根级必须只含
   `kat.exe` 和 `python/`，不能使用单独的 Cargo 输出；
2. `build/runtime-inputs.json` 锁定的 Windows uv ZIP；
3. 同一文件锁定的 Microsoft VC Runtime VSIX；
4. 与 `build/postgresql-pack-devkit-inputs.json` 完全一致的三个 wheel。

wheelhouse 的精确文件名和 SHA-256 已锁定。可在联网 Windows CPython 3.14 环境下载：

```powershell
$wheelhouse = 'D:\kat-inputs\postgresql-wheelhouse'
New-Item -ItemType Directory -Path $wheelhouse

py -3.14 -m pip download --only-binary=:all: --no-deps `
  --dest $wheelhouse `
  'psycopg==3.3.4' `
  'psycopg-binary==3.3.4' `
  'tzdata==2026.3'
```

不要在受限目标机执行这一步。组装器会拒绝多余、缺失、改名或 hash 不符的 wheel。

### 2. 运行组装器

建议使用短路径，且输出目录和 ZIP 必须尚不存在：

```powershell
py -3.14 -B .\build\build_postgresql_pack_devkit.py `
  --windows-payload 'D:\kat-inputs\windows-x86_64' `
  --uv-archive 'D:\kat-inputs\uv-x86_64-pc-windows-msvc.zip' `
  --vc-redist-archive 'D:\kat-inputs\Microsoft.VC.14.44.17.14.CRT.Redist.X64.base.vsix' `
  --wheelhouse 'D:\kat-inputs\postgresql-wheelhouse' `
  --windows-payload-provenance '<Payload 发布 tag、提交和原始归档 SHA-256>' `
  --output 'D:\kat-build\postgresql-pack-devkit' `
  --archive 'D:\kat-build\postgresql-pack-devkit-windows-x86_64.zip'
```

组装器不会修改输入 Payload。它在副本中执行离线、带 hash 的 wheel 安装和
`uv pip check`，重新计算 native PE/VC Runtime 闭包，再验证 Python/Psycopg/libpq、
PACK inspection、目录结构和清单。成功后会同时产生：

- 完整开发包目录；
- ZIP；
- ZIP 旁的 `.sha256` 文件。

这些生成物、下载缓存、wheelhouse 和 Data Home 只放在 `target/` 或仓库外的受控目录，
不提交到 Git。

## 在网络受限目标机运行

通过受控介质传入 ZIP，并从独立可信渠道取得预期 SHA-256。旁路 `.sha256` 可帮助抄录，
但不能代替独立信任来源。先校验，再解压到已存在、可写、ACL 受控的本地目录：

```powershell
$expected = '<独立可信渠道提供的 64 位 SHA-256>'
$actual = (Get-FileHash `
  -LiteralPath '.\postgresql-pack-devkit-windows-x86_64.zip' `
  -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $expected.ToLowerInvariant()) { throw 'devkit ZIP hash mismatch' }
```

不要放到桌面同步盘或公共共享目录。目标机需要 PowerShell 7.3 或更高版本；脚本固定
Standard native argument passing，避免旧版本改写 SQL 中的双引号。

修改 PACK 前启动一个短命 PowerShell 7.3+ 进程验证原始交付：

```powershell
pwsh -NoProfile -File .\scripts\Verify-Devkit.ps1
```

随后编辑 `pack/workflows/`、`pack/tests/` 或 `queries/`。连接真实数据库并跑完整闭环：

```powershell
pwsh -NoProfile -File .\scripts\Invoke-LiveValidation.ps1 `
  -DatabaseHost '<服务端证书中的 DNS 名>' `
  -HostAddress '<实际连接 IP，可省略>' `
  -DatabasePort 5432 `
  -DatabaseName '<数据库名>' `
  -DatabaseUser '<只读账号>' `
  -CaCertificate 'D:\approved-ca\postgresql-root.crt'
```

脚本先在没有数据库凭据时执行 `kat inspect`，然后提示输入密码，仅在当前进程设置
`PGPASSWORD`，依次执行真实 `kat test`、无 Dataset 的 `kat run` 和 `kat query`，最后清除
全部 `PG*`。本地无 TLS 的一次性容器可显式使用 `-SslMode disable`；远程环境默认
`verify-full`，不应为了连通而隐式降级。

完整参数、目录用途和修改流程见开发包根级 `README.md`；凭据、TLS、Data Home 与数据库
授权配置见同级 `ENVIRONMENT.md`。

## 连接环境配置

运行脚本会设置 `PGHOST`、可选 `PGHOSTADDR`、`PGPORT`、`PGDATABASE`、`PGUSER`、
`PGPASSWORD`、`PGSSLMODE`、`PGSSLROOTCERT`、`PGCONNECT_TIMEOUT` 和
`PGCLIENTENCODING=UTF8`。无 DNS 但证书使用 DNS 名时，把证书身份放在 `PGHOST`，实际
连接 IP 放在 `PGHOSTADDR`。

PostgreSQL 官方列出了完整的
[libpq 环境变量](https://www.postgresql.org/docs/18/libpq-envars.html)和
[连接参数](https://www.postgresql.org/docs/18/libpq-connect.html)。官方也说明
`PGPASSWORD` 在部分系统上可能被同机进程看到，因此本开发包只在短命、受控进程中使用
专用只读凭据，不把密码写入脚本、`.env`、Profile、`setx`、SQL 或命令行。

Psycopg 使用自包含的 binary 安装；其官方说明见
[Psycopg 安装文档](https://www.psycopg.org/psycopg3/docs/basic/install.html)。目标机不得
自行替换或在线升级私有 Host 中的 wheel。

## 常见失败

- `Verify-Devkit.ps1` 报 hash 不匹配：原始交付被修改或损坏；重新取得受控 ZIP，不要
  随手重写清单。编辑 PACK 后 hash 变化是预期行为。
- inspection 导入失败：必须使用开发包内的 `kat.exe` 和相邻私有 Host，不能使用
  `PATH` 中的另一个 `kat` 或系统 Python。
- `verify-full` 报证书名称不匹配：修正 `DatabaseHost`；若只缺 DNS，另传
  `HostAddress`，不要把证书身份替换为不匹配的 IP。
- Data Home 选择失败：`data-home/` 必须已存在且可写；默认 KAT `config.json` 若已存在，
  也必须是有效 UTF-8 JSON。
- SQL 没有输出或产生多个 rowset：修改 SQL，使其只返回一个小结果集。
- 错误密码：操作应失败且不发布 Run；修正凭据后在新的短命进程重试。
