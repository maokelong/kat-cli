# KAT 领域词汇

KAT 是面向性能分析的可扩展平台。本文只收录名称不足以表达的项目特有概念及其相邻边界；具体设计与实现合同由 ADR 和对应规格承载。

## 产品与交互

**KAT**:
Kernel AI Kit 的简称，是由内核团队发起并承担平台基础设施看护责任的性能分析平台。Kernel 表达项目起源而非产品范围；平台维护者也不会因此让自己拥有的 PACK 获得特权。

**KAT Skill**:
KAT 面向用户的唯一公共入口和原子发布单元，承接数据分析与 PACK 开发任务。用户表达目标，Skill 组织所需操作并依据结构化事实形成下一步或最终结论；底层命令与运行机制不是独立产品面。

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
KAT 面向 PACK 作者提供的公共编程界面，用于声明 Workflow 并使用 KAT 管理的执行能力和领域类型。它随 KAT Skill 原子交付，不是可独立安装或兼容的通用 SDK。
_Avoid_: Python SDK、Pack API

**KAT Trace Library**:
KAT 向所有 PACK 平等提供的公共 Trace 分析语义，只接纳经过多个真实消费者和真实 Trace 验证的复用能力。来源解码、具体用户问题和单个 PACK 内尚未验证的候选算法不属于它。
_Avoid_: `kat.stdlib`

## Workflow 与测试

**Workflow**:
PACK 中回答一个具体分析问题的可运行入口，定义用户输入并产生一个或多个 Run Output。Workflow 承载分析任务，PACK 才是所有权与发布边界。

**Workflow Context**:
KAT 在一次 Workflow 执行内显式提供的运行能力入口，通过 `datasource_root` 暴露当前 PACK 在 KAT Data Home 下的存储范围，并在迁移期以 `ctx.sql(sql, **params) -> DataFrame` 保留旧 Dataset grant 查询。首版 Workflow 在该范围内创建临时 workspace，并把其中派生的普通路径传给文件 Provider；新 Datasource Provider 与 DataFusion Provider 都不接收 Context，Context 也不创建、发现、包装或自动关闭它们。它只在当前执行期间有效，不是用户输入，也不存在隐式的全局当前 Context。

**Required tables**:
迁移期 Workflow 对旧 Dataset Source table 的完整、精确声明。KAT 在调用 Workflow 前继续强制这组 Table Grant；它不声明 Datasource Provider、Source query 或 DataFusion Provider 的显式 relation 输入。空声明表示不需要旧 Dataset relation，不表示 Workflow 不使用 Datasource Provider。

**Workflow arguments**:
调用 Workflow 时提供的原始具名文本输入。它们尚未表达类型、默认值或业务语义，只有选定的 Workflow 才能把它们解释为 Workflow input values。

**Workflow input values**:
Workflow arguments 经选定 Workflow 的约束解析后得到的具名、带类型且包含默认值的实际控制值。迁移期旧来源事实仍由 Dataset 提供；新 Datasource 可以让输入选择 Workflow 已准入 Provider 的非敏感数据位置或来源生态 selector。凭据和任意 Provider 代码不是 Workflow input values。

**PACK test**:
针对 PACK 生产 Interface 的 KAT 集成测试，可以按所测边界使用 Test Dataset 或 Provider fixture 执行真实 Workflow 行为。它产生测试证据而不发布生产 Run。

**Test Dataset**:
随 PACK 一起版本化、供测试按需使用的普通 Dataset。迁移期它继续验证旧 Dataset/Table Grant 边界；Provider fixture 不替代或转换它。

**Provider fixture**:
随 PACK 一起版本化、供测试用普通配置与临时路径创建 Datasource Provider 的受控输入与非敏感配置。它不是平台持久状态，也不规定 Provider 的存储格式。

## 数据与事实

**Data Import**:
迁移期由一个显式选定的旧 Datasource type 把外部来源完整转换为新 Dataset，或整体替换已有 Dataset 的用户操作。新 PACK Datasource/Provider 不注册成这种封闭类型，也不通过 `kat import` 创建 Table。

**Datasource**:
由 Workflow 明确选择的 PACK Python 来源边界，拥有外部事实的定位、配置解释、来源内查询和来源特定物化。它没有独立平台身份、注册或发现机制；跨 Datasource 组合由 Workflow 显式使用 DataFusion Provider 完成。迁移期旧 Data Import Datasource type 仍是另一个概念，不是这里的插件机制。

**Datasource Provider**:
PACK 拥有并直接暴露给 Workflow 的来源能力对象，以来源自己的词汇提供定位、解码、查询、物化等显式操作，并按普通 Python 规则拥有所使用的来源资源。它不是 KAT 创建或包装的统一 facade，也不是可由平台发现的持久状态。
_Avoid_: Source executor、KAT Provider facade

**Data Provider Toolkit**:
KAT 通过公共模块 `kat.dataprovider` 提供的标准表数据工具，推荐以 `from kat import dataprovider as dp` 使用。它包含 Schema、Table、Parquet 写入与打开、Catalog 和具体 DataFusion Provider，但不定义 Datasource Provider 基类，不发现、注册、构造或包装 PACK 的 `datasources/` 来源实现。

**DataFusion Provider**:
KAT Data Provider Toolkit 提供的具体本地查询 Provider，由 Workflow 或 PACK 像普通 Python 对象一样显式构造和重复调用。它在构造时复制 `tables` Mapping 并保留其中 Table 与一个可选 Catalog 的强引用，不提供 relation 注册或替换；每次 `query()` 取得各 Table 的调用时快照，把内存与 Parquet relation 交给新的短命 DataFusion Session，在 planning 后、扫描前校验结果 Schema，执行并收集后再校验数据级 Table 约束，最后 eager 返回 Table。它不是 Datasource Provider 的基类、Runtime facade、全局 registry，也不发现或调用来源 Provider。

**Datasource Schema**:
PACK 通过 `dp.Schema` 保存的一组具名逻辑表及其列约束，规定自定义解析代码准备形成的事实结构，并能创建与每张声明对应的 Table。声明使用普通嵌套 Mapping 和基础 Python 类型；它是 Table 构造与追加合同，不是打开既有 Parquet 时必须提供的持久化 Schema。

**Source catalog**:
Datasource Provider 私有的来源 relation 集合及名称映射，只供该 Provider 的 Source query 使用。KAT 不规定其发现或布局，也不把其中关系自动传给 DataFusion Provider。

**Parquet catalog**:
以 Parquet 文件承载多张具名 relation 的只读集合，可以由 `dp.write()` 产生，也可以接入已有 Parser 产物。`dp.open(root=...)` 发现目录当前已有的非空 relation 集合，不证明预期表完整；`dp.open(tables=...)` 显式绑定调用方列出的全部 relation 路径并可发现缺失。两者都不接收 Datasource Schema，而是从 Parquet footer 取得并校验锁定 DataFusion 可扫描的物理结构。Catalog 来源列可以比标准 Table 更宽；只有 Fusion query 最终结果需要满足 Table 准入。Catalog 只通过 `catalog.tables` 暴露稳定 relation 名称，不公开另一套列 Schema API；列结构由 DataFusion Provider 的 `DESCRIBE` 查询形成标准 Table。Catalog 自身不创建 Session、执行 SQL 或持有查询结果，只能显式交给 DataFusion Provider 扫描。它不是 Dataset、内存 Table 集合或 Run Output。

**Source query**:
Workflow 显式交给 Datasource Provider、由该 Provider 在一个来源内解释的查询。其方言、参数和来源内关系组合属于 Datasource；KAT 不把它解析或拆分成跨来源计划。它不同于 DataFusion Provider 执行的 Fusion query。

**Fusion relation name**:
DataFusion Provider 查询使用的本地关系名：内存 Table 的名称来自构造参数 `tables` 的 key；Parquet relation 的名称来自 `dp.open(tables=...)` 的 Mapping key，或 `dp.open(root=...)` 发现的文件 stem。它只供该 Provider 的 SQL 引用，不是 Table 身份、Datasource Provider 私有 Source catalog 名、远端关系身份或全局注册名。
_Avoid_: Query result name

**Fusion query**:
DataFusion Provider 对 Workflow 显式提供的内存 Table、Parquet Catalog 或两者组合执行并形成新 Table 的本地查询。它不能透明引用 Datasource Provider 私有的 Source catalog、发现来源 Provider、隐式触发 Source query，或替 Workflow 拆分和下推远端 SQL。迁移期 `ctx.sql(sql, **params) -> DataFrame` 只为旧 Dataset grant 保留兼容查询职责，不是新 Datasource 的 Fusion query 入口。

**Datasource materialization**:
Datasource Provider 为来源查询准备的本地 backend，其格式与生命周期属于当前 PACK。首版文件 Provider 只在当前 Workflow 内、`ctx.datasource_root` 的临时子目录中使用这类产物，eager Table 脱离来源后立即清理；它不是跨 Workflow cache、KAT 平台状态、Provider query result 或 Run Output。需要来源身份、版本、并发锁和回收的可复用物化以后另行设计。

**Dataset**:
迁移期由旧 Data Import 完整生成或整体替换的本地事实集合，是现有 `required_tables` Workflow 的事实输入。其具体位置就是身份，不另设 Dataset ID；Datasource Table、Run Output 和 Datasource materialization 都不属于 Dataset。

**Source table**:
旧 Dataset 在迁移期通过 Table Grant 向 Workflow 提供的不可变事实关系。Runtime 只在兼容 `ctx.sql(sql, **params) -> DataFrame` 的受管理执行面中注册已授权的 Source table；新 Datasource 的 DataFusion Provider 不能取得这些 grant 或其裸路径。

**Table**:
一个具有明确列结构、可追加并可重复读取的单表数据值；Datasource Schema 创建、`Table.from_arrow()`、Source query 与 Fusion query 都产生同一种可追加 Table，不因来源切换为只读状态，也不另设 Builder、ReadOnlyTable 或 QueryTable。追加合同由构造路径决定：`Schema.create()` 与 `Table(single_table_schema)` 使用 Python 逻辑合同，`Table.from_arrow()` 与 DataFusion Provider query 使用已经准入的 Arrow 物理 Schema；Source query 可以选择前述任一路径。append 只接受与合同对应的精确 Python 类型，不借用 PyArrow 做跨类型隐式转换；整行成功后才增加新 Arrow chunk，历史 buffer 与既有快照不变。Table 不携带固有名称；Schema 表名、Fusion relation name、Parquet 表名和 Run Output name 都由各自边界的 Mapping 表达。

**Workflow DataFrame**:
迁移期 Pack Authoring API 通过 `ctx.sql(sql, **params)`、`ctx.from_arrow()` 等旧入口暴露的惰性 DataFusion 关系值。新的 Datasource Source query 与 Fusion query 都形成已经执行完成、可重复读取的 Table，不再把这种引擎值作为标准查询结果。

**Trace fact**:
Datasource 从原始 Trace 直接解码或跨记录规范化得到、可供多个 PACK 复用的来源事实。它仍是来源数据，不是预先计算的分析结果。

## 时间语义

**UnifiedClock**:
由 `ClockDomain` 与 `ClockValue` 组成的不可变值，表示某个底层时钟坐标上的一次具体读数。统一的是值结构和受支持的时间操作，不是计量单位、时间原点或公共时间线。
_Avoid_: ClockReading、SourceTimestamp

**Clock value**:
某个 Clock domain 上的非负原生读数，单独不具有完整时间语义。它不必然以纳秒计量，也不能脱离 Clock domain 被解释为 UTC 时间。
_Avoid_: Timestamp、timestamp_ns

**Clock domain**:
Dataset 内一个具体时钟坐标的身份，并提供解释 Clock value 所需的时钟语义。相同名称出现在不同 Dataset 中不表示同一时钟实例，时钟类型也不能代替具体 domain。
_Avoid_: Source clock、Event clock

**Clock snapshot**:
同一采集附近对多个 Clock domain 取得的一组关联读数，是跨 domain 换算的证据。它不把这些 domain 变成同一个时钟，也不自动担保映射在整个采集期间保持准确。

**Clock conversion**:
依据当前 Dataset 的明确证据，把来源 Clock value 显式换算到目标 Clock domain。结果仍是目标 domain 上的 Clock value，不会自动成为 Duration 或 Wall-clock timestamp；不同 domain 的比较、连接和相减必须先完成这种对齐。

**Duration**:
两个兼容时间点之间的纳秒精度非负经过时长，不携带时区或时钟原点。它描述时间差，而不是某个 Clock domain 上的读数或公历时间点。

**Wall-clock timestamp**:
带有明确 UTC 偏移、能够定位到公历时间线的绝对时间。一个 Clock value 即使以纳秒计量，也不会因此自动获得墙上时间语义。

## Run、查询与结论

**Run**:
一次成功发布的 Workflow 执行，包含唯一 Run Manifest 和至少一个 Run Output。失败或尚未发布的候选执行不是 Run，Run ID 也只在发布成功后成立。

**Run Manifest**:
一个 Run 的唯一持久清单，记录 Run 身份及其 PACK、Workflow、迁移期可选 Dataset 关联、实际输入和 Run Output 引用。它不记录失败状态，也不承载 Analysis Result。

**Run Output**:
随 Run 持久发布的具名结构化程序产物，可来源于 Workflow 返回的 Table 或迁移期 Workflow DataFrame，并可供后续 Output Query 使用。单值使用 `main`，多个值由非空普通 dict 显式命名；Table 自身不携带 Output name。Run Output 可以保存比 Output Query JSON 结果更宽的扁平 Table 类型，因此“可以发布”不表示每种列都能未经投影直接返回为 JSON。它不是 Datasource materialization、Query Result 或面向用户的 Analysis Result。
_Avoid_: Artifact、Result

**Output Query**:
针对已发布 Run Output 发起的本地只读后续查询，不创建新 Run，也不重新执行当时的 Provider query。它的 JSON Query Result 只接受现有无损标量集合；Binary 等较宽 Output 列必须由 SQL 显式投影或 cast，非有限 float 失败。查询若使用 Run 关联的旧 Dataset，看到的是该位置当前可用的事实，而不是 Run 输入的历史快照；用户 SQL、输出规模、等待时间与本机资源消耗由调用方和用户负责。

**Query Result**:
一次 Output Query 返回的短命结构化数据。它不会成为新的 Run Output，也不是模型面向用户形成的 Analysis Result。

**Analysis Result**:
模型基于 Run Output 和必要的 Query Result 形成的面向用户判断、报告或结论。它不由 Workflow 生成，也不写入 Run Manifest。
