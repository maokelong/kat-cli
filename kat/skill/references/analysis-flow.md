# 分析数据流程

## 1. 确认起点和问题

每次只接受一个 Source、一个 Dataset 或一个 Run。没有分析问题时，询问用户要回答什么；没有可定位的起点时，询问 Source、Dataset 或 Run 中的一个。不要隐式组合多个输入或推断跨输入比较语义。

- Source：正常分析只接受本地 `.htrace`。调用私有 `kat import hitrace`，成功后仅从 KAT Response 的 `result.path` 取得 Dataset path；随后调用 `kat inspect --dataset`，再进入第 2 步。
- Dataset：调用 `kat inspect --dataset`，再进入第 2 步。
- Run：直接进入第 4 步，再进入第 5 步；不得重新导入或运行 Workflow。

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

不要把完整输出、私有 Output ID、Run 内部文件或未发布候选带入对话。Workflow 失败时不发布 Run，不能把候选目录、日志或部分输出当作可查询结果。

## 4. 用有界查询取得证据

对已发布 Run 只使用只读的 `kat query --run ... --sql ...`。先选择投影、过滤、聚合、排序，并显式限制少量明细；每次只取得回答当前问题所需的 columns 与 rows。

Query 成功后保留其 columns 与 rows 作为证据。Query 因行数、字节数或执行时间边界失败时，缩小查询；不要自动截断、注入 `LIMIT`、读取 Run 文件或把失败包装成部分成功。

## 5. 形成交付

从 Run 的有界 Query 证据形成 Analysis Result，并在交付前读取 [result-contract.md](result-contract.md)。结论必须区分已观察事实、推断和不确定性；不要把 KAT Response、完整表或日志原样转发给用户。
