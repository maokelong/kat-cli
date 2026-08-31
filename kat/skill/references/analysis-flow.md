# 分析数据流程

## 1. 确认起点和问题

每次任务从本地来源输入，或一个已有 Run 与 Run Output 元数据开始。没有分析问题时，询问用户要回答什么；没有可定位的起点时，只询问来源输入或 Run 中缺少的一项。不要隐式组合多个来源或推断跨来源比较语义；只有选定 Workflow 的 Interface 明确接收多个输入时，才按用户目标传入它们。

- 来源输入：保留用户提供的准确来源格式和路径，进入第 2 步。来源路径只是候选 Workflow 的普通显式参数；KAT 没有独立的来源导入或 inspection 操作。没有已发现 Workflow 接纳该格式时，以受阻状态说明边界，不改写格式，也不把文件读取成功表述为完成分析。
- Run：需要 Run ID、Output 名称和 columns。刚完成的 Run 可以沿用成功 Response 的 `result.outputs`；用户也可以提供同等元数据。只有 Run ID 时，一次只询问缺少的 Output 名称和 columns，并停止在查询前；不得猜名称、读取 Run 内部文件或重新执行 Workflow。元数据齐全后进入第 4 步。

任一操作的 Response 不是 `status=success` 时停止该分支，保留 Diagnostic、可用 `log_path` 和可执行帮助，按结果契约交付。

## 2. 发现并选择分析能力

对来源输入：

1. 无目标调用 `kat inspect`，只从成功 Response 的 PACK 概要筛选少量候选。
2. 对候选调用 `kat inspect --pack`，读取 Workflow 的用途和 `parameters`。
3. 只选择参数能够显式表达当前来源位置、所需 selector 和分析控制值的 Workflow。Inspection 不证明来源内容有效；来源准入由 Workflow 显式调用的 PACK-owned Provider 负责。

唯一明确匹配时继续。候选会导向实质不同分析结论时，只提出一个最小必要澄清问题，说明每个选择的差异。其他差异采用可说明的默认选择。

没有匹配时，以受阻状态交付已检查的能力边界；可以建议新建或扩展 PACK，但不得修改源码或切换作者流，除非用户明确要求。

## 3. 执行 Workflow

按 inspection 返回的 Workflow Interface 构造 `kat run`：选择 PACK、Workflow 和必要的 `--pack-dir`，再在 `--` 后传入 inspection 明示的参数。来源路径、多个来源 selector 和其他输入都由 Workflow 参数显式表达，不存在平台来源 selector。

只有 `status=success` 时，才从 Response 保留 `run_id`、Output 名称、columns 与 `row_count`；这些是下一步查询的唯一执行事实。不要把私有 Run 文件或未发布候选带入对话。Workflow 失败时不发布 Run，不能把候选目录、日志或部分 Output 当作可查询结果。

## 4. 用主动约束范围的查询取得证据

对已发布 Run，先根据已保留或用户提供的 Run Output 名称与 columns 构造完整 SQL，再只使用只读的 `kat query --run ... --sql ...`。SQL 只能引用当前 Run 的 `output.<name>`；先选择投影、过滤、聚合和排序，明细查询必须显式使用 `LIMIT`。不要尝试访问 PACK、Datasource、其他 Run 或历史 Manifest 字段。

KAT 不会自动添加固定行数、字节数或超时限制；调用方和用户负责查询规模、等待时间与本机资源消耗。Query 成功后必须验证 `result.format == "ndjson"`，使用 `result.columns` 解释 `result.path` 指向的单文件 NDJSON：每个非空行是一个使用查询列名的 JSON object，零行是空文件。只保留回答当前问题所需的列和行作为证据；行数据不在 Response 中。

实际执行失败时根据 KAT Response 的 Diagnostic 缩小 SQL 范围；不要在返回后静默截断、读取 Run 内部 Output 文件或把失败包装成部分成功。

## 5. 形成交付

从 Run 中由调用方主动约束范围的 Query 证据形成 Analysis Result，并在交付前读取 [result-contract.md](result-contract.md)。结论必须区分已观察事实、推断和不确定性；不要把 KAT Response、完整 NDJSON 或日志原样转发给用户。
