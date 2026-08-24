---
status: superseded by ADR-0062
---

# Workflow 声明可强制的表依赖

> ADR-0062 已删除 Workflow Required tables 与逐 Workflow Table Grant；Analysis 的实际 DataFusion 查询按需解析 PACK Source SchemaProviders。本文不再构成现行合同。

Workflow 通过装饰器必填的 `required_tables: list[str]` 显式声明其完整 PACK-visible Dataset 表依赖；没有 PACK-visible Dataset 表依赖的 Workflow 也必须写 `required_tables=[]`，不能用字段缺失表达空依赖。声明位于实际入口旁，由 KAT PACK authoring flow 中的 AI 沿入口可达的同 PACK helper 调用关系分析 SQL 与表访问，生成或修正为完整平铺的依赖集合；声明本身是需要 review 的 PACK 源码，AI 的推导或某次运行观察不是运行时事实。Decorator 应用时立即复制并规范化该 list，但此时没有本次 Dataset，调用条件由 Python Runtime 选定 Workflow 后检查；后续 Python 代码不能通过修改原对象改变已注册约束。KAT 平台自身不修改 PACK 源码；受信任 PACK 代码主动写入本地文件不属于这项 Interface 的保证。

PACK inspect 将 Required tables 去重并按 table name 排序输出。自动 Workflow 选择在提供 Dataset 时只保留 `required_tables` 是其实际表集合子集的候选，未提供 Dataset 时只保留空 Required tables 的候选；Hitrace Dataset 不包含零行占位表，因此表存在本身表示对应事实实际出现过，不再额外查询 row count。Python Workflow Runtime 在选定 Workflow 后由 TableGrantResolver 执行唯一运行时判定：Required tables 非空却没有 Dataset，或已提供 Dataset 缺少任一所需表时，在 Workflow 调用前失败；Required tables 为空时，无论是否提供 Dataset 都创建空 Table Grant 并签发 Execution Lease。Workflow execution plane 只把 Grant 中的 Dataset 表以裸名称注册为不可变 Source tables，通过受支持执行面访问未声明表或改写 Source table 会失败。Source 是来源与生命周期角色，不在只有这一类可寻址表的 Workflow SQL 中增加 namespace；Output Query 始终把 Run Outputs 注册为 `output.*`，只有 `available` Dataset 才另外注册 `dataset.*`。这使声明成为受支持 KAT 执行的实际访问约束，而不是一段可以漂移的文档。

第一版只声明精确的 Dataset table name。该名称与 Datasource 写入名、`tables/<name>.parquet` 文件 stem 和 Workflow SQL 中的裸表名完全一致，必须匹配统一的可移植 lowercase ASCII snake_case 规则；KAT 不做大小写、连字符、别名或其他转换，也不为罕见的 SQL 关键字冲突维护黑名单或预执行解析探针。名称、列或类型与实际查询不兼容时，由 DataFusion 在该次计划中直接失败；不引入 Datasource 身份依赖、Schema 约束 DSL、optional/alternative 表表达式、符号 Workflow Plan 或运行后回写的生成物。不同表组合对应不同分析能力时，使用不同 Workflow 表达，由单一 KAT Skill 自动选择。

普通 PACK helper 是实现细节，不单独声明 Table Grant；authoring flow 把入口可达 helper 的表访问汇总进调用它的 Workflow 的完整平铺 Required tables，Runtime 不建立或推导 Python 调用图。第一版 PACK 完全自包含，不存在 Workflow 入口互调或跨 PACK 能力调用；复用逻辑放在 helper。

`clock_domain` 是 Datasource 产生并以 `tables/clock_domain.parquet` 保存的普通 Source table，而不是 catalog 或隐藏 manifest。PACK 在 SQL 或 DataFrame 中直接读取它时，仍必须将其列入 Required tables。KAT 自有时钟操作为了解释 `clock_domain + clock_value`，可以在 Runtime 私有实现中读取该表而不把它注册进 Workflow execution plane；这种平台内部证据读取不是 PACK 的隐式表依赖，也不授权 PACK 访问其他未声明表。
