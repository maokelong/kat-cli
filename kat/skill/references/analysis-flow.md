# 分析问题流程

## 1. 确认问题与起点

每次先确认一个要回答的问题，以及以下一种起点：

- 新分析：用户提供要分析的 Source、路径或其他业务输入；它如何进入系统由选中的 Workflow 与 Provider 决定，Agent 不预设统一中间数据模型。
- 已有 Run：用户提供 Run ID 和 Run Output 元数据，其中至少包含输出名称与 columns。刚完成的 `kat run` 可以沿用成功 Response 的 `result.outputs`；用户也可以提供同等元数据。

缺少问题时只询问要回答什么；只有 Run ID、没有 Output 元数据时只请求输出名称与 columns。不要猜表名、读取 Run 内部文件，或为追问重新执行 Workflow。

## 2. 渐进发现 Workflow

对新分析按以下顺序发现能力：

1. 调用裸 `kat inspect`，只从 manifest 概要筛选少量候选 PACK。此步不加载 PACK Python。
2. 对候选调用 `kat inspect workflow --pack <名称>`，只比较按名称排序的 `name`、`description` 摘要。不要在筛选阶段读取所有 guide，也不要调用 Provider inspection。
3. 选定一个 Workflow 后，调用带 `--workflow <名称>` 的 Workflow inspection，读取唯一 detail 中的 `parameters` 和 `guide`。

`name` 与 `description` 用于索引和选择能力；`parameters` 决定运行时如何传入用户事实；`guide` 是选中 Workflow 的分析策略，指导如何解释可能结果、向哪些方向发散以及下一步查询什么。没有声明 guide 时值为 `null`，按 Workflow 的 description、parameters 和实际输出继续，不自行猜测 guide 文件路径。

唯一明确匹配时继续。候选会导向实质不同结论时，只提出一个最小必要澄清问题并说明差异。没有匹配时以受阻状态交付已发现的能力边界；可以建议新建或扩展 PACK，但未经用户明确授权不得修改源码或切换到作者流程。

已有 Run 已经选定 Workflow，不重新做全局能力筛选。调用 `kat inspect workflow --run <Run ID>` 取得当前 PACK 中该 Workflow 的 detail 和分析 guide。guide 不是 Run 快照；如果 PACK 已更新，应把它表述为当前分析策略，不声称它就是历史执行时的版本。

任一 inspection 失败时停止该分支，按 KAT Response 的 Diagnostic 交付，不通过扫描 PACK 源码、导入 Provider 或静态能力清单绕过失败。

## 3. 执行选中的 Workflow

新分析按 detail 的 `parameters` 构造 `kat run` 请求。Workflow 自己显式选择和调用 Provider；分析 Agent 不 inspect Provider，也不依赖 Provider guide。

只有 Run Response 的 `status="success"` 时，才保留 `run_id`、输出名称、columns 与 `row_count`。它们是查询阶段的执行事实。失败时没有可发布 Run；不要把候选目录、日志或部分输出当作结果。

## 4. 查询最少证据

根据已保留或用户提供的 Run Output 名称与 columns 构造完整 SQL，并只使用 `kat query --run ... --sql ...` 查询 Workflow 输出。先选择投影、过滤、聚合和排序；明细查询显式使用 `LIMIT`。每次只取得回答当前问题所需的 columns 与 rows。

KAT 不自动添加固定行数、字节数或超时限制；Agent 和用户负责查询规模、等待时间与本机资源消耗。Query 成功后保留 `result.columns` 与 `result.rows` 作为证据。执行失败时根据 Diagnostic 修正或缩小 SQL，不读取 Run 文件，也不把失败包装成部分成功。

## 5. 使用策略形成结论

结合选中 Workflow 的 analysis guide 与主动约束范围的 Query 证据形成结果。guide 用于组织分析和下一步方向，不替代实际数据证据，也不扩大用户授权。

交付前读取 [result-contract.md](result-contract.md)。结论区分已观察事实、推断与不确定性；不要把完整 Response、完整表、原始 guide 或日志原样转发给用户。
