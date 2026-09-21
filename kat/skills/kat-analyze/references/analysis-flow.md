# 分析问题流程

一个 Session 对应一个分析目标和一份当前 Analysis Record。记录保存所选 Run 的组织、AI 选择依据、节点解释、原 Guide、关键查询证据和总报告；程序运行仍只产生不可变 Run。命令、JSON 请求与 Response 字段以[公共命令合同](../../kat/references/command-reference.md)为准。

## AI 必须执行的时机

下表适用于已授权的新分析和继续分析，由 AI 自动执行，不等待用户另行要求保存。普通能力浏览不初始化记录；恢复已有有效报告可以直接交付，不重复写入。

| 触发时机 | 必做操作与完成条件 |
|---|---|
| 接到已有 Session 的分析或恢复请求 | 先 `analysis show` 读取目标、进展和 revision，再决定是否需要取证；不先要求用户重述问题。 |
| 新分析已选定首个 Workflow | `session create` 后 `analysis init`；成功后才读取并归档 detail、执行 Workflow。 |
| 读取将用于解释的 Guide，或查询将用于解释的关键证据 | 分别使用 `--archive-to-session`、`--archive` 取得材料 ID；已有适用材料直接复用，探索查询不必全部归档。 |
| 成功 Run 被选用，包括按需展开的程序子 Run | 通过 `analysis update` 的 `select_node` 及时选入；AI 串联时同时保存已知 `set_selection`，再继续分析。 |
| 形成或修订一个节点解释 | 用 `set_interpretation` 保存并取得实际版本，再复用它选择后续 Workflow 或形成依赖它的解释；消费者按实际依赖分批保存。 |
| 同一目标的范围或已知缺口改变 | 用 `set_goal` 保存进展，重新核对受影响的解释；需要补问或暂时受阻时也保留已经形成的进展。 |
| 准备交付新报告或更新后的报告 | 用 `set_report` 保存正文与实际采用的解释版本，确认成功返回的报告为 `current` 后交付该版本；保存失败按第 6 节说明受阻。 |

AI 按公共命令合同编写 UTF-8 JSON 请求文件，通过公开 CLI 提交，不直接编辑 Session 内部记录。各步骤使用成功 Response 中的完整 ID、材料和版本；并发冲突或保存响应不确定时按第 6 节恢复，不让用户代为管理这些状态。

## 1. 从目标或已保存记录开始

- 新分析：先明确用户问题和来源范围，按第 2 节选择已有 Workflow。独立新目标使用新 Session，追问和补证继续当前目标。
- 已有 Session：先执行 `kat analysis show --session S`，恢复 `goal`、`revision`、有序 `nodes` 和 `report`。用户只给 Session ID 时不要先要求重述原问题、提供旧报告或选择 Run。
- 已有 Run：仍先读取所属 Session 的记录；需要继续这个 Run 时按需读取 `analysis show --session S --run R`。只给 Run ID 且上下文不能确定 Session 时，才询问所属 Session ID；不跨目录搜索。

恢复已有有效报告或解释时，直接使用保存的内容；需要核对依据时按材料 ID 执行 `analysis show --session S --material M`。此路径不要求当前 PACK 仍安装，也不要求普通 Query Result 文件仍在，不重新 inspection 当前 Guide 或重跑 Workflow。

没有记录与记录损坏不同。明确没有记录时，可以用 `kat inspect session --session S` 找到现存 Run，并在已知目标下初始化记录、重新选入证据；不能声称恢复了过去的 AI 解释或补造原选择依据。记录损坏、未知格式或其他失败时按 Diagnostic 停止该分支，不初始化覆盖。记录存在但缺少解释或总报告时，从最后保存的进展继续。

`current` 只表示登记的依赖未失效。复用前仍要确认解释覆盖当前问题、统计范围和已知限制；若用户要求按新策略重新分析，才读取新 Guide 并更新解释。恢复不自动扩大来源授权。

## 2. 选择 Workflow 并保存执行前 Guide

新分析按顺序渐进发现：

1. 裸 `kat inspect` 从 PACK manifest 概要筛选候选；此步不加载 PACK Python。
2. `kat inspect workflow --pack P` 比较 `name`、`description`，不在筛选阶段读取全部 Guide 或扫描 Provider。
3. 选定首个 Workflow 后，调用 `kat session create` 保存成功返回的 `session_id`；将已明确的 `question`、`scope`、`gaps` 写入 JSON，通过 `kat analysis init --session S --file goal.json` 保存目标与初始 revision。
4. `kat inspect workflow --pack P --workflow W --archive-to-session S` 读取参数和 Guide，同时保存此次实际正文。沿用成功 Response 的 `result.workflow` 与 `result.analysis.material_id`，按第 6 节处理 revision。

同一分析选择后续 Workflow 时复用 Session/记录，不重复初始化。只有唯一匹配时直接继续；候选会导向实质不同结论时，只询问一个最小必要问题。没有匹配时说明能力边界，未经授权不转入创作或修改 PACK。

Guide 快照可以先属于 Session 和 Workflow，尚无 Run；执行成功后再由该节点引用。不要在形成解释时重新读取可能变化的 Guide 并冒称是先前版本。没有 Guide 时保存 `guide=null` 的材料，依据 description、参数和实际输出继续，不猜 Guide 文件路径。

既有 Run 需要新的解释且没有适用的已存 Guide，或明确采用新策略时，使用 `kat inspect workflow --session S --run R --archive-to-session S`。它取得当前 PACK 的知识，必须作为新依据表述；inspection 失败时不扫描源码或私有文件绕过失败。

## 3. 执行并选入报告节点

按 detail 的 `parameters` 形成 `kat run --session S --pack P --workflow W -- ...`。不自动把 Run Output 当作下一 Workflow 的输入，不 inspect Provider。只有 `status="success"` 时使用顶层 `session_id`、`run_id` 和 `outputs`；随后用 `select_node` 选入 Run，保存进展，不能等整份分析结束才记录。

报告根是用户问题，没有 Workflow/Run 身份。记录只有一份有序 `nodes`：`run_id` 唯一，`report_parent=null` 挂报告根，非空值指向已选入的真实直接程序父 Run。同父节点的相对顺序就是报告顺序；没有另一份持久 `tree` 或 `choices`。

### AI 临时串联

AI 运行 R1、解释其证据，再据此选择 R2 时，将 R1、R2 分别挂报告根。为 R2 保存 `selection`：来源 Run/材料、选择当时的简短发现、目的与各输入的来源。它记录已知的选择依据，不是内部思考全文，也不把时间先后改成程序父子边。

```text
报告根：用户问题
├── R1：已有解释
└── R2：selection 记录根据 R1 的什么发现继续取证
```

若 R2 的结论实际使用 R1 的解释，另在 `interpretation.uses` 引用 R1 的解释版本。仅因为 R1 促成了这次调用，不自动构成解释依赖。来源证据相同也不意味着统计口径相同或存在因果关系。

### 父 Workflow 使用 ctx.run()

父程序完成后选入 R0。先用其已归档 Guide 与自身 Output 判断所需证据；只有父 Guide 要求汇总子结论或父证据不足时，才从 Session inventory 读取 R0 的 `child_runs`，选入所需子节点并将 `report_parent` 指向 R0。更深层同样按需展开，程序执行期间不等待 AI。

```text
报告根：用户问题
└── R0：父 Workflow
    ├── R1：R0.child_runs 可证实的直接子 Run
    └── R2：R0.child_runs 可证实的直接子 Run
```

对必要子节点分别复用有效解释，或归档它自己的 Guide、查询证据并保存解释；再用父级自己的 Guide/Output 与实际采用的子解释汇总。`outputs: {}` 不可查询虚构的 `output.main`；无输出和零行都不能单独证明没有问题。没有 Guide 的 Run 不必强制生成独立解释。

`child_runs` 是按 Run ID 排序的无序关系集合，不能据数组位置猜调用次序或角色。同 Workflow 的多个子 Run 无法从公开事实区分时，保留候选和歧义，不按 Workflow 名合并节点。

子 Run 可以先独立挂报告根，后来选入真实父 Run 时用 `select_node` 重挂原节点，保留解释和选择依据。`report_parent=null` 不证明它当初由 AI 顶层调用；`selection=null` 只表示未记录依据。同一证据支持多个结论时使用引用，不复制 Run 节点或重复计数。

Guide 要求后续补证时，确认参数来源、授权范围和预期新增证据，在同 Session 发起新的顶层 Run，默认挂报告根；相关父解释通过证据或 `uses` 引用它，不补写既有 Manifest。

Run 失败时没有本次顶层 Run 的成功身份；Session 与已发布子 Run 仍可能存在。可用 Session inspection 查询已知事实，但不补造失败父节点，也不把并发产生的无父 Run 猜成某次失败调用的后代。保存已确认缺口，不自动重复相同输入。

## 4. 查询并归档最少证据

只用 `kat query --session S --run R --sql SQL` 查询公开 Output。刚执行完时沿用 `result.outputs`；只有双 ID 时先查 `information_schema.tables` 和 `information_schema.columns`，再构造实际 `output.*` 查询。Guide 不代替 Schema。

查询先投影、过滤、聚合和排序，明细显式 `LIMIT`；KAT 不自动限制规模或等待时间。读取成功 Response 的 `result.path`，不猜路径、扫描 `query-results/` 或读取 Run 私有文件。

用于保存解释的关键查询证据必须有归档材料 ID；已有适用材料直接复用。探索后需要采用尚未归档的结果时，用更窄的 SQL 加 `--archive` 固定必要证据。程序保存这次实际 SQL、列、Run 来源和完整的少量 NDJSON 行，成功返回 `result.analysis.material_id`。先读取这次真实结果再作判断；它是新证据，不能冒称原探索查询的历史结果。不要把整表作为“关键证据”，也不要抄写行或重新序列化数值提交给 `analysis update`。

恢复时读取材料 ID 对应的原文即可；只有需要额外事实时再查询。新增材料本身不改变旧解释的依据，也不自动使全部旧结论失效。

## 5. 保存解释，再汇总一份报告

节点解释保存简短 `facts`、`conclusion`、`scope`、`limitations`，以及实际使用的 `guide` 材料 ID、`evidence` 材料 ID 列表和 `uses`。Guide 必须对应节点 Workflow；证据可以来自同 Session 的其他 Run，保留真实出处，引用材料不强制把来源 Run 选入报告。没有自己的输出表时，可以使用必要子证据或已保存子解释。

用 `set_interpretation` 保存后，从返回记录读取 CLI 分配的 `interpretation_version` 和状态。材料 ID 标识原依据；解释版本标识该节点结论；记录 `revision` 用于并发控制，三者不能互换。

按实际依赖自底向上提交：先子解释，再消费者解释，最后 `set_report`。不要在一次更新中引用这次才会分配的新解释版本，或保存仍采用本次被替换旧版本的新结论。没有实际解释依赖的节点无需等待全部树叶；不允许相互引用尚待证明的结论形成循环佐证。

报告根围绕用户问题综合仍适用的已存结论，处理口径差异、矛盾、重叠证据和缺口，保存正文及实际采用的解释引用。无需为每个节点交付一份完整子报告，也不需要创建总 Workflow 或拼接总 Guide。

更新一个解释后，实际依赖它的旧消费者和总报告会失效；跨分支与子树依赖遵循同一规则。修改报告位置/顺序使旧总报告失效，不自动否定局部解释；调整问题范围后重新核对现有解释。缺少或失效的必要解释补齐并保存后，再更新上层汇总。

## 6. 修订、失败与交付

每次 `analysis update` 带先前读到的 `--expected-revision`。冲突时重新 `analysis show`，比较目标、节点和依赖后再形成更新，不能只把数字换新就盲目重提。

显式归档成功返回 `result.analysis.{session_id,previous_revision,revision,material_id}`。只有 `previous_revision` 等于自己先前读/写的 revision 时，才可沿用返回的新 revision；否则可能有别的任务已修改记录，必须重新读取并核对。不能让归档返回的新数字绕过并发检查。

保存失败不声称本次更新可恢复。响应不确定时重新 `show` 核实是否已经提交，不能盲重提解释或重跑 Workflow；正式记录损坏不当作可丢弃缓存。材料归档失败也不能把查询已完成声称为材料已保存。

追问、缺口和未解释节点也保存为进展。总报告保存成功后才确认该版本可凭 Session ID 恢复；恢复已有有效报告可以直接交付，无需重复保存。交付前读取[结果契约](result-contract.md)，不原样转发完整 Response、原 Guide、全表或日志。证据足够即停止，不自动触发独立复核。
