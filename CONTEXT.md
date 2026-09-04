# KAT 领域词汇

KAT 是面向性能分析的可扩展平台。本文只收录名称不足以表达的项目特有概念及其相邻边界；具体设计与实现合同由 ADR 和对应规格承载。

## 产品与交互

**KAT**:
Kernel AI Kit 的简称，是由内核团队发起并承担平台基础设施看护责任的性能分析平台。Kernel 表达项目起源而非产品范围；平台维护者也不会因此让自己拥有的 PACK 获得特权。

**KAT Skill**:
KAT 面向用户的唯一公共入口和原子发布单元，承接数据分析与 PACK 开发任务。用户表达目标，Skill 可以在当前任务中临时组织多个正式 Workflow 并依据结构化事实形成结论，但这种调用序列本身不是新的 Workflow 或 Run；底层命令与运行机制不是独立产品面。

**KAT Agent Knowledge**:
由 KAT Skill 公共 reference、PACK 自有 declaration 与 guide、Runtime 结构化事实共同组成的渐进知识面。各内容随其所有者版本化；KAT 不把 PACK 知识复制进集中索引，也不自动摄取历史设计文档或源码注释。

**KAT Response**:
一次已经形成的操作交付给 KAT Skill 的结构化成功或失败事实。它是短命产品视图，不是 Run 的持久事实源，也不是 Analysis Result。

**Operation log**:
某项操作在 KAT Response 不适合完整承载时提供的可读证据。它不是每次调用都有的审计记录，也不构成 Run 或其他系统状态。

## PACK 与所有权

**PACK**:
由一个明确组织或团队拥有并独立维护的一组性能分析策略，是 KAT 的自包含扩展与发布边界。其边界由所有权和发布责任决定并独立于其他 PACK，而不是由 Workflow 数量、代码规模或单个分析问题决定。

**PACK owner**:
对一个 PACK 承担看护责任的唯一组织或团队。PACK owner 是可变的展示信息，不是 PACK 身份、命名空间、发布者认证或权限依据。

**Bundled PACK**:
与 KAT Skill 同版本发布的 PACK。Bundled 只说明交付来源，不形成公共 PACK kind，也不赋予额外运行权限。
_Avoid_: Built-in PACK、System PACK

**External PACK**:
由用户或第三方在受信任本地环境中独立部署的 PACK。它与 Bundled PACK 使用同一作者接口与运行模型，External 同样只说明交付来源。

**Pack Authoring API**:
KAT 面向 PACK 作者提供的公共编程界面，用于声明 Workflow 与 Provider inspection 元数据、构造标准表值，并使用 KAT 管理的执行能力和领域类型。私有纯 Python distribution `kat-workflow` 同时承载顶层 `kat` API 和 Runtime；它随 KAT Skill 原子交付，不是可独立安装或兼容的通用 SDK。
_Avoid_: Python SDK、Pack API

**Datasource wheel**:
平台原生私有 distribution `kat-datasource`，提供窄的 `kat_datasource` 来源 API。它与 `kat-workflow` 使用同一 KAT 版本，但二者互不依赖；Payload 同时安装它们，PACK 必须按所需边界显式 import。它不是 CLI 插件、公共 SDK 或可独立升级的产品。

**KAT Trace Library**:
KAT 向所有 PACK 平等提供的公共 Trace 分析语义，只接纳经过多个真实消费者和真实 Trace 验证的复用能力。来源解码、具体用户问题和单个 PACK 内尚未验证的候选算法不属于它。
_Avoid_: `kat.stdlib`

## Workflow 与测试

**Workflow**:
PACK 中回答一个具体分析问题的显式可调用入口，定义用户输入并产生一个或多个 Run Output。它可以直接形成证据，也可以用普通程序控制流显式调用其他 Workflow、按需查询这些子 Run 的只读 Catalog 并组装自己的输出；需要固化但只负责调用与 Guide 汇总的组合 Workflow 可以返回具有明确非空 Schema 的零行 Table。Workflow declaration 不重复声明 Output name 或 Schema，已发布 Catalog 与 Parquet footer 是消费方的唯一运行时合同。执行期间不接受 AI 解释或改写，PACK 仍是所有权与发布边界。

**Workflow guide**:
Workflow declaration 可选引用的 PACK 自有 Markdown 分析策略，只指导 AI 解释该 Workflow 对应的 Run Output、向哪些方向继续取证。组合调用默认先读取父 Guide 与父 Run Output inventory；只有父 Guide 要求汇总子结论或父级证据不足时，才沿 `child_runs` 按需读取相关子 Run，并分别用各自 Guide 解释。Guide 建议 AI 后续调用的 Workflow 会在同一 Analysis Session 中形成新的独立根 Run，不会事后加入或修改已经发布的父 Run。Guide 不自动继承或合并，缺省 Guide 表示该 Run 不要求独立的 AI 解释。Guide 不能脱离具有 Python 入口的 Workflow 独立执行，也不是 Output Schema、可执行计划或 Run 快照；inspection 每次读取当前 PACK 版本。

**Workflow Context**:
KAT 在一次 Workflow 调用内提供的窄能力对象，通过 `datasource_root` 暴露当前 Analysis Session 的共享来源物化范围，通过 `scratch_root` 暴露当前候选执行的临时工作范围，并允许用线程安全的 `ctx.run(pack_name, workflow_name, /, **inputs)` 显式调用另一个完整命名的 Workflow、取得其已发布 Run Output 的只读 `dp.Catalog`。PACK 与 Workflow name 是仅限位置的路由参数，目标输入全部使用具名参数；目标只能从顶层命令已经确定的 PACK discovery roots 中解析，Context 不能增加目录、直接传入路径或绕过 KAT 执行边界直接调用另一个 Workflow 函数。Context 不直接提供 Session 身份或根目录能力，不查询来源、不持有查询 Session，也不创建、发现、包装或自动关闭 Provider；它只在当前调用期间有效，不是用户输入，不存在隐式全局当前 Context。子 Run 物理路径不进入受支持的 Catalog 公共接口、CLI Response 或公共诊断；向受信任 PACK 提供的普通路径不构成文件系统沙箱。Context 拒绝再次调用当前活动调用链中已经出现的 Workflow，但允许在上一次调用完成后重复调用同一 Workflow；每次允许的调用都会形成独立 Run。父 Workflow 返回时必须已经结束并等待自己启动的全部子调用，否则父 Run 不会发布。

**RunError**:
`ctx.run()` 无法向调用方交付可用子 Run Catalog 时抛出的唯一公开异常类型 `kat.RunError`。它的文本只供人阅读，程序不能解析，也不公开稳定 phase、Session ID、Run ID、路径或“是否已发布”字段。任意 RunError 都不能假定可安全自动重试，因为失败调用可能已经留下成功 Run、成功后代或外部副作用；捕获后可以降级，主动重试始终是可能产生重复 Run 的全新执行。

**Datasource root**:
`ctx.datasource_root` 返回的当前 Analysis Session 共享来源物化目录能力。同一 Session 中的 Workflow 可以跨 PACK 按约定复用其中以 Source stem 命名且首次发布后不可原位替换的完整物化；它不是临时工作目录、Run Output、整个 Session 根或需要向用户暴露的物理路径。

**Source stem**:
原始来源 basename 去掉最后一个后缀后的 Session 内物化名称。它不附加 hash 或自动消歧，不是跨 Session 的 Source identity；同一分析中的唯一性由用户保证。

**Scratch root**:
`ctx.scratch_root` 返回的当前候选执行私有临时目录能力。它只承载本次执行可丢弃的中间文件，执行结束时清理，不能被后续 Workflow 当作输入。

**Workflow arguments**:
调用 Workflow 时提供的原始具名文本输入。它们尚未表达类型、默认值或业务语义，只有选定的 Workflow 才能把它们解释为 Workflow input values。

**Workflow input values**:
Workflow arguments 经选定 Workflow 的约束解析后得到的具名、带类型且包含默认值的实际控制值。`ctx.run()` 使用与目标标注对应的精确 Python 值：`str`、有符号 64 位 `int`、有限 `float`、`bool`、允许的字符串 `Literal`、`kat.Duration` 或 `kat.WallClockTimestamp`；只有 Optional 接受 `None`，省略参数仍由目标 Input Compiler 应用默认值。嵌套调用不执行 CLI 字符串 coercion，也不把 `bool` 当作 `int` 或把 `int` 当作 `float`。输入可以选择 Workflow 已准入的非敏感来源位置或来源生态 selector；凭据、任意 Provider 代码和另一个 Workflow 的 `dp.Catalog` 不是 Workflow input values。组合父 Workflow 可以查询 Catalog 后，从结果中提取目标 Input Compiler 已支持的普通值再调用下一个 Workflow，但不形成第二套仅供嵌套调用的 Catalog 或 Table 输入合同。

**PACK test**:
针对 PACK 生产 Interface 的 KAT 集成测试。测试用普通 fixture 构造来源文件、配置、Datasource Provider 和临时路径，再通过 `kat_run` 执行真实 Workflow；组合 Workflow 的 `ctx.run()` 复用生产环境的发现、输入校验、独立子 Runtime、异常和子 Run Catalog 返回路径。被测 PACK 固定来自 `kat test --pack-dir` 的精确目录，嵌套调用还可以发现正常默认或已安装 PACK roots；首版不接受额外 dependency checkout 目录，未安装 sibling PACK 必须先进入正常发现范围。测试命令使用自己的临时 Analysis Session 与工作空间，不发布生产 Session 或 Run；`kat_run` 保持现有合同，向测试返回按 Output name 索引的 `dict[str, pyarrow.Table]`。

**Provider fixture**:
随 PACK 一起版本化、供测试用普通配置与临时路径创建 Datasource Provider 的受控输入与非敏感配置。它不是平台持久状态，也不规定 Provider 的存储格式。

## 数据与事实

**Datasource**:
由 Workflow 明确选择的 PACK Python 来源边界，拥有外部事实的定位、配置解释、来源内查询和来源特定物化。它通常由 PACK 顶层 `datasources/` 中的普通模块和类表达，没有独立平台身份、注册或发现机制；跨 Datasource 组合由 Workflow 显式使用 DataFusion Provider 完成。

**Datasource Provider**:
PACK 拥有并直接暴露给 Workflow 的来源能力对象，以来源自己的词汇提供定位、解码、查询、物化等显式操作，并按普通 Python 规则拥有所使用的来源资源。它不是 KAT 创建或包装的统一 facade，也不是平台托管的持久状态；独立 Provider inspection 只发现其 metadata declaration，不改变生产 Workflow 显式 import、构造和调用它的方式。
_Avoid_: Source executor、KAT Provider facade

**Provider guide**:
Provider declaration 必须引用的 PACK 自有 Markdown 来源知识，说明 Source query 方言、relation、Schema、接入限制与诊断方式。它服务 PACK 开发，不是 Workflow 分析策略，也不规定 Provider 固定方法。

**Data Provider Toolkit**:
`kat-workflow` 通过公共模块 `kat.dataprovider` 提供的标准表数据工具，推荐以 `from kat import dataprovider as dp` 使用。它提供 Datasource Schema、单表数据、Parquet Catalog 和本地 Fusion query 能力，但不是 Database facade，不拥有 Datasource Provider 的来源定位、decode、query 或生命周期，也不发现、注册、构造或包装 PACK 的 `datasources/` 来源实现。

**Datasource Schema**:
PACK 通过 `dp.Schema` 保存的一个 Datasource Provider 可产生的一组具名逻辑表及其列约束，规定自定义解析代码准备形成的多表事实结构。声明使用普通嵌套 Mapping 和基础 Python 类型；它是 Provider 产出事实的逻辑合同，也是 `dp.write()` 创建一次 Datasource 流式写事务时唯一需要的结构声明，但不是 Database 定义或打开既有 Parquet 时必须提供的持久化 Schema。

**Table**:
一个已经完成、具有明确非空物理列结构、不可变且可重复读取的 eager 单表值；具有明确 Schema 的零行 Table 同样有效。Source query、Fusion query、已经形成的 Arrow 数据，以及带显式 PyArrow Schema 的完整小型 Python rows 可以产生 Table；它不是逐行构建器、Datasource Schema 的可写实例或隐式持久化指令。Table 不携带固有名称，Fusion relation name、Parquet relation name 和 Run Output name 都由各自边界表达；它是 Workflow 首版唯一允许产生的 Output value。

**Source catalog**:
Datasource Provider 私有的来源 relation 集合及名称映射，只供该 Provider 的 Source query 使用。KAT 不规定其发现或布局，也不把其中关系自动传给 DataFusion Provider。

**Parquet catalog**:
以 Parquet 文件承载多张具名 relation 的只读集合，可以由以 Datasource Schema 约束的 `dp.write()` 流式写事务产生、接入已有 Parser 产物，或由 KAT 为一次成功 `ctx.run()` 的已发布子 Run Output 构造。`dp.open(root=...)` 发现目录当前已有的非空 relation 集合；`dp.open(tables=...)` 显式绑定调用方列出的 relation 路径。它们从 Parquet footer 取得物理结构。Catalog 只通过 `catalog.tables` 暴露稳定 relation 名称，本身不创建 Session、执行 SQL 或持有查询结果；它只能显式交给 DataFusion Provider 扫描。子 Run Catalog 的受支持公共表面同样只暴露实际 relation 名称，列与类型在 DataFusion SQL planning 时从 footer 取得；它不能作为 Workflow Input 或 Run Output。

**Source query**:
Workflow 显式交给 Datasource Provider、由该 Provider 在一个来源内解释的查询。其方言、参数和来源内关系组合属于 Datasource；KAT 不把它解析或拆分成跨来源计划。它不同于 DataFusion Provider 执行的 Fusion query。

**Fusion relation name**:
DataFusion Provider 查询使用的本地关系名：内存 Table 的名称来自构造参数 `tables` 的 key；Parquet relation 的名称来自 `dp.open(tables=...)` 的 Mapping key，或 `dp.open(root=...)` 发现的文件 stem。它只供该 Provider 的 SQL 引用，不是 Table 身份、Datasource Provider 私有 Source catalog 名、远端关系身份或全局注册名。
_Avoid_: Query result name

**DataFusion Provider**:
Data Provider Toolkit 提供的具体本地查询 Provider，由 Workflow 或 PACK 显式构造和重复调用。它接受具名内存 Table、至多一个 Parquet Catalog 或二者组合；每次 `query()` 使用新的短命 DataFusion Session，完成规划、执行和结果校验后 eager 返回 Table。组合 Workflow 调用多个子 Workflow 时，分别查询各子 Catalog 的必要列或聚合，再通过 `tables={...}` 融合这些缩减后的 Table；首版不直接联邦查询多个 Catalog。DataFusion Provider 不会发现 Datasource Provider、触发 Source query 或访问未传入的 relation。

**Fusion query**:
DataFusion Provider 对 Workflow 显式提供的内存 Table、Parquet Catalog 或两者组合执行并形成新 Table 的本地查询。它不能透明引用 Datasource Provider 私有的 Source catalog、发现来源 Provider、隐式触发 Source query，或替 Workflow 拆分和下推远端 SQL；来源特定下推由各 Datasource Provider 自己负责。

**Hitrace decode**:
PACK 通过独立 `kat-datasource` wheel 的 `kat_datasource.hitrace.decode(source, destination)` 显式执行的原生来源解码。调用方拥有 source 和尚不存在的 destination；成功后 destination 的直接子级只含扁平具名 `*.parquet` relation，并返回 unsupported plugin/section report。Workflow 可以把一次性 destination 放进 `ctx.scratch_root`，或按 Session 复用约定发布到 `ctx.datasource_root` 下的来源目录，再用 `dp.open(root=destination)` 打开；解码结果不会自动成为 Run Output。

**Datasource materialization**:
Datasource Provider 为来源查询准备的本地 backend，其来源语义与准入合同由 Provider 拥有。准备跨 Run 复用的物化在 Analysis Session 中首次完整发布后不可原位替换，其他 Workflow 可以跨 PACK 按约定只读复用；一次性物化留在 `ctx.scratch_root`，打不开或不符合合同的共享物化只能换用新的 Source stem 或在新 Session 中重新物化，并且它不是 Provider query result 或 Run Output。

**Streaming materialization**:
自定义 Parser 在不先形成完整 eager Table 的前提下，持续形成完整 Datasource materialization 的作者能力。`dp.write(schema, destination=...)` 是该能力唯一的公共入口：它产生一次性、多 relation、只写事务，并只在全部输入成功结束后发布可由 `dp.open()` 打开的 Parquet 目录。它不是 Table、解析中的部分 Catalog 或 Run Output；独立 `materialize` API、接受完整 Table Mapping 的 eager `write` 重载和可追加 Table 都不属于该模型。
_Avoid_: Streaming Table、Disk-backed Table

**Trace fact**:
Datasource 从原始 Trace 直接解码或跨记录规范化得到、可供多个 PACK 复用的来源事实。它仍是来源数据，不是预先计算的分析结果。

## Workflow 时间输入

**Duration**:
纳秒精度的非负经过时长，不携带时区或时钟原点。它描述时间差，不代表某个来源时钟上的读数或公历时间点。

**Wall-clock timestamp**:
带有明确 UTC 偏移、能够定位到公历时间线的绝对时间。来源数据中的整数时钟读数即使以纳秒计量，也不会因此自动获得墙上时间语义。

## Run、查询与结论

**Analysis Session**:
一次可以跨 PACK 的多 Workflow 分析边界，归集其中相互独立的 Run、可复用来源物化与临时工作数据。它具有独立于其中各 Run 和 PACK 的身份，统一这些内容的分析归属和生命周期，但不把不可变 Run Output、可重建 Datasource materialization 与临时数据变成同一种事实。Session 在任何生产 Workflow 执行前显式创建并允许为空；成功或失败的 `kat run` 都不会自动删除它，只有用户显式删除才结束其生命周期。最外层 `kat run` 执行在整个嵌套执行、Catalog 查询和完成收拢期间持有 Session 共享租约；显式删除必须取得独占租约，遇到活动执行时快速失败，不等待也不取消 Workflow。Session inspection 以平坦 Run inventory 返回每个 Run 的直接 `child_runs`，供调用方按需遍历，不递归嵌入整棵树。父 Workflow 失败不会使其中已经发布的子 Run 失效，也不保证能把没有已发布父级的 Run 精确归因到某次失败调用。

**Run**:
Analysis Session 中一次成功发布的 Workflow 执行，包含在该 Session 内唯一的 Run Manifest 和至少一个 Run Output。失败或尚未发布的候选执行不是 Run，Run ID 也只在发布成功后成立；公共定位同时需要 Session ID 与 Run ID。

**Run Manifest**:
一个 Run 的唯一持久清单，记录其 Session 与 Run 身份，以及 PACK、Workflow、有效输入和 Run Output 元数据。组合 Workflow 的 Manifest 还以按 Run ID 稳定排序、但语义无序的集合记录执行期间实际成功发布的直接子 Run；子 Run 发布成功后即属于该集合，即使随后向父 Runtime 交付或构造 Catalog 失败。失败调用没有 Run Manifest，因此其已发布后代只保留在 Session inventory 中，既不记录失败父级，也不提升为更高祖先的直接子 Run。Manifest 不记录 Datasource materialization provenance，也不表达调用先后、分支、线程关系、计划、未来分支、失败尝试或执行状态，不承载 Analysis Result。

**Session inspection**:
一个 Analysis Session 的平坦已发布 Run inventory。每个 Run 精确投影 `run_id`、`pack`、`workflow`、按 Run ID 排序的直接 `child_runs` 和公开 Output inventory；叶子 Run 的 `child_runs` 是空数组。它不递归嵌入子 Run，也不公开 effective inputs、物理路径、执行计划或失败调用。调用方用选中 Run 的双 ID 继续执行 Workflow Guide inspection 和 Output Query。

**Run Output**:
随 Run 持久发布的具名不可变表格事实，只能来源于 Workflow 返回的精确 `dp.Table`，或非空普通 `dict[str, dp.Table]` 中的精确 Table；单值命名为 `main`，多值由 dict key 显式命名，并完整写为该 Run 自己的 Parquet。子 Run Catalog、物理文件、任意 Python 对象、裸路径、Markdown 或 JSON 都不是 Run Output；它也不是 Datasource materialization、Query Result 或面向用户的 Analysis Result。
_Avoid_: Artifact、Result

**Output Query**:
针对由 Session ID 与 Run ID 共同定位的已发布 Run `output.*` 发起的本地只读后续查询，不创建新 Run，也不重新执行 Provider query。每次查询使用独立 DataFusion Session，只注册该 Run 的 Output 与 `information_schema`；PACK、Datasource、同一 Session 的其他 Run 和历史 Manifest 字段均不可见。Python/DataFusion 把单条只读 SQL 的结果以原生 Arrow JSON 映射直接写成单文件 NDJSON；KAT 不建立自定义标量转换层，也不自动增加分页、截断、固定 `LIMIT` 或超时。用户 SQL、输出规模、等待时间与本机资源消耗由调用方和用户负责。

**Query Result**:
一次成功 Output Query 发布的单文件 NDJSON。KAT Response 只返回 `format="ndjson"`、文件 `path` 和有序 `columns`；文件每行是一个使用查询列名的 JSON object，零行结果是空文件。它不会成为新的 Run Output，也不是模型面向用户形成的 Analysis Result。

**Analysis Result**:
模型基于 Run Output 和必要的 Query Result 形成的面向用户判断、报告或结论。组合调用结束后，KAT Skill 先依据父 Guide 分析父 Run；只有父 Guide 要求或父级证据不足时，才按各子 Run 自己的 Guide 形成所需解释，再回到父级汇总。这些中间解释仍只是模型工作上下文，不是新的持久对象。Analysis Result 不由 Workflow 生成，也不写入 Run Manifest。
