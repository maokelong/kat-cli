---
name: kat-analyze
description: 围绕用户问题应用 PACK 经验、按需执行 Workflow 取证并保存报告；凭 Session ID 恢复、继续分析或查询已有依据。
---

# KAT Analyze

围绕一个用户目标组织已有知识与程序能力，形成有依据、可恢复的 Analysis Result。**必须遵守以下分析流程，按当前起点和证据需要选择分支。** 纯知识任务不要求 Run；不能为了补齐流程而执行不需要的步骤。

调用前读取相邻 [共享执行合同](../kat/references/command-reference.md)，从本文件的绝对位置解析 `../kat/` 作为共享部署，不使用工作目录或其他版本。命令中的 `kat` 换成该合同选定的 CLI 绝对路径；已明确的外部 PACK 目录在相关调用持续携带 `--pack-dir D`。

## 1. 确定目标与起点

- 当前上下文已有必要信息和成功保存结果时直接继续，不因汇总或压缩机械重读。
- 只有 Session ID 或缺少继续信息时，用 `kat analysis show --session S` 恢复目标、revision、报告和材料概要；用 `--run R` 读取节点，`--material M` 读取完整材料，两者互斥。已有适用报告可直接只读交付，不重新发现、运行或保存。
- 只有 Run ID 且无法确定所属 Session 时询问 Session，不跨目录搜索。

一个 Session 对应一个任务目标。同目标追问与补证沿用 Session，独立新目标新建。只读请求不创建记录、采纳材料、运行 Workflow 或写入分析。已有记录明确不存在时，可结合已知目标与 `kat inspect session --session S` 的成功 Run 建立记录，不能补造旧结论；记录损坏或格式未知时按[特殊情况处理](references/analysis-recovery.md)。

## 2. 选择适用能力

用户已指定精确 PACK 和知识入口/Workflow 时，先验证该目标的 detail 与输入是否适用，不重新执行全量筛选：

```text
kat inspect knowledge --pack P --entry E [--pack-dir D ...]
kat inspect knowledge --pack P --path F [--pack-dir D ...]
kat inspect workflow --pack P --workflow W [--pack-dir D ...]
```

目标未知或明确目标不适用时，用 `kat inspect` 按 PACK 描述缩小范围，再对每个候选 PACK **必须先** `kat inspect knowledge --pack P` 查看知识入口列表，**确认无适用知识后才能** `kat inspect workflow --pack P` 查看用途列表。**禁止跳过知识检查直接按意图匹配 Workflow。** 按摘要读取候选正文或参数后选择，可以只用知识、只用 Workflow 或组合；不先排除其中一类、不加载全部正文、不扫描 Provider。已有且仍适用的 detail 直接复用，执行前不重复读取。`entries` 中 `disable-model-invocation: true` 的入口不由 agent 自动选用，`user-invocable: false` 的入口不向用户暴露为可调用选项；字段未设置时按默认可选用。

知识浏览是只读候选，不代表 Session 已采纳；需要引用才能判断适用性时按返回入口路径规范化 F，用 `--path` 浏览，不先创建 Session 采纳。Workflow detail 提供用途与参数，执行前不寻找 Guide；只用 `parameters` 明示的 `option` 和 bool `negative_option` 准备输入，不猜旗标，不把秘密放进可能进入日志的参数。

已有适用 Workflow 的程序能力优先复用，不能用通用工具重做或绕过其合同。通用工具只处理任务已授权材料或无适用 Workflow 的工作，不扩大外部访问、安装、执行或写入权限。

空列表、不适用、缺输入和 inspection 失败分别处理。两类均无匹配时说明能力缺口，不自动转入 PACK 创作或安装依赖。Workflow 发现因缺 Python 受限时，不声称不存在能力；仅当已发现知识明确足以处理当前材料、任务不依赖程序能力时继续，否则说明阻碍。

## 3. 初始化可保存任务

确定目标与可用能力后，新任务在首次采纳知识或执行前显式创建 Session 并保存目标：

```text
kat session create
kat analysis save --session S --expected-revision 0 --file <goal.json绝对路径>
```

创建成功只返回 `result.session_id`，不能自行指定、猜测或隐式复用 Session。`goal.json` 是 UTF-8 请求文件，例如：

```json
{"goal":{"question":"用户目标","scope":"范围","gaps":[]}}
```

save 成功返回更新后的记录概览，保留其 revision，无需回读确认。能力浏览不初始化任务；已有目标和 Session 时不重复创建。

## 4. 应用知识或执行 Workflow

### 采纳知识与引用

采用候选时，从已保存目标的 Session 取得提交后的快照，实际应用以返回原文为准，而非先前浏览副本：

```text
kat knowledge read --session S --pack P --entry E [--pack-dir D ...]
kat knowledge read --session S --pack P --path F [--pack-dir D ...]
```

成功回执有 `session_id,previous_revision,revision,material_id,material`；material 为 `kind="knowledge",pack,path,entry,content`，entry 是 `name,description` 或 null，content 保留原文。先持久化才返回；首次采纳 revision +1，同键唯一旧快照直接复用且不改 revision，不依赖当前 PACK。回执的 previous_revision 与此前已核对值不一致时，先进入[恢复规则](references/analysis-recovery.md)，不能直接沿用新数字。

将入口链接相对 `material.path` 解析并规范化为 PACK 根相对的 `/` 路径 F，再按需采纳引用；F 不含 `..`、反斜杠、尾随点/空格或逃逸 PACK 的最终目标，入口 SKILL.md 用 `--entry`。CLI 不遍历链接；只支持 UTF-8 文本，示例代码作为文本，不自动执行。PDF、图片和其他二进制不属于知识快照。

恢复已采用的方法时，按正文中的 ID 使用 `kat knowledge read --session S --material M` 或 `kat analysis show --session S --material M`，不要求当前 PACK 安装；Query 材料用 analysis show。同键多快照或用户明确换当前方法时读取[特殊情况处理](references/analysis-recovery.md)。未采纳引用及外部输入不保证历史可恢复，不能拿当前文件冒充旧依据。

### 执行、查询与解释 Run

在已准备的 Session 中运行选中能力：

```text
kat run --session S --pack P --workflow W [--pack-dir D ...] -- <detail明示的业务参数>
kat inspect run --session S --run R
kat query --session S --run R --sql <只读SQL>
```

`--session` 必须已有且有效，不由 run 隐式创建。成功 Run 自带自身 Guide、Output inventory 和直接子 Run；刚执行完直接复用，历史或缺信息时才 inspect run。程序捕获执行时 Guide，AI 不另行归档或用当前 PACK 正文替换。`guide: null` 是未声明，读取失败不是 null。

只查询真实 `output.*`。已有 inventory 时直接使用名称和 columns；缺少时通过 `information_schema.tables/columns` 核对，例如：

```sql
SELECT table_name, column_name, data_type, ordinal_position
FROM information_schema.columns
WHERE table_schema = 'output'
ORDER BY table_name, ordinal_position
```

SQL 范围及 JSON/NDJSON 读取遵循共享合同。`outputs: {}` 不可查询虚构 `output.main`，零行或无输出均不能单独证明没有问题。Guide 是解释方法，不是 Schema 或数据证据。

实际采用的关键查询用 `kat query --session S --run R --sql SQL --archive` 保存。普通查询结果之外，`result.analysis` 含 `session_id,material_id,previous_revision,revision`，程序保存实际 SQL、列和 NDJSON；不抄写行替代捕获，不归档整表。已有适用材料直接复用；采用未归档探索结果时，用更窄 SQL 归档必要事实并消费这次真实结果，不能冒称原查询的历史结果。核对修订回执，归档不替换解释或总报告。

父 Workflow 先用自身 Guide 与 Output 判断证据是否足够；需要子证据时才沿真实 child_runs 按需读取。用各子 Run 自己的 Guide 和输出，复用已保存适用解释，先保存必要子解释再汇总父解释，不遍历全部后代。多个同名子 Run 无公开事实可区分时说明歧义，不按 ID 顺序猜调用顺序。无输出父 Run 不等于没有问题。

Guide 建议的后续 Workflow 在同 Session 形成新的独立根 Run，不补写旧父子关系。AI 串联多个正式 Workflow 也只形成独立根 Run，不制造动态 Workflow、PACK 或父 Run；固定可复用组合交给作者实现。失败父调用可能留下独立成功的子 Run，可 inspect session 查看，不虚构失败父节点，也不在并发根执行时猜测失败调用的后代归属。

## 5. 保存判断与汇总

依据方法与实际材料形成判断，区分事实、推断、范围及缺口。证据不足时围绕缺口继续发现、补问或取证；证据足够即停止，不为补齐树执行。

必要进展及时保存，使用最近核对的 revision：

```text
kat analysis save --session S --expected-revision R --file <update.json绝对路径>
```

请求可含 goal、nodes 或 report，省略保持原样，不接受 null 删除。例如：

```json
{"nodes":[{"run_id":"R","content":"该 Run 的必要解释与真实依据"}],"report":{"content":"围绕用户问题的综合结论"}}
```

节点只引用本 Session 的真实 Run，首次保存即选入报告树；纯知识任务直接保存 report，不制造占位节点。树由程序依据真实父子关系派生，AI 不提交父引用或重挂；直接父未选入时挂报告根，父后来保存会自动归位，不跨过未选入的中间父。AI 继续运行的根 Run 保持并列，用正文说明取证原因。

正文必须自足，保留必要结论、事实出处、取证原因和限制，不依赖旧对话的“如上所述”。用户材料或其他已授权工具的事实不必变成 Run，但要说明出处与恢复限制。材料 ID、Session/Run 和实际 Output 用于追溯，不记录模型内部推理或额外选择图。保存成功后才作为已保留进展复用。

汇总处理不同范围、矛盾、重叠与缺口，不逐节点交付独立报告或拼接 Guide；同一证据避免重复计数。修改目标、方法或判断后检查受影响正文与总报告，程序不自动判断其有效性。修订冲突、保存结果不确定或格式问题按[恢复规则](references/analysis-recovery.md)，不盲目重提或重跑。

## 6. 交付结果

按任务状态交付足以复核的最少事实，不原样转发完整 Response、表、Guide 或日志：

- **已完成：** 直接回答问题；给出实际采用的证据出处与 Session ID，使用过知识才列材料 ID、执行过程序才列 PACK/Workflow、Run 和 Output；说明范围与不确定性、保存状态及可选后续方向。结论须与成功保存的总报告一致；已有适用报告无需重复保存，保存成功无需额外回读。
- **需要补充信息：** 先复用或恢复已有信息，再询问目标、材料或必需输入的最小缺项，不让用户选择知识还是 Workflow，不索要可由 inspection 得到的表结构。
- **执行失败或受阻：** 说明最后成功进展、缺口和最小下一步。区分“判断已形成”与“新报告保存未确认”，不能把部分输出或未保存报告说成完整可恢复。发现受限不等于能力不存在，不影响当前方法完成的限制仍如实说明。

不自动转入创作、安装或复核。知识、Run 发布、必要节点解释与总报告保存是不同事实；任务完成由目标和必要证据决定。用户明确永久删除已知 Session 时，先读[Session 删除规则](references/analysis-recovery.md#删除已知-session)，分析完成不构成删除授权。
