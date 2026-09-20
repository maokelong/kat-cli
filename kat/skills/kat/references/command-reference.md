# KAT 命令速查

用户只需用自然语言说明目标。以下命令供 KAT Skills 共用；始终以本合同选出的平台载荷绝对路径替代示例中的 `kat`。

除 `--help` 外，每次 KAT CLI 调用的 stdout 都是一个 KAT Response JSON。只在 `status="success"` 时读取 `result`；失败时读取 `error`，以及存在时的 `log_path` 或 `test_report_path`。不要从终端文本、日志或 pytest 输出推断成功。

## 调用前选择平台载荷

`<kat-root>` 固定为总入口 `kat/SKILL.md` 的绝对父目录。直接使用 `kat-analyze`、`kat-author`、`kat-review` 或 `kat-dev-sdk` 时，从该任务 `SKILL.md` 所在目录解析相邻 `../kat/`，确认 `kat/SKILL.md` 后使用同一根目录；不能锚定任务 Skill、当前工作目录或其他版本的部署。

在每次操作前重新检查当前主机：

- Linux：读取 `uname -m` 与 `getconf GNU_LIBC_VERSION`。仅支持 glibc 2.28 或更高版本的 x86_64，执行 `<kat-root>/scripts/targets/linux-x86_64/kat`。
- Windows：读取原生架构、系统版本与 `Win32_OperatingSystem.ProductType`。Windows 10/11 x86_64 客户端（`ProductType=1`）是预发布候选目标，执行 `<kat-root>/scripts/targets/windows-x86_64/kat.exe`；正式支持仍需完成 [Issue #143](https://github.com/maokelong/kat-cli/issues/143) 的干净客户端验收。拒绝 Windows Server、Windows 7/8.1。

拒绝其他系统、架构、libc 或版本；载荷缺失时也拒绝，Linux 还需确认可执行位。始终使用上述绝对路径，不搜索 `PATH`，不回退到系统 Python 或系统 `kat`。

## 沿用当前 Data Home

直接调用 KAT，由 CLI 选择当前 Data Home；不预先询问是否更换目录或展示配置教程。选择优先级、校验与失败语义由 CLI 决定，不把平台默认路径当作实际选中路径。不得替用户读取或修改配置、设置或清空 `KAT_DATA_HOME`、创建覆盖目录，或改用其他目录自动重试。

仅在用户明确要求更换时说明配置方式：配置文件固定位于 Linux 的 `$XDG_DATA_HOME/kat/config.json`（未设置时为 `$HOME/.local/share/kat/config.json`），或 Windows 的 `%APPDATA%\KAT\data\config.json`，由用户维护。取得已存在、可访问的绝对目录后，指导用户仅更新字符串字段 `kat_data_home` 并保留其他字段；不展开 `~`、`%USERPROFILE%` 或 `$HOME`。说明非空 `KAT_DATA_HOME` 的优先级更高，等待用户完成手工调整后再调用 KAT，依据实际 Response 验证。

## 失败与缺项的交付

只引用成功 Response 中存在的公开字段。调用失败时说明停止阶段、已验证事实、Diagnostic，以及可用 `log_path`、`test_report_path` 或其他可追溯证据，并给出最小下一步；不发布部分 discovery 结果，不把候选 Run 或日志中的乐观文字描述为成功。

仅在缺少继续任务的关键事实，或选择会改变实质结论时补问，说明已确认事实和缺项，一次只询问一个最小必要问题，不要求用户选择内部命令。

Data Home 失败时引用实际 Diagnostic，说明尚未完成的操作；沿用当前配置停止，不把配置失败改成目录选择问卷。用户正在手工调整时说明等待事项，不把用户确认当作 KAT 验证成功。

分析和复核遇到缺失 Python 依赖时报告缺项，不自动安装。用户明确要求安装时，读取 [Python 依赖管理](python-packages.md) 并按授权执行；创作中的补装边界由 `kat-author` 规定。

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

裸 `kat inspect` 通过当前 KAT Python 定位已安装 SDK 根，再读取候选 `pack.toml`，返回 `result.packs` 中的 PACK manifest 概要。官方 SDK 作为 `kat-sdk` PACK 自动加入发现范围，无需 `--pack-dir`；未安装 SDK 时只发现原有 PACK，已安装 SDK 的资源损坏时报告错误。它不导入 PACK Python，不扫描 Workflow 或 Provider，也不读取 guide。`--pack-dir` 可重复，每个目录必须直接包含 `pack.toml`。

## 发现和读取 Workflow 知识

```text
kat inspect workflow --pack <PACK名称> [--pack-dir <PACK目录> ...]
kat inspect workflow --pack <PACK名称> --workflow <Workflow名称> \
  [--pack-dir <PACK目录> ...]
kat inspect workflow --session <Session ID> --run <Run ID> \
  [--pack-dir <PACK目录> ...]
```

- 第一条列出 PACK 的 Workflow。成功 `result.workflows` 按 `name` 排序；每项恰好只有 `name`、`description`。
- 第二条返回一个精确 Workflow。成功 `result.workflow` 恰好只有 `name`、`description`、`parameters`、`guide`；未声明 guide 时 `guide` 是 JSON `null`。
- 第三条从 `(Session ID, Run ID)` 定位 Workflow，并读取当前安装或指定 PACK 中该 Workflow 的 detail；它不会读取 Run 中的 guide 快照。`--session` 与 `--run` 必须一起提供，该定位与 `--pack`、`--workflow` 互斥。

Workflow list 用于低成本筛选分析能力；只在选定一个 Workflow 后请求 detail。`guide` 是 Runtime 已读取的原始 Markdown 字符串，用于指导分析策略、结果发散和下一步方向。Agent 不自行拼接或打开 guide 路径。

Workflow list 仍会校验所有声明的 guide。任一 Workflow 导入、声明、重名或 guide 无效都会使本次 inspection 原子失败，不返回部分列表。

## 发现和读取 Provider 知识

```text
kat inspect provider
kat inspect provider --provider ftrace-text
kat inspect provider --pack <PACK名称> [--pack-dir <PACK目录> ...]
kat inspect provider --pack <PACK名称> --provider <Provider名称> \
  [--pack-dir <PACK目录> ...]
```

- 第一条列出已安装 SDK 的公共 Provider；未安装 SDK 时返回空列表。第二条返回公共 `ftrace-text` 的详情，未安装时按目标不存在报告。
- 第三条只列出所选 PACK 自有的 Provider，第四条返回该范围内一个精确 Provider。
- 列表的成功 `result.providers` 按 `name` 排序；每项恰好只有 `name`、`description`。详情的成功 `result.provider` 恰好只有 `name`、`description`、`module`、`qualname`、`guide`；`guide` 始终是非空 Markdown 字符串。

公共 Provider inspection 可由各领域直接使用。公共范围读取当前 SDK 的声明与随包 Guide，不依赖领域 PACK；PACK 范围会导入所选 PACK `datasources/` 下的普通 Python 模块以发现声明，因此这些模块必须 import-safe。inspection 不实例化 Provider，也不连接服务、读取凭据或启动外部进程。所选范围内的声明、名称唯一性或 guide 任一无效，都会使本次 inspection 失败，不返回部分结果。

Provider `guide` 是 Runtime 已读取的原始 Markdown，说明数据库、SQL、Schema 或接入方式。Agent 直接读取 Response 字段，不自行查找文件，也不把它当作分析策略。

## 阅读 SDK 公共函数和 API 文档

公共函数通过 Python import 使用，不注册为 Workflow 或 Provider。先从 [公共库导航](libraries/index.md) 阅读按领域维护的介绍；需要核对当前安装版本的完整 API 时，用当前载荷的 Python 定位知识首页：Windows 使用与 CLI 同目录的 `python/python.exe`，Linux 使用 `python/bin/python3`，均替换为绝对路径。

```text
<当前 KAT Python> -I -B -c "from importlib.resources import files; print(files('kat_sdk').joinpath('knowledge/index.md'))"
```

读取输出的首页，按相对链接加载所需文档。公共库介绍由 Skill 维护，生成 API 随 SDK 发布；SDK 升级后重新定位 API，核对介绍标注的适用版本，不依赖旧路径或缓存内容。Provider/Workflow 的 Guide 优先通过上述 inspection 获取。SDK 更新使用当前 KAT Python 的 `-m pip install --upgrade <wheel路径或URL>`，安装权限与环境检查遵循 [Python 依赖管理](python-packages.md)。

## 创建一个 Session

```text
kat session create
```

生产分析必须在运行任何 Workflow 前显式创建 Analysis Session。该命令不接受 Session ID；成功 Response 精确为 `{"status":"success","result":{"session_id":"<Session ID>"}}`，并发布一个可以为空的 Session。失败时没有部分 `result`，也不能把候选目录当作 Session。新建后可以立即用 Session inspection 得到 `runs: []`。

## 执行 Workflow

```text
kat run --session <已有 Session ID> \
  --pack <PACK名称> --workflow <Workflow名称> \
  [--pack-dir <PACK目录> ...] -- \
  <来自 workflow detail.parameters 的参数>
```

只传 inspection 明示的 option；不要把秘密作为 Workflow 参数，因为参数可能进入 Operation log。每次生产 Run 的 `--session` 都必填，并且必须引用已经存在的 Session；缺少、错误、不存在或损坏的 Session 都不会隐式创建、复用或猜测 Session。成功后从 `result.session_id`、`result.run_id` 和 `result.outputs` 取得本次顶层可查询 Run 及其输出名称、columns 与行数；Response 不递归展开 `child_runs`。失败时不返回本次顶层 Run 的部分 `result` 或 Run ID，已存在的 Session 及其中已经独立发布的 Run 仍可 inspection。Workflow 自己选择并调用 Provider；分析 Agent 不需要先 inspect Provider。

例如，若 Workflow detail 明示 `--source-path` 和 `--limit`：

```text
kat run --session <Session ID> --pack example --workflow analyze \
  --pack-dir <PACK目录> -- \
  --source-path <本地来源路径> --limit 20
```

## 查看一个 Session

```text
kat inspect session --session <Session ID>
```

成功 `result` 恰含 `session_id` 和按 Run ID 稳定排序的 `runs`；空 Session 返回 `runs: []`。每个 Run 只包含 `run_id`、`pack`、`workflow`、`child_runs` 与和 Run success 相同形状的 `outputs` inventory。`child_runs` 只列出该 Run 直接、成功发布的子 Run ID，按 Run ID 排序但语义无序，叶子固定为 `[]`；它不递归嵌入后代，也不表达调用顺序、分支或线程。结果不包含 inputs、materializations、scratch、失败调用、执行计划或物理路径。KAT 不提供 Session list/current，也不跨 Data Home 搜索。

## 查询 Workflow 输出

```text
kat query --session <Session ID> --run <Run ID> --sql <一条只读 SQL>
```

`kat query` 每次在独立 DataFusion Session 中只注册该双 ID 定位 Run 的 `output.*` 与 `information_schema`。优先沿用刚完成的 `kat run` 成功 Response 中的 `result.outputs`；只有双 ID 时，先依次查询实际 relation 与 columns：

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

## 删除一个 Session

```text
kat session delete --session <Session ID>
```

这是唯一删除入口，会永久删除该 Session 的 Runs、Outputs、materializations 与 scratch。成功 `result` 恰含 `session_id`；删除不创建 Operation log，也不删除 Session 外的既有 Operation logs 或 Query Results。活跃 Run、Query 或 inspection 占用该 Session 时删除立即失败且不修改它。失败后若已进入内部 tombstone，可以用相同命令续删；KAT 不提供单 Run 删除、TTL 或自动 GC。

只有用户明确要求永久删除这个已知 Session 时才调用；分析完成、切换问题或磁盘空间可能不足都不自动构成删除授权。

## 测试一个 PACK

```text
kat test --pack-dir <PACK目录>
kat test --pack-dir <PACK目录> --test <pytest node ID> [--test <pytest node ID> ...]
```

在生产执行平面运行该 PACK 的 pytest。`--pack-dir` 是一个直接包含 `pack.toml` 的精确目录，不使用 PACK 名称。测试 fixture 用普通来源文件、配置和临时路径构造 Provider；`kat_run` 只接收 Workflow 和 arguments。成功 `result.summary` 是测试结论；失败时引用 Response 的 Diagnostic，以及存在时的 `test_report_path` 和 `log_path`。测试或诊断失败不授权修改 PACK。

Provider inspection 不带 `--pack` 时只查询平台公共 Provider（`ftrace-text`、`trace-streamer-sqlite`）；带 `--pack` 时只查询该 PACK 自有声明。两个范围允许同名，未找到时不自动切换。`--pack-dir` 必须与 `--pack` 一起使用。
