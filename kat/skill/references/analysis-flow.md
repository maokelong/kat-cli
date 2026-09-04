# 分析问题流程

## 1. 确认问题与起点

每次先确认一个要回答的问题，以及以下一种起点：

- 新分析：用户提供要分析的 Source、路径或其他业务输入；它如何进入系统由选中的 Workflow 与 Provider 决定，Agent 不预设统一中间数据模型。
- 已有 Session：用户提供 Session ID 时，用 `kat inspect session --session <Session ID>` 查看已发布 Run inventory，再选择相关 Run 或继续同一次分析。
- 已有 Run：用户提供 Session ID 与 Run ID。刚完成的 `kat run` 可以沿用成功 Response 的 `result.outputs`；只有双 ID 时，后续通过 Output Query 的 `information_schema` 发现实际 relation 与 columns。

缺少问题时只询问要回答什么。不要猜表名、读取 Run 内部文件，或为追问重新执行 Workflow。

## 2. 渐进发现 Workflow

对新分析按以下顺序发现能力：

1. 调用裸 `kat inspect`，只从 manifest 概要筛选少量候选 PACK。此步不加载 PACK Python。
2. 对候选调用 `kat inspect workflow --pack <名称>`，只比较按名称排序的 `name`、`description` 摘要。不要在筛选阶段读取所有 guide，也不要调用 Provider inspection。
3. 选定一个 Workflow 后，调用带 `--workflow <名称>` 的 Workflow inspection，读取唯一 detail 中的 `parameters` 和 `guide`。

`name` 与 `description` 用于索引和选择能力；`parameters` 决定运行时如何传入用户事实；`guide` 是选中 Workflow 的分析策略，指导如何解释可能结果、向哪些方向发散以及下一步查询什么。没有声明 guide 时值为 `null`，按 Workflow 的 description、parameters 和实际输出继续，不自行猜测 guide 文件路径。

唯一明确匹配时继续。候选会导向实质不同结论时，只提出一个最小必要澄清问题并说明差异。没有匹配时以受阻状态交付已发现的能力边界；可以建议新建或扩展 PACK，但未经用户明确授权不得修改源码或切换到作者流程。

已有 Run 已经选定 Workflow，不重新做全局能力筛选。调用 `kat inspect workflow --session <Session ID> --run <Run ID>` 取得当前 PACK 中该 Workflow 的 detail 和分析 guide。guide 不是 Run 快照；如果 PACK 已更新，应把它表述为当前分析策略，不声称它就是历史执行时的版本。

任一 inspection 失败时停止该分支，按 KAT Response 的 Diagnostic 交付，不通过扫描 PACK 源码、导入 Provider 或静态能力清单绕过失败。

## 3. 执行选中的 Workflow

新分析按 detail 的 `parameters` 构造 `kat run` 请求。省略 `--session` 会建立新 Session；后续操作只有在用户目标属于同一次分析时，才显式用先前成功 Response 的 `session_id` 调用 `kat run --session ...`。不存在隐式 current/last Session，也不要把 Run Output 自动作为后续 Workflow 输入。Workflow 自己显式选择和调用 Provider；分析 Agent 不 inspect Provider，也不依赖 Provider guide。

只有 Run Response 的 `status="success"` 时，才同时保留 `session_id`、`run_id`、输出名称、columns 与 `row_count`。它们是查询和继续分析阶段的执行事实。失败时没有可发布身份；不要把候选目录、日志或部分输出当作结果。

## 4. 查询最少证据

只使用 `kat query --session ... --run ... --sql ...` 查询 Workflow 输出。已有 `kat run` 的 `result.outputs` 时直接使用其中名称与 columns；只有双 ID 时，先查询 `information_schema.tables` 与 `information_schema.columns`，取得实际 `output.*` relation 与列，再形成证据 SQL。不要把 Workflow guide 当作 Output Schema。

先选择投影、过滤、聚合和排序；明细查询显式使用 `LIMIT`。KAT 不自动添加固定行数、字节数或超时限制，Agent 和用户负责查询规模、等待时间与本机资源消耗。Query 成功 Response 恰以 `result.format="ndjson"`、`result.path` 和 `result.columns` 描述结果；只读取该 Response 给出的 NDJSON 文件，并从其中保留回答当前问题所需的对象行作为证据。执行失败时根据 Diagnostic 修正或缩小 SQL，不读取 Run 文件、猜测结果路径或把失败包装成部分成功。

## 5. 使用策略形成结论

结合选中 Workflow 的 analysis guide 与主动约束范围的 Query 证据形成结果。guide 用于组织分析和下一步方向，不替代实际数据证据，也不扩大用户授权。

交付前读取 [result-contract.md](result-contract.md)。结论区分已观察事实、推断与不确定性；不要把完整 Response、完整表、原始 guide 或日志原样转发给用户。
