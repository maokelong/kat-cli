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
│   └── smoke.sql                    # 外部 SQL 文件示例
├── skill/
│   ├── SKILL.md
│   └── scripts/targets/windows-x86_64/
│       ├── kat.exe
│       └── python/python.exe
├── pack/
│   ├── pack.toml
│   ├── queries/
│   │   └── smoke.sql                # 固定文件 Workflow 的 SQL
│   ├── workflows/
│   └── tests/
└── data-home/
```

`skill/` 是标准 Windows Platform Payload；其私有 Host 已正式包含
`kat.common.sql.postgresql`、Psycopg、openpyxl、XlsxWriter 和 defusedxml。`pack/` 是可编辑的
External PACK 源码。`data-home/` 保存 Operation log、PACK test report、Run Manifest 和
Run Output。

## 使用顺序

### 1. 解压后先验证原始交付物

先从受信渠道取得 ZIP 的外置 `.sha256`，用 `Get-FileHash -Algorithm SHA256` 比较 ZIP；外置 hash 应与 ZIP 分开传递或发布。ZIP 内的 `SHA256SUMS` 能发现解压后的损坏，但如果攻击者可以同时替换文件和清单，它本身不提供来源认证。

解压后，在修改 `pack/` 前，从新的短命 PowerShell 7.3+ 进程运行：

```powershell
pwsh -NoProfile -File .\scripts\Verify-Devkit.ps1
```

验证脚本先检查根级 `SHA256SUMS`，随后检查私有 Host 的 CPython、Psycopg binary、libpq、
`kat.common` 导入和 Excel 实际写读，最后在清空全部继承 `PG*` 的环境中检查两个 Workflow。
它不会请求数据库密码，也不会连接数据库。

`SHA256SUMS` 描述组装时的原始文件。修改 PACK 后 hash 变化是预期行为；需要重新确认原始交付物时，请重新解压一份或与受控准备机的源码比较，不要把修改后的 hash 冒充原始清单。

### 2. 编辑 PACK 或 SQL

- Workflow 入口位于 `pack/workflows/`。
- PACK test 位于 `pack/tests/`。
- 默认真实验证使用 `query-postgresql-file`，其 SQL 位于 `pack/queries/smoke.sql`，Workflow
  通过 `__file__` 构造绝对路径。
- 根级 `queries/smoke.sql` 是外部文件调用示例；传绝对 `-SqlFile` 时脚本读取指定 UTF-8 文件，
  并通过 `query-postgresql --sql` 执行，不会修改 PACK。
- Workflow 的 `required_tables=[]`，因此不创建 Dataset，也不运行 `kat import`。
- 远程 SQL 原样交给 PostgreSQL。必须自行保证它只返回一个小 rowset；KAT 不解析、限制、改写或自动添加 `LIMIT`。

通过 `query-postgresql --sql` 传入的 SQL 文本是 Workflow input，会进入 Operation log 和 Run
Manifest。固定文件 Workflow 的 Run 不记录文件路径、内容或摘要；这也不构成可复现性保证。
任何 SQL 都不得包含密码、token、DSN 或其他秘密。

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
5. 执行真实 `kat test`，覆盖文本和固定文件两个 Workflow；
6. 不传 Dataset 执行 `kat run`；默认选择固定文件 Workflow，提供 `-SqlFile` 时选择文本
   Workflow；
7. 从成功 Response 取得 Run ID 和 `postgresql_result` 元数据；
8. 立即清除全部 `PG*`，释放 BSTR 与 SecureString；
9. 在无数据库凭据状态对 `output.postgresql_result` 执行 `COUNT(*)`；
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

## 开发固定 SQL 与外部 SQL

`pack/queries/smoke.sql` 是默认 Workflow 固化的 SQL。修改它之后，运行默认命令即可验证修改。
PACK 内固定文件应像示例一样相对 Python 模块定位，再传给 common 的文件接口：

```python
from pathlib import Path

import kat
from kat.common.sql import postgresql

SQL_FILE = (Path(__file__).resolve().parents[1] / "queries" / "orders.sql").resolve()


@kat.workflow(
    name="query-orders",
    title="Query Orders",
    required_tables=[],
)
def query_orders(ctx: kat.Context):
    """Execute the fixed orders query."""
    return {
        "orders": postgresql.execute_sql_file(ctx, SQL_FILE),
    }
```

如果 SQL 由 PACK 之外的受控目录共享，也可以在 Workflow 中直接构造该文件的绝对路径。部署到
另一台机器时必须保证路径合同仍成立；`execute_sql_file()` 不接受相对路径，也不展开 `~`、
`%ENV%` 或通配符。

只想临时验证其他 SQL 时，建议另建文件并通过脚本的 `-SqlFile` 入口：

```powershell
pwsh -NoProfile -File .\scripts\Invoke-LiveValidation.ps1 `
  -DatabaseHost '<数据库证书身份>' `
  -DatabaseName '<数据库名>' `
  -DatabaseUser '<只读账号>' `
  -CaCertificate 'D:\approved-ca\postgresql-root.crt' `
  -SqlFile 'D:\approved-sql\my-query.sql'
```

此时脚本读取文件内容，并把它传给 `query-postgresql`；这不会让文件路径成为 PACK 合同。若 SQL
使用参数，直接在 Workflow 中调用：

```python
postgresql.execute_sql_text(
    ctx,
    "SELECT * FROM orders WHERE order_day = %(day)s",
    parameters={"day": day},
)
```

参数必须使用 Psycopg 的 `%(name)s` 占位符，禁止通过 Python 字符串拼接或替换注入值。

common 保真映射首版支持的布尔、数值、文本、二进制和日期时间标量；未知 PostgreSQL 类型会
明确失败，不会静默字符串化。请在 SQL 中把数组、JSON、UUID、枚举、复合类型、区间和扩展类型
显式 `CAST` 为支持类型。当前 `kat query` 的 JSON 输出不能直接编码全部 Arrow 日期时间类型；
Run Output 仍会保留这些类型，终端展示时应在 Query SQL 中显式转为 `VARCHAR`。

## 使用 Excel 库

标准 Host 预装 openpyxl 3.1.5、XlsxWriter 3.2.9 和 defusedxml 0.7.1。PACK 可以直接 import：

```python
import openpyxl
import xlsxwriter
```

openpyxl 适合读取、修改和写入 `.xlsx/.xlsm`；XlsxWriter 适合生成新的 `.xlsx`。PACK 不需要也
不能在目标机执行 pip。旧 `.xls`、`.xlsb` 和 pandas 未预装；需要时必须先由 KAT Platform
正式增加并验证依赖，不能依赖系统 Python。

环境与安全配置见 [ENVIRONMENT.md](ENVIRONMENT.md)。
