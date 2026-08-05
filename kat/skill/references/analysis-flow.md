# 分析数据流程

## 1. 确认起点和问题

每次只接受一个预发布 Source、一个 Dataset，或一个已有 Run 与 Run Output 元数据。没有分析问题时，询问用户要回答什么；没有可定位的起点时，询问 Source、Dataset 或 Run 中的一个。不要隐式组合多个输入或推断跨输入比较语义。

- Source：当前只在用户明确要求试用或验证时接受本地 Trace Streamer SQLite。先说明它是 Deprecated 预发布入口、Schema 和 PACK 不构成生产兼容承诺；用户仍确认后调用私有 `kat import trace-streamer`，成功后仅从 KAT Response 的 `result.path` 取得 Dataset path，随后调用 `kat inspect --dataset`，再进入第 2 步。`.htrace` 的长期 Bundled Workflow 尚未就绪；收到 `.htrace` 分析请求时以受阻状态说明边界，不擅自改用 Trace Streamer，也不把仅导入成功表述为完成分析。
- Dataset：调用 `kat inspect --dataset`，再进入第 2 步。
- Run：需要已有 Run 与 Run Output 元数据，其中至少包含输出名称和 columns。刚完成的 Run 操作可以沿用成功 Response 的 `result.outputs`；用户也可以直接提供同等元数据。只提供 Run ID 时，一次只询问缺少的输出名称和 columns，并停止在查询前；不得猜测名称或读取 Run 内部文件，且不得重新导入或运行 Workflow。元数据齐全后进入第 4 步，再进入第 5 步。

对 Source 或 Dataset 的 Dataset inspection，只有 `status=success` 时才保留 Response 的 `path`、tables 与 schema；它们是第 2 步判断 Required tables 的唯一 Dataset facts。

任一操作的 Response 不是 `status=success` 时停止该分支，保留 Diagnostic、可用 `log_path` 和可执行帮助，按结果契约交付。

## 2. 发现并选择分析能力

对已检查 Dataset：

1. 无目标调用 `kat inspect`，只从成功 Response 的 PACK 概要筛选少量候选。
2. 对候选调用 `kat inspect --pack`，读取 Workflow 的用途、参数与 `required_tables`。
3. 只有 Required tables 是 Dataset 实际 tables 子集的 Workflow 才可执行。

唯一明确匹配时继续。候选会导向实质不同分析结论时，只提出一个最小必要澄清问题，说明每个选择的差异。其他差异采用可说明的默认选择。

没有匹配时，以受阻状态交付已检查的 Dataset facts 和能力边界；可以建议新建或扩展 PACK，但不得修改源码或切换作者流，除非用户明确要求。

## 3. 执行 Workflow

按 inspection 返回的 Workflow Interface 构造内部 `kat run` 请求。只有 `status=success` 时，才从 Response 保留 `run_id`、输出名称、columns 与 `row_count`；这些是下一步查询的唯一执行事实。

不要把完整输出、私有 Run Output ID、Run 内部文件或未发布候选带入对话。Workflow 失败时不发布 Run，不能把候选目录、日志或部分输出当作可查询结果。

## 4. 用主动约束范围的查询取得证据

对已发布 Run，先根据已保留或用户提供的 Run Output 名称与 columns 构造完整 SQL，再只使用只读的 `kat query --run ... --sql ...`。先选择投影、过滤、聚合和排序，明细查询必须显式使用 `LIMIT`；每次只取得回答当前问题所需的 columns 与 rows。

KAT 不会自动添加固定行数、字节数或超时限制；调用方和用户负责查询规模、等待时间与本机资源消耗。Query 成功后保留其 columns 与 rows 作为证据；实际执行失败时根据 KAT Response 的 Diagnostic 缩小 SQL 范围，不要在返回后静默截断、读取 Run 文件或把失败包装成部分成功。

## 5. 形成交付

从 Run 中由调用方主动约束范围的 Query 证据形成 Analysis Result，并在交付前读取 [result-contract.md](result-contract.md)。结论必须区分已观察事实、推断和不确定性；不要把 KAT Response、完整表或日志原样转发给用户。
