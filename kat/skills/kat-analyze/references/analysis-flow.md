# 分析流程

一个 Session 对应一个分析目标和一份当前 Analysis Record。Run 保存执行时 Guide、Output 和直接子 Run；分析记录保存目标、按 Run 归属的正文、关键查询材料及总报告。命令与 JSON 字段以[公共命令合同](../../kat/references/command-reference.md)为准，必要保存点以 [Skill 主流程](../SKILL.md#问题分析流程必须遵守)为准。

## 1. 继续当前分析或按需恢复

当前上下文已经保留相关解释、证据出处和成功保存结果时，直接继续，不为汇总或确认成功再读记录。判断信息是否足够，不要求检测是否发生过上下文压缩；压缩摘要仍足够时也不必重读。

新任务只有 Session ID，或当前缺少继续所需信息时，用 `kat analysis show --session S` 恢复目标、revision、报告树和正文。不要先要求用户重述原问题或提供旧报告。需要核对某个节点或原始查询时，按需使用 `--run R` 或 `--material M`。已有报告经 AI 判断仍适用即可交付，不重新保存或执行 Workflow。

明确没有记录时，可以从已知目标和 `inspect session` 的现存 Run 建立记录；没有记录不能还原未保存的旧结论。损坏或未知格式不能当作无记录覆盖。只有 Run ID 且无法确定 Session 时，才询问所属 Session，不跨目录搜索。恢复不扩大来源授权；只读请求不创建记录、执行 Workflow 或归档材料。

## 2. 选择并执行 Workflow

1. `kat inspect` 发现 PACK，再以 `kat inspect workflow --pack P` 比较用途。只从 Workflow 知识发现分析能力，不扫描 Provider。
2. 选定后用 `kat inspect workflow --pack P --workflow W` 读取用途和参数，确认输入。执行前需要知道的输入语义以此为准，不提前寻找分析 Guide。
3. 新分析创建 Session，以 `analysis save --session S --expected-revision 0 --file goal.json` 保存已明确的目标，成功后执行。后续取证复用同一 Session；独立新问题新建 Session。能力浏览不创建记录。
4. 执行 `kat run --session S --pack P --workflow W -- ...`。只采用成功 Response 中的身份、Guide、Output 元数据和直接子 Run ID。程序自动捕获 Guide 并随 Run 发布，不依赖分析记录，也不需要 AI 另行归档。

没有匹配 Workflow 时说明能力边界；未经用户明确授权不切换 PACK 创作。执行失败不能把日志或候选目录当作成功结果。父失败时，已成功发布的子 Run 仍可从 Session inventory 找到；不虚构失败父节点，也不凭失败文本猜测调用顺序。

## 3. 按需解释 Run 和组织报告树

刚执行完直接使用 `kat run` 返回的自身 Guide 与 Output，不再重复读取。历史或子 Run 的信息缺失时，用 `kat inspect run --session S --run R` 读取执行时快照。这个读取不依赖当前 PACK；不能用当前 PACK 的内容补造旧 Guide，也不猜测文件路径。`guide: null` 明确表示未声明指南，读取失败不能视为 `null`。

### AI 临时串联

AI 解释 R1 后根据发现运行 R2，两份正文在报告根下并列：

```text
用户问题
├── R1：解释正文
└── R2：解释正文，按需说明根据 R1 的什么发现继续取证
```

解释形成后保存正文，再用它选择后续 Workflow。必要的取证原因、参数来源或采用其他结论的说明自然写入正文，不额外登记选择信息或结论依赖。时间先后不形成程序父子边，相同证据也不证明因果关系。

### 父 Workflow 使用 ctx.run()

先使用父 Run 自身的 Guide 和 Output 判断证据是否足够。需要子结论时，沿返回的直接 `child_runs` 按需读取子 Run，各自使用自己的 Guide 与最少证据；更深层同样按需展开，不强制遍历全部后代。

```text
用户问题
└── R0：父解释，按需综合子证据或已保存的子解释
    ├── R1：子解释
    └── R2：子解释
```

需要采用子解释时，先复用已有适用正文，或形成并保存子解释，再保存父解释。首次保存某 Run 正文即选入报告树，不为刚执行成功的 Run 保存占位记录。程序依据真实直接父关系安排节点；直接父尚未选入时，子节点挂报告根，父后来保存后自动归位，不跨越未选中的中间父节点。AI 不提交父引用或重挂操作。

`outputs: {}` 不可查询虚构的 `output.main`；无输出和零行都不能单独证明没有问题。无 Guide 的 Run 不必强制形成独立解释。Guide 建议继续取证时，在同 Session 执行新的 Workflow，必要出处写入正文，不补写既有调用关系。同一证据支持多个结论时避免重复计数。

## 4. 查询并归档关键证据

只用 `kat query --session S --run R --sql SQL` 查询公开 Output。优先使用已有 `result.outputs`；必要时通过 `information_schema.tables` 和 `information_schema.columns` 核对实际结构。Guide 是分析方法，不代替 Schema 或数据证据。

SQL 显式投影、过滤、聚合和排序，明细查询加 `LIMIT`。读取成功 Response 的 `result.path` 获取真实 NDJSON 行；不扫描私有目录，不用日志、Provider 知识或未执行 SQL 支撑结论。

实际采用的关键查询证据须通过 `--archive` 保存；已有适用材料直接复用。探索后决定采用尚未归档的结果时，用更窄的 SQL 归档必要证据，再读取这次真实结果。程序保存实际 SQL、Run 来源、列和少量原始 NDJSON，返回材料 ID；这是新查询证据，不能冒称原探索查询的历史结果。不把整表作为关键证据，也不抄写行替代程序归档。

查询材料归档会改变分析记录 revision。只有回执的 `previous_revision` 等于先前已读或自行保存的 revision，才能直接沿用回执的新 revision；否则先 `analysis show` 核对并发修改。归档本身不替换任何解释或总报告。

## 5. 保存正文并汇总

每个 Run 只保存一个 `content` Markdown 正文。写清足以恢复分析的结论、关键事实、真实证据出处、必要取证原因、范围和未解决问题；不依赖旧对话中的“如上所述”，不要求固定标题或结构化解释字段，也不保存模型内部推理过程。材料可来自同 Session 的其他 Run，正文保留真实出处，不必因此把材料来源全部选入报告。

解释形成后及时用 `analysis save` 保存，成功后再供后续分析复用。若需保留部分判断，正文可记录实际进展与缺口。省略的节点、目标和报告保持原样，更新某节点不自动删除旧总报告；程序不判断正文是否仍适用，AI 须根据目标、证据和新发现复核。

汇总围绕用户问题处理结论间的范围差异、矛盾、重叠证据和缺口。当前上下文足够时，直接使用已成功保存的适用解释形成总报告，不为汇总再调用 `analysis show`；缺少信息时才恢复。无需为每个节点交付独立子报告、创建总 Workflow 或拼接 Guide。

## 6. 并发、保存失败与交付

每次保存使用最近已核对的 revision；首次创建用 0。冲突先重读并核对实际修改，再决定如何保存，不能只换数字重提。保存响应不确定时读取记录确认，不盲目重复保存、归档或重跑 Workflow。记录损坏不作为缓存丢弃，失败不能覆盖上次完整内容。

交付前读取[结果契约](result-contract.md)，保存总报告正文。正常成功 Response 已确认保存，不再安排一次回读；交付结论与该正文一致，并提供 Session ID。保存未确认时说明已形成判断与保存受阻，不承诺新报告可恢复。证据足够即停止，不为补齐树执行更多 Workflow，也不自动触发独立复核。
