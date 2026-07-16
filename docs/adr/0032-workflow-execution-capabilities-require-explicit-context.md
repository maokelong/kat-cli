---
status: accepted
---

# Workflow 执行能力需要显式 Workflow Context

Workflow Runtime 把 `kat.Context` 作为每个 Workflow 的第一个参数显式传入。第一版 Context 恰好只公开 `ctx.sql(sql, **params)`、`ctx.from_arrow(table)` 与 `ctx.convert_clock(clock_domain, clock_value, *, target_domain)` 三个受 Execution Lease 约束的方法；模块级 `kat` 只保留装饰器和类型定义，不提供 `kat.sql`、`kat.from_arrow`、`kat.convert_clock`、`kat.testing.run` 或其他隐式当前 Context 的平行 Interface。PACK 集成测试的 Workflow 执行能力只由 KAT pytest plugin 注入的 `kat_run` fixture 提供。需要运行能力的 PACK helper 必须显式接收 Context，纯计算 helper 不接收它，以一个固定参数换取可见依赖、同一测试 Seam 和集中维护。

`ctx.convert_clock(...)` 接受精确类型为 Arrow `Utf8` 的来源 `clock_domain` Expr、精确类型为 `UInt64` 的 `clock_value` Expr，以及 keyword-only 的普通固定字符串 `target_domain`，返回目标 domain 下的 `UInt64 ClockValue Expr`。它是 Runtime 注册的 SQL scalar UDF `kat_convert_clock(clock_domain, clock_value, target_domain)` 的 Python 构造入口；两者共享同一 Python/PyArrow 批量实现、Dataset clock evidence、精确类型检查和失败语义。SQL 的目标必须是普通字符串字面量，其 DataFusion 内部 literal 表示不属于公共类型承诺。KAT 不隐式 cast 来源的其他 string、integer 或 Decimal 类型；PACK 必须先使用 DataFusion 严格显式 cast。Context 不开放通用 `ctx.udf(name)`，PACK 也不能传 snapshot、频率或 Dataset 路径。函数不返回 Struct 或重复的目标 domain 列；需要发布结果时由 PACK 使用自说明列名。它不把结果提升为 Wall-clock timestamp；第一版只有目标 `clock_type` 为 `realtime` 或 `realtime_coarse` 时，PACK 才复用 DataFusion 的严格 Arrow cast，Context 不增加 `to_wall_clock` 等平行方法。这个方法依赖 Execution Lease，并在实际调用时依赖本次可选 Dataset 中的时钟证据；并且让 DataFrame Workflow 不必退回 SQL，因此满足新增 Context 方法必须是通用 Workflow 执行机制的门槛。

时钟证据不在 Context 或 execution plane 创建时预加载。实际使用换算时，Runtime 才从本次可选 Resolved Dataset 构建该 execution plane 私有的内存 Resolver，并在后续调用中复用；没有 Dataset 或缺少证据只使使用换算的操作失败。Resolver 不形成 PACK 可访问对象、持久缓存或新的 Context 方法，Workflow 执行结束时随 execution plane 一同释放。

Workflow 通过返回 `DataFrame | dict[str, DataFrame]` 交付 Output，不提供 `ctx.output`。返回边界把裸 DataFrame 固定规范化为 `{"main": dataframe}`；显式字典用于领域命名或多表输出。Runtime 此后只处理具名 DataFrame 映射，不从 Workflow 名、函数名或路径推导 Output name，也不提供 decorator override、TypedDict、`kat.Output` 或 builder。`main` 避免把程序产物误称为 Analysis Result；PACK 若需要更具体的名称，即使只有一个输出也返回单元素字典。PyArrow Table、list、tuple、generator、标量和其他返回形态不属于该 Interface。

Output name 必须完整匹配 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`，并在 Python 字典、Run Manifest 与 `output.<name>` SQL 引用中保持同一拼写；Runtime 原样拒绝非法名称，不做 lowercase、sanitize 或 quoted-name fallback。限定名称直接交给实际随 KAT 发布的 DataFusion 解析，KAT 不复制随 SQL parser 演进的关键字表或维护版本探针；罕见的语法冲突由该次查询正常失败。名称只是逻辑机器标识，物理目录和文件始终由 Runtime 使用不透明 Output ID 私有定位，因此不维护 Windows 文件保留名黑名单；第一版也不增加没有真实依据的长度上限。

KAT 不要求或解释 Workflow return annotation，也不增加 decorator `outputs`、逐 Output description、静态 Schema 或列说明。返回标注可以由 PACK 按普通 Python 惯例自愿提供给 IDE 和 type checker；Runtime 只校验真实返回值，`kat test` 通过实际执行覆盖该边界。Output name、列名和 Workflow 执行时的实际 Schema 默认足以自说明；确需补充的领域背景写入普通 Workflow docstring 或就近源码注释，但不形成 KAT 解析的结构化协议或第二份返回约束。

发布 Run 要求 Workflow 返回值在规范化后产生非空的具名 DataFrame 映射。`None` 与空字典没有可供模型或用户消费的分析产物，通常表示漏写 `return`，因此直接形成带 Workflow 上下文的错误；日志不能替代 Output。业务上的“没有匹配记录”由具有确定 Schema 的零行 DataFrame 表达，它仍会进入 Run Manifest 并显示 Output name、完整 `columns` 与 `row_count = 0`。

多输出采用逻辑 all-or-fail，而不为 Output 文件引入文件系统事务。Runtime 在执行任何 DataFrame 前先完整校验容器、所有名称和值类型，再执行并写出各项；只有全部成功才把全部 Output ID 一次性写入临时 Runtime Response 的 success `result`。CLI 仅在 Runtime、Runtime Response 验证与 Operation log 均完整成功后，把 CLI 持有的候选 UUID、PACK、Workflow 与可选 canonical Dataset path，同 Runtime `result` 中的 effective inputs 和 Outputs 合成为不复制 Runtime Response 的 status/result wrapper 的独立 Run Manifest，并使用既定同目录临时文件持久化边界发布最终 `manifest.json`。任一执行、写出或外层交付失败都会使 `kat run` 操作失败且不发布 Run；处于 failure 分支的 Runtime Response 不暴露 Table Output，随机临时 Runtime Response、未发布 Run Manifest 和先前写出的文件也不被 `kat query` 访问。Operation log 与失败候选目录可以保留诊断证据；除最终 Run Manifest 的标准临时文件持久发布外，第一版不为 Output 承诺回滚、清理、原子重命名或崩溃恢复。

PACK 使用 Python 标准库 `logging` 记录日志，由 Runtime 安装 handler 并补充候选 UUID、PACK 和 Workflow 元数据，不提供 `ctx.log`。Context 也不提供 `ctx.table`、PACK 发现、配置与运行元数据读取、Dataset 路径、依赖查找或底层 SessionContext，且不能在 Workflow 执行结束后复用；任何新增方法都必须先证明它是通用且受 Lease 约束的运行机制。这使 Context 成为深的执行 Module，而不是随项目增长不断吸收无关职责的 God Object。
