# KAT 命令速查

用户只需用自然语言说明目标。以下命令由 Agent 使用；始终以 `SKILL.md` 选出的平台载荷绝对路径替代示例中的 `kat`。

除 `--help` 外，每次调用的 stdout 都是一个 KAT Response JSON。只在 `status="success"` 时读取 `result`；失败时读取 `error`，以及存在时的 `log_path` 或 `test_report_path`。不要从终端文本、日志或 pytest 输出推断成功。

## 调用前选择平台载荷

先把 `SKILL.md` 的父目录解析为绝对 `<skill-root>`，并在每次操作前重新检查当前主机：

- Linux：读取 `uname -m` 与 `getconf GNU_LIBC_VERSION`。仅支持 glibc 2.28 或更高版本的 x86_64，执行 `<skill-root>/scripts/targets/linux-x86_64/kat`。
- Windows：读取原生架构、系统版本与 `Win32_OperatingSystem.ProductType`。Windows 10/11 x86_64 客户端（`ProductType=1`）是预发布候选目标，执行 `<skill-root>/scripts/targets/windows-x86_64/kat.exe`；正式支持仍需完成 [Issue #143](https://github.com/maokelong/kat-cli/issues/143) 的干净客户端验收。拒绝 Windows Server、Windows 7/8.1。

拒绝其他系统、架构、libc 或版本；载荷缺失时也拒绝，Linux 还需确认可执行位。始终使用上述绝对路径，不搜索 `PATH`，不回退到系统 Python 或系统 `kat`。

## 首次状态写入前确认 Data Home

Data Home 的默认配置文件位于 Linux 的 `$XDG_DATA_HOME/kat/config.json`（未设置时为 `$HOME/.local/share/kat/config.json`），或 Windows 的 `%APPDATA%\KAT\data\config.json`。它是用户维护的 KAT 私有配置；KAT CLI 和本 Skill 都不创建或写入它。

本次对话首次将要写入 KAT 状态时，展示当前平台的默认 Data Home 和配置路径，并询问是否更换：

- 不更换：直接调用 KAT，不编辑配置，也不设置、清空或猜测 `KAT_DATA_HOME`。
- 更换：取得一个已存在、可访问、可规范化的绝对目录，展示配置文件的绝对路径和以下只增加或更新 `kat_data_home` 的 JSON。保留其他字段，不展开 `~`、`%USERPROFILE%` 或 `$HOME`；等待用户确认已经手工修改后再调用 KAT。

```json
{
  "kat_data_home": "<已存在、可访问的绝对目录>"
}
```

Data Home 的优先级、校验与失败语义只由 KAT CLI 决定。损坏配置或无效的已选路径会失败；不得替用户读取或修改配置、设置或清空环境变量、创建目标目录，或改用其他目录自动重试。

## 查看帮助

```text
kat --help
kat <命令> --help
```

用于确认固定 CLI 路由和外层参数。Workflow 的业务参数不在 `kat run --help` 中；先读取选中 Workflow detail 的 `parameters`，再按其合同在 `--` 后传入。

## 发现 PACK

```text
kat inspect [--pack-dir <PACK目录> ...]
```

裸 `kat inspect` 只读取 `pack.toml`，返回 `result.packs` 中的 PACK manifest 概要；空列表也是成功结果。它不导入 PACK Python，不扫描 Workflow 或 Provider，也不读取 guide。`--pack-dir` 可重复，每个目录必须直接包含 `pack.toml`。

## 发现和读取 Workflow 知识

```text
kat inspect workflow --pack <PACK名称> [--pack-dir <PACK目录> ...]
kat inspect workflow --pack <PACK名称> --workflow <Workflow名称> \
  [--pack-dir <PACK目录> ...]
kat inspect workflow --run <Run ID> [--pack-dir <PACK目录> ...]
```

- 第一条列出 PACK 的 Workflow。成功 `result.workflows` 按 `name` 排序；每项恰好只有 `name`、`description`。
- 第二条返回一个精确 Workflow。成功 `result.workflow` 恰好只有 `name`、`description`、`parameters`、`guide`；未声明 guide 时 `guide` 是 JSON `null`。
- 第三条从 Run identity 定位 Workflow，并读取当前安装或指定 PACK 中该 Workflow 的 detail；它不会读取 Run 中的 guide 快照。`--run` 与 `--pack`、`--workflow` 互斥。

Workflow list 用于低成本筛选分析能力；只在选定一个 Workflow 后请求 detail。`guide` 是 Runtime 已读取的原始 Markdown 字符串，用于指导分析策略、结果发散和下一步方向。Agent 不自行拼接或打开 guide 路径。

Workflow list 仍会校验所有声明的 guide。任一 Workflow 导入、声明、重名或 guide 无效都会使本次 inspection 原子失败，不返回部分列表。

## 发现和读取 Provider 知识

```text
kat inspect provider --pack <PACK名称> [--pack-dir <PACK目录> ...]
kat inspect provider --pack <PACK名称> --provider <Provider名称> \
  [--pack-dir <PACK目录> ...]
```

- 第一条列出 PACK 的 Provider。成功 `result.providers` 按 `name` 排序；每项恰好只有 `name`、`description`。
- 第二条返回一个精确 Provider。成功 `result.provider` 恰好只有 `name`、`description`、`module`、`qualname`、`guide`；`guide` 始终是非空 Markdown 字符串。

Provider inspection 只用于 PACK 开发。它会导入所选 PACK `datasources/` 下的普通 Python 模块以发现声明，因此这些模块必须 import-safe；inspection 不实例化 Provider，也不连接服务、读取凭据或启动外部进程。一次扫描会校验全部 Provider 的导入、声明、名称唯一性和 guide，任何失败都不会返回部分结果。

Provider `guide` 是 Runtime 已读取的原始 Markdown，说明数据库、SQL、Schema 或接入方式。Agent 直接读取 Response 字段，不自行查找文件，也不把它当作分析策略。

## 执行 Workflow

```text
kat run --pack <PACK名称> --workflow <Workflow名称> \
  [--pack-dir <PACK目录> ...] -- \
  <来自 workflow detail.parameters 的参数>
```

只传 inspection 明示的 option；不要把秘密作为 Workflow 参数，因为参数可能进入 Operation log。成功后从 `result.run_id` 和 `result.outputs` 取得唯一可查询 Run 及其输出名称、columns 与行数。Workflow 自己选择并调用 Provider；分析 Agent 不需要先 inspect Provider。

## 查询 Workflow 输出

```text
kat query --run <Run ID> --sql <一条 SQL>
```

`kat query` 每次在独立 DataFusion Session 中只注册该 Run 的 `output.*` 与 `information_schema`。优先沿用刚完成的 `kat run` 成功 Response 中的 `result.outputs`；只有 Run ID 时，先依次查询实际 relation 与 columns：

```sql
SELECT table_name
FROM information_schema.tables
WHERE table_schema = 'output'
ORDER BY table_name
```

```sql
SELECT table_name, column_name, data_type, ordinal_position
FROM information_schema.columns
WHERE table_schema = 'output'
ORDER BY table_name, ordinal_position
```

随后只访问当前问题需要的列和行，在 SQL 中显式投影、过滤、聚合、排序，并给明细查询写入 `LIMIT`。Workflow inspection 提供分析策略，不替代 Output relation 发现。

成功结果恰含 `result.format="ndjson"`、`result.path` 与 `result.columns`。查询对象行不在 Response 内；只读取当前成功 Response 给出的 `result.path`，按 NDJSON 逐行取得证据。不要猜测或扫描 `query-results/`，不要读取 Run 内部文件，也不要假设 KAT 会自动限制查询。实际执行失败时按 Diagnostic 修正 SQL，不读取候选结果或包装成部分成功。

## 测试一个 PACK

```text
kat test --pack-dir <PACK目录>
kat test --pack-dir <PACK目录> --test <pytest node ID> [--test <pytest node ID> ...]
```

在生产执行平面运行该 PACK 的 pytest。`--pack-dir` 是一个直接包含 `pack.toml` 的精确目录，不使用 PACK 名称。成功 `result.summary` 是测试结论；失败时引用 Response 的 Diagnostic，以及存在时的 `test_report_path` 和 `log_path`。测试或诊断失败不授权修改 PACK。
