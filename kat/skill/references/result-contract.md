# 任务结果契约

每次任务只以以下三种状态之一交付。先陈述状态，再给足以复核的最少事实；不输出完整 KAT Response、完整表、原始 guide 或原始日志。

## 已完成

### 分析问题

按以下顺序交付：

1. 对用户问题的直接结论。
2. 少量可追溯证据：选中的 PACK/Workflow、Run identity、Run Output 名称，以及调用方主动约束范围的 Query columns 与 NDJSON 对象行。
3. 结论的适用范围、假设与不确定性；若使用 `--run` 读取当前 Workflow guide，避免把当前策略声称为历史 Run 的快照。
4. 基于 Workflow analysis guide 和已有证据形成的可选下一步探索方向。

Workflow guide 指导分析方法，但不是数据证据。不要把 Provider guide、Provider 实现或未执行的 SQL 当作分析结论。

### 创作或维护 PACK

- 只读理解：说明 PACK 的问题域、Workflow、运行参数、相关 Provider 能力、公开 guide 的约束、已有验证证据与限制。
- 写入变更：说明变更摘要、受影响文件、实际 Workflow/Provider inspection 和 PACK pytest 证据、仍存限制。

只引用成功 Response 中存在的公开字段。Workflow list 项只有 `name`、`description`；Workflow detail 只有 `name`、`description`、`parameters`、`guide`。Provider list 项只有 `name`、`description`；Provider detail 只有 `name`、`description`、`module`、`qualname`、`guide`。不要补造 guide 路径或任何其他内部字段。

## 需要补充信息

只在缺少继续任务的关键事实，或选择会改变实质结论时使用。说明：

1. 当前任务阶段与已经确认的事实。
2. 缺失的具体信息，或每个会改变结论的选项。
3. 用户只需提供的最小下一步。

一次只提出一个澄清问题。不要要求用户预先填写内部 CLI 参数或选择内部操作。

已有 Run 只有 Run ID 时不属于缺少用户信息：`kat inspect workflow --run` 取得当前 Workflow 知识，`kat query` 的 `information_schema` 取得实际 Output relation 与 columns。只有 Run 本身不可用，或用户问题仍不明确时才请求补充信息。

## 执行失败或受阻

说明：

1. 停止在哪个阶段。
2. 已验证的 KAT Response facts。
3. Diagnostic、可用 `log_path`、测试报告或其他可追溯证据。
4. 具体原因，以及可采取的最小下一步。

没有匹配 PACK/Workflow、Workflow 或 Provider inspection 的原子失败、失败测试、失败 Run 和实际执行失败的 Query 都属于这一状态。不得发布部分 discovery 结果，也不得把候选 Run、日志中的乐观文本或未修复代码描述为已完成。

Data Home 配置或选择失败时，说明停止在目标路径、等待用户手工修改，还是 KAT 选择阶段。不得代替用户读写 `config.json`、清空 `KAT_DATA_HOME` 或回退到其他目录重试。路径无效或用户尚未确认手工修改完成时，只需说明没有读取或修改配置、没有设置进程环境，也没有调用 KAT。

用户确认已经手工更换 Data Home 后，仍以 KAT Response 和 Diagnostic 作为配置是否有效、实际选择哪个目录的唯一事实来源；不要把用户确认本身描述为已经通过验证。
