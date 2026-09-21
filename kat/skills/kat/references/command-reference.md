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

为分析显式保存本次实际 Guide 时，在 detail inspection 上增加归档参数：

```text
kat inspect workflow --pack P --workflow W --archive-to-session S
kat inspect workflow --session S --run R --archive-to-session S
```

需要外部 PACK 时仍可提供 `--pack-dir`。归档目标必须已有 Analysis Record；按 Run 定位时两个 Session 必须一致。程序保存本次真实 Guide 字符串或 `null`、PACK/Workflow 身份，原 `result.workflow` 保持不变，并附加 `result.analysis` 回执。执行前可以尚无 Run，之后节点引用材料时核对 Workflow 身份。没有显式归档参数时仍只读取当前知识；恢复历史依据使用下文 `analysis show --material`，不重新读取当前 PACK。

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

公共函数通过 Python import 使用，不注册为 Workflow 或 Provider。先从 [公共库导航](helpers/index.md) 阅读按领域维护的使用说明与 API Markdown，按需用 `inspect.signature()` 和 `inspect.getdoc()` 核对已安装函数。Provider/Workflow 的随包 API 从知识首页读取：Windows 使用与 CLI 同目录的 `python/python.exe`，Linux 使用 `python/bin/python3`，均替换为绝对路径。

```text
<当前 KAT Python> -I -B -c "from importlib.resources import files; print(files('kat_sdk').joinpath('knowledge/index.md'))"
```

读取输出的首页，按相对链接加载 Provider/Workflow API 文档；其 Guide 优先通过上述 inspection 获取。公共函数的 Markdown 全部由 Skill 维护；SDK 升级后核对介绍标注的适用版本及已安装函数，不依赖旧路径或缓存内容。SDK 更新使用当前 KAT Python 的 `-m pip install --upgrade <wheel路径或URL>`，安装权限与环境检查遵循 [Python 依赖管理](python-packages.md)。

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
kat query --session <Session ID> --run <Run ID> --sql <一条只读 SQL> [--archive]
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

普通查询的成功结果恰含 `result.format="ndjson"`、`result.path` 与 `result.columns`。查询对象行不在 Response 内；只读取当前成功 Response 给出的 `result.path`，按 NDJSON 逐行取得证据。不要猜测或扫描 `query-results/`，不要读取 Run 内部文件，也不要假设 KAT 会自动限制查询。实际执行失败时按 Diagnostic 修正 SQL，不读取候选结果或包装成部分成功。

`--archive` 要求该 Session 已初始化分析记录；在普通结果外附加 `result.analysis`。程序保存这次实际 SQL、Run 来源、列和原始 NDJSON 文本，不让模型抄写行或重编码数值。探索后需要保存依据时执行更窄的证据 SQL，归档其完整少量结果；这是一份新证据，不保证与先前查询相同。归档失败不返回可恢复材料的成功身份，不能把已完成查询与已保存材料混为一谈。

## 保存和恢复分析

`analysis` 是供 AI/Skill 管理分析状态的公开命令接口。用户通过自然语言发起分析，`kat-analyze` 按[分析流程中的触发时机](../../kat-analyze/references/analysis-flow.md)自动初始化、保存和恢复，不要求用户主动调用这些命令或编写 JSON。CLI 负责持久化与校验，不会因 `run` 成功自动产生 AI 解释或总报告；调用时机由 Skill 负责。`kat-review` 仅按自身复核流程读取记录，不初始化、更新或归档。

```text
kat analysis init --session S --file goal.json
kat analysis show --session S
kat analysis show --session S --run R
kat analysis show --session S --material M
kat analysis update --session S --expected-revision N --file changes.json
```

这些命令操作已存在 Session 内唯一的 Analysis Record，不创建 Run。`init` 只创建尚不存在的记录，已有记录拒绝覆盖，初始 `revision` 为 1。`--file` 指向 UTF-8 JSON 请求文件；下例的 S/R/G/Q/N 是说明用占位符，实际使用成功 Response 中的完整 Session、Run、材料身份及整数 revision，不能照抄占位值。

`goal.json` 直接保存目标对象：

```json
{"question":"这个时间范围内出现了什么现象？","scope":"用户指定的来源与时间范围","gaps":[]}
```

`init`、默认 `show`、`update` 的成功 `result` 包含 `schema_version`、`session_id`、`revision`、`goal`、有序 `nodes`、材料 ID 到元数据的 `materials` 映射和 `report`。概览不展开所有 Guide/NDJSON 正文。`show --run R` 返回 `result.{session_id,revision,node}`；`show --material M` 返回 `result.{session_id,revision,material_id,material}`，其中有保存的原材料。两个选择器互斥；恢复读取不加载当前 PACK。没有记录、损坏或未知格式按 Diagnostic 处理，不当作空分析覆盖。

每个节点只有一个 `run_id`，通过 `report_parent` 指向已选入的真实直接程序父 Run；`null` 挂隐含报告根。同父节点的数组相对顺序就是报告顺序，没有另存的 `tree` 或 `choices`。节点还保存可空 `selection` 与 `interpretation`，前者记录已知 AI 选择依据，后者保存解释及实际依赖。

`changes.json` 的唯一外层字段是 `changes` 数组。可用的领域更新如下；不得提交整份记录或通用 JSON Patch：

| `op` | 字段与含义 |
|---|---|
| `set_goal` | `goal:{question,scope,gaps}`；更新同一目标的范围与缺口，独立新目标新建 Session |
| `select_node` | `run_id,report_parent,before`；选入或移动原节点；`before` 是同父节点的 Run ID，`null` 放末尾；保留原选择依据和解释 |
| `set_selection` | `run_id,selection`；值为 `null` 或 `{source_runs:[],materials:[],finding,reason,input_sources:{}}`；`input_sources` 按输入名记录来源说明 |
| `set_interpretation` | `run_id,interpretation:{facts:[],conclusion,scope,limitations:[],guide,evidence:[],uses:[]}`；Guide/证据使用程序返回的材料 ID |
| `set_report` | `report:{content,uses:[]}`；保存一份综合报告及实际采用的解释引用 |

每个 `uses` 项都是 `{"run_id":"R","interpretation_version":1}`，版本必须来自成功保存的解释。CLI 生成 `interpretation_version`、解释/报告的 `state` 和记录 revision；请求不能自写这些字段、Guide 正文或原始证据。Guide 材料匹配节点 Workflow，证据可来自同 Session 的其他 Run，不强制把材料来源选入报告；`uses` 引用已有节点解释，也允许跨分支引用。AI 选择来源不自动成为解释依赖。

以下三个更新文件按顺序分别提交，每次使用最近读写的 revision；真实结论和证据 ID 必须来自本次分析。

先用 `select.json` 选入一个独立报告节点：

```json
{"changes":[{"op":"select_node","run_id":"R","report_parent":null,"before":null}]}
```

运行 `kat analysis update --session S --expected-revision N --file select.json`。程序子节点把 `report_parent` 换为已选入的真实父 Run，CLI 按 Manifest 核对；同一 Run 重挂不复制节点。AI 选择后续根 Run 时可在同批追加 `set_selection`：

```json
{"changes":[{"op":"set_selection","run_id":"R2","selection":{"source_runs":["R1"],"materials":["Q1"],"finding":"证据 Q1 中记录的发现摘要","reason":"进一步核对该现象","input_sources":{"thread_id":"来自 Q1 的实际 thread_id 值"}}}]}
```

选入节点并归档实际 Guide G、证据 Q 后，用 `interpretation.json` 保存解释：

```json
{
  "changes": [{
    "op": "set_interpretation",
    "run_id": "R",
    "interpretation": {
      "facts": ["按证据 Q 记录的事实"],
      "conclusion": "由实际证据支持的局部结论",
      "scope": "本次查询覆盖的来源、对象和范围",
      "limitations": [],
      "guide": "G",
      "evidence": ["Q"],
      "uses": []
    }
  }]
}
```

运行相同的 `analysis update`，替换 JSON 文件名与 revision。需要采用其他节点解释时，将它成功返回的 Run ID/解释版本加入 `uses`；没有自己的表时可以直接采用必要子证据或子解释。未声明 Guide 时仍引用那份记录 `guide=null` 的材料。

从返回节点读取实际 `interpretation_version`，再用 `report.json` 汇总（下例的 1 也须换成真实版本）：

```json
{"changes":[{"op":"set_report","report":{"content":"回答用户问题的综合报告，包含关键证据、范围与限制。","uses":[{"run_id":"R","interpretation_version":1}]}}]}
```

`update` 在预期 revision 不一致时拒绝旧写；重新 `show` 核对其他任务的修改后再决定更新，不能仅换成新数字重提。新解释令实际消费者和总报告失效；重排节点使旧总报告失效。`current` 只表示登记依赖有效，不保证文字判断正确。依赖新解释的消费者分次提交：先保存子解释取得版本，再保存父解释，最后报告；同批不得引用这次尚未生成的新版本，也不能让新的消费者使用本批被替换的旧版本。

### 材料归档回执与修订

显式 Guide/query 归档保持原结果并附加以下结构：

```json
{"analysis":{"session_id":"S","previous_revision":3,"revision":4,"material_id":"M"}}
```

该片段位于成功 Response 的 `result` 内。材料追加不要求 `--expected-revision`，但会锁住最新记录追加并增加 revision。只有 `previous_revision` 等于 Agent 先前读/写的 revision 时，才能直接接受新 revision；否则先 `analysis show` 并核对变化，再提交解释。新材料尚未被解释采用时，不使旧解释失效，也不自动成为结论依据。

保存成功才承诺本次状态可恢复；损坏不能作为缓存丢弃。保存响应不确定时重新 `show` 检查，不能盲重提、重复归档或重跑 Workflow。普通 query/inspection 不带归档参数时行为不变，材料恢复不依赖 Session 外的查询文件。

## 删除一个 Session

```text
kat session delete --session <Session ID>
```

这是唯一删除入口，会永久删除该 Session 的 Runs、Outputs、materializations、scratch，以及存在时的 Analysis Record 和归档材料。成功 `result` 恰含 `session_id`；删除不创建 Operation log，也不删除 Session 外的既有 Operation logs 或 Query Results。活跃 Run、Query、inspection 或分析记录读写占用该 Session 时删除立即失败且不修改它。失败后若已进入内部 tombstone，可以用相同命令续删；KAT 不提供单 Run 删除、TTL 或自动 GC。

只有用户明确要求永久删除这个已知 Session 时才调用；分析完成、切换问题或磁盘空间可能不足都不自动构成删除授权。

## 测试一个 PACK

```text
kat test --pack-dir <PACK目录>
kat test --pack-dir <PACK目录> --test <pytest node ID> [--test <pytest node ID> ...]
```

在生产执行平面运行该 PACK 的 pytest。`--pack-dir` 是一个直接包含 `pack.toml` 的精确目录，不使用 PACK 名称。测试 fixture 用普通来源文件、配置和临时路径构造 Provider；`kat_run` 只接收 Workflow 和 arguments。成功 `result.summary` 是测试结论；失败时引用 Response 的 Diagnostic，以及存在时的 `test_report_path` 和 `log_path`。测试或诊断失败不授权修改 PACK。

Provider inspection 不带 `--pack` 时只查询平台公共 Provider（`ftrace-text`、`trace-streamer-sqlite`）；带 `--pack` 时只查询该 PACK 自有声明。两个范围允许同名，未找到时不自动切换。`--pack-dir` 必须与 `--pack` 一起使用。
