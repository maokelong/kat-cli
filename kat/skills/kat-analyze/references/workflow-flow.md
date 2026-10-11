# Workflow 取证

本文件只规定执行、查询和解释真实 Run 的操作；目标、能力选择、Session 初始化、保存与恢复遵循[共同分析流程](analysis-flow.md)，不是另一条任务生命周期。

## 执行与结果

选定候选后，用 `inspect workflow --pack P --workflow W` 读取用途和参数，按合同准备输入；执行前不寻找 Guide。使用已保存目标的 Session 执行 `kat run --session S --pack P --workflow W -- ...`，只采用成功 Response 的身份、Guide、Output 元数据和直接子 Run ID。程序捕获 Guide 并随成功 Run 发布，AI 不另行归档或以当前 PACK 正文替换它。

执行失败不把日志或候选目录当作成功结果。父失败时已成功发布的子 Run 仍可从 Session inventory 找到；不虚构失败父节点，也不凭失败文本猜测调用顺序。

## 查询并归档关键证据

只用 `kat query --session S --run R --sql SQL` 查询公开 Output。优先使用已有 `result.outputs`，必要时通过 `information_schema.tables` 和 `information_schema.columns` 核对结构；Guide 不代替 Schema 或数据证据。`outputs: {}` 不可查询虚构的 `output.main`，无输出和零行均不能单独证明没有问题。

SQL 显式投影、过滤、聚合和排序，明细查询加 `LIMIT`；读取成功 Response 的 `result.path` 获取真实 NDJSON，不扫描私有目录，不用日志、Provider 知识或未执行 SQL 支撑结论。

实际采用的关键查询证据通过 `--archive` 保存，已有适用材料直接复用。采用尚未归档的探索结果时，用更窄的 SQL 归档必要证据，再读取这次真实结果；它是新查询，不能冒称原探索查询的历史结果。不归档整表，不抄写行替代程序捕获。修订回执按共同流程核对，归档不替换任何解释或总报告。

## 解释 Run 与按需使用子证据

刚执行完直接使用返回的自身 Guide 和 Output；历史或子 Run 信息缺失时，用 `inspect run --session S --run R` 读取执行时快照，不依赖当前 PACK 或猜测文件路径。`guide: null` 表示未声明指南，读取失败不是 null。无 Guide 不必强制形成独立解释，可依据实际事实分析并说明限制。

需要保存的 Run 解释只用一个 `content` Markdown 正文，保留足以恢复判断的结论、真实出处、必要取证原因与限制。仅在已实际执行且需要保留解释时保存节点；首次保存即选入报告树，不为成功 Run 保存占位记录。

AI 根据 R1 的解释继续运行 R2 时，两者是报告根下并列节点，后者正文按需说明参数与取证原因，不把时间先后或知识入口变成程序调用边。

父 Workflow 使用 `ctx.run()` 时，先以父 Guide 与父 Output 判断证据是否足够；需要子结论才沿真实 `child_runs` 按需读取，不强制遍历所有后代。复用已有适用子正文，或先形成并保存子解释，再保存父解释。树由程序按真实直接父关系派生，直接父未选入时子节点挂报告根，父后来保存后自动归位，不跨越未选中的中间父节点；AI 不提交父引用或重挂操作。

Guide 建议后续取证时，在同 Session 执行新的 Workflow，不补写旧 Manifest 关系。正文可引用同 Session 其他 Run 的材料，不因此强制选入所有来源节点；同一证据支持多个结论时避免重复计数。必要 Run 解释保存成功后再供后续分析复用，共同保存与交付规则不在此重复。
