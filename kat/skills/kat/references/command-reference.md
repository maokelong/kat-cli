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

## 发现 Workflow 并准备输入

```text
kat inspect workflow --pack <PACK名称> [--pack-dir <PACK目录> ...]
kat inspect workflow --pack <PACK名称> --workflow <Workflow名称> \
  [--pack-dir <PACK目录> ...]
```

- 第一条列出 PACK 的 Workflow。成功 `result.workflows` 按 `name` 排序；每项恰好只有 `name`、`description`。
- 第二条返回一个精确 Workflow。成功 `result.workflow` 恰好只有 `name`、`description`、`parameters`，不返回分析 Guide。

Workflow list 用于低成本筛选能力，detail 用于执行前核对用途与参数。Guide 在执行时由程序捕获，AI 在执行后从 Run 成功结果或 `inspect run` 读取，不自行拼接或打开文件路径。

Workflow inspection 校验导入、声明与名称唯一性，不读取 Guide 文件；任一导入、声明或重名错误都会使本次 inspection 原子失败，不返回部分列表。只有执行选定 Workflow 时才读取并验证它声明的 Guide，读取失败会阻止该 Run 的业务执行与发布，不阻止其他 Workflow 的能力发现。

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

读取输出的首页，按相对链接加载 Provider/Workflow API 文档。Provider Guide 通过 Provider inspection 获取；Workflow Guide 在执行后从对应 Run 获取。公共函数的 Markdown 全部由 Skill 维护；SDK 升级后核对介绍标注的适用版本及已安装函数，不依赖旧路径或缓存内容。SDK 更新使用当前 KAT Python 的 `-m pip install --upgrade <wheel路径或URL>`，安装权限与环境检查遵循 [Python 依赖管理](python-packages.md)。

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

只传 inspection 明示的 option；不要把秘密作为 Workflow 参数，因为参数可能进入 Operation log。每次生产 Run 的 `--session` 都必填，并且必须引用已经存在的 Session；缺少、错误、不存在或损坏的 Session 都不会隐式创建、复用或猜测 Session。成功 `result` 恰含 `session_id`、`run_id`、`guide`、`outputs`、`child_runs`：`outputs` 提供输出名称、columns 与行数；`guide` 是本 Run 执行前捕获的 Markdown，未声明时为 `null`；`child_runs` 只给出直接成功子 Run ID，不展开子 Guide。顶层和 `ctx.run()` 子调用使用同一快照规则，不依赖 Analysis Record；Guide 读取失败不会发布缺失快照的成功 Run。

失败时不返回本次顶层 Run 的部分 `result` 或 Run ID，已存在的 Session 及其中已经独立发布的子 Run、Guide 仍可读取。Workflow 自己选择并调用 Provider；分析 Agent 不需要先 inspect Provider。`ctx.run()` 向父程序返回 Catalog 的合同不变。

例如，若 Workflow detail 明示 `--source-path` 和 `--limit`：

```text
kat run --session <Session ID> --pack example --workflow analyze \
  --pack-dir <PACK目录> -- \
  --source-path <本地来源路径> --limit 20
```

## 读取一个已发布 Run

```text
kat inspect run --session <Session ID> --run <Run ID>
```

成功 `result` 与该 Run 的执行成功结果有相同核心字段：`session_id`、`run_id`、`guide`、`outputs`、`child_runs`。读取执行时快照，无需当前 PACK 或 Analysis Record；历史快照缺失或损坏时报告错误，不用当前 PACK 补造。无输出为 `outputs: {}`，叶子为 `child_runs: []`。

刚执行完直接使用 Run Response，无需重复读取。历史或子 Run 的必要信息缺失时按需调用本命令；要恢复已保存的 AI 正文和总报告则使用下文 `analysis show`。

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

随后只访问当前问题需要的列和行，在 SQL 中显式投影、过滤、聚合、排序，并给明细查询写入 `LIMIT`。Run Guide 提供分析方法，不替代 Output relation 发现。

普通查询的成功结果恰含 `result.format="ndjson"`、`result.path` 与 `result.columns`。查询对象行不在 Response 内；只读取当前成功 Response 给出的 `result.path`，按 NDJSON 逐行取得证据。不要猜测或扫描 `query-results/`，不要读取 Run 内部文件，也不要假设 KAT 会自动限制查询。实际执行失败时按 Diagnostic 修正 SQL，不读取候选结果或包装成部分成功。

`--archive` 要求该 Session 已初始化分析记录；在普通结果外附加 `result.analysis`。程序保存这次实际 SQL、Run 来源、列和原始 NDJSON 文本，不让模型抄写行或重编码数值。探索后需要保存依据时执行更窄的证据 SQL，归档其完整少量结果；这是一份新证据，不保证与先前查询相同。归档失败不返回可恢复材料的成功身份，不能把已完成查询与已保存材料混为一谈。

## 保存和恢复分析

`analysis` 由 `kat-analyze` 在已授权的分析中自动管理，不要求用户主动调用或编写 JSON。CLI 保存执行事实和正文，不生成 AI 解释、不判断正文有效性。保存点和按需恢复规则见[分析 Skill](../../kat-analyze/SKILL.md#问题分析流程必须遵守)；`kat-review` 只读取，不保存或归档。

```text
kat analysis show --session S
kat analysis show --session S --run R
kat analysis show --session S --material M
kat analysis save --session S --expected-revision N --file payload.json
```

这些命令操作已有 Session 内唯一的 Analysis Record，不创建 Run。`--file` 指向 UTF-8 JSON 请求。首次 `save` 使用 `--expected-revision 0` 且必须包含 `goal`，成功创建 `revision: 1`；已有记录使用最近已核对的实际 revision。损坏或未知格式不能按不存在处理或覆盖。示例中的 S/R/M/N 均是占位符，实际使用成功 Response 中的身份与整数 revision。

请求只含本次需要保存的内容：

| 可选字段 | 内容 |
|---|---|
| `goal` | `{question,scope,gaps}`，首次必填；更新当前问题的范围与缺口，独立新目标新建 Session。 |
| `nodes` | `[{run_id,content}]`，按 Run 新建或替换该节点的 Markdown 正文，Run 必须属于本 Session 且已发布。 |
| `report` | `{content}`，保存当前综合报告的 Markdown 正文。 |

省略的字段和未提交的节点保留；不提交整份记录或派生树。节点正文保留必要结论、事实、证据出处、取证原因、范围和缺口，不拆分为选择或依赖字段。程序不解析正文重建依赖，也不要求固定标题或嵌套 JSON。

首次保存目标 `goal.json`：

```json
{"goal":{"question":"这个时间范围内出现了什么现象？","scope":"用户指定的来源与时间范围","gaps":[]}}
```

执行 `kat analysis save --session S --expected-revision 0 --file goal.json`。形成一个节点解释后，以最近已核对的 revision 保存 `node.json`：

```json
{"nodes":[{"run_id":"R","content":"根据本 Run 的 Guide 和查询材料 M，发现……。结论……，适用范围与限制……。"}]}
```

执行 `kat analysis save --session S --expected-revision N --file node.json`。成功后沿用返回的 revision；当前上下文已保留必要正文时，直接汇总并保存 `report.json`：

```json
{"report":{"content":"回答用户问题的综合报告，包含关键证据、范围与限制。"}}
```

正常成功 Response 即确认保存，无需另行 `show`。程序保留其他节点和旧总报告原文，不自动标记或清空；AI 判断是否需要修订或重新汇总。

### 读取正文和派生树

默认 `show` 与 `save` 的成功 `result` 包含 `schema_version`、`session_id`、`revision`、`goal`、`nodes`、材料 ID 到元数据的 `materials` 映射及可空 `report`。概览不展开 Guide 或 NDJSON 原文。`nodes` 是程序派生的嵌套报告树，每项为 `{run_id,content,children}`，叶子 `children: []`，每个 Run 只出现一次：

```json
{"nodes":[{"run_id":"R0","content":"父级解释","children":[{"run_id":"R1","content":"子级解释","children":[]}]}]}
```

首次保存 Run 正文即选入树。程序只按真实直接父关系组织已选节点：父已选入时挂该父之下，否则挂隐含的问题根；父之后保存时子自动归位，保留正文。不跨越未选中的中间父，也不将 AI 临时串联或时间先后变成调用边。没有解释的 Run 仍在 Session inventory 中，不要求先保存占位节点。

`show --run R` 返回 `result.{session_id,revision,node}`，其中 `node` 为 `{run_id,content}`；它读取已保存解释，不是 Run Guide。`show --material M` 返回 `result.{session_id,revision,material_id,material}`，含实际归档的查询来源、SQL、列与 NDJSON 原文。两个选择器互斥。读取不加载当前 PACK；没有记录、损坏和未知格式分别诊断。

当前上下文足够时，不为汇总或确认保存重复读取。新任务、缺少必要上下文、获知外部修改、保存冲突或响应不确定时，才 `show` 恢复或确认。记录仅保证最近成功保存的内容可恢复，不保证正文已随其他修改同步或结论正确。

### 材料归档回执与修订

`query --archive` 保持普通查询结果并附加：

```json
{"analysis":{"session_id":"S","previous_revision":3,"revision":4,"material_id":"M"}}
```

该片段位于成功 Response 的 `result` 内。材料追加要求已有分析记录，不要求 `--expected-revision`，但在锁下追加并增加 revision。仅当 `previous_revision` 等于 Agent 先前已读或自行保存的 revision 时，才能直接沿用新 revision；否则先 `analysis show` 核对并发修改，不能用归档回执跳过未读变更。原材料恢复不依赖 Session 外的查询文件；Guide 由 Run 保存，不归入查询材料。

`save` 整体校验并提交，预期 revision 不一致时拒绝旧写。冲突后重读核对，不能只换数字重提。保存失败不能覆盖先前完整记录；响应不确定时先读取确认，不盲目重复保存、归档或重跑 Workflow。保存成功才承诺本次内容可恢复。

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
