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
KAT 在一次 Workflow 执行内显式提供的执行能力入口。Datasource factory 通过唯一构造入口 `ctx.provider(executor)` 创建 operation-bound Provider facade；成功的 `Provider.query()` 会把具名本地结果自动加入它的 operation-local catalog，`ctx.sql()` 提供本地 DataFusion 融合能力。只读 `ctx.datasource_root` 返回当前 PACK 在所选 KAT Data Home 下的长期存储根，不暴露平台其他目录。任一 Provider query 在返回 `Table` 前失败都会使 Context 不可发布，即使 PACK 捕获异常也不能继续查询或产出成功 Run。Context 只在当前执行期间有效，不是用户输入，也不存在隐式的全局当前 Context。

**Required tables**:
迁移期 Workflow 对旧 Dataset Source table 的完整、精确声明。KAT 在调用 Workflow 前继续强制这组 Table Grant；它不声明 Provider、Source query 或 Query result name。空声明表示不需要旧 Dataset relation，不表示 Workflow 不使用 Datasource Provider。

**Workflow arguments**:
调用 Workflow 时提供的原始具名文本输入。它们尚未表达类型、默认值或业务语义，只有选定的 Workflow 才能把它们解释为 Workflow input values。

**Workflow input values**:
Workflow arguments 经选定 Workflow 的约束解析后得到的具名、带类型且包含默认值的实际控制值。迁移期旧来源事实仍由 Dataset 提供；新 Datasource 可以让输入选择 Workflow 已准入 Provider 的非敏感数据位置或来源生态 selector。凭据和任意 Provider 代码不是 Workflow input values。

**PACK test**:
针对 PACK 生产 Interface 的 KAT 集成测试，可以按所测边界使用 Test Dataset 或 Provider fixture 执行真实 Workflow 行为。它产生测试证据而不发布生产 Run。

**Test Dataset**:
随 PACK 一起版本化、供测试按需使用的普通 Dataset。迁移期它继续验证旧 Dataset/Table Grant 边界；Provider fixture 不替代或转换它。

**Provider fixture**:
随 PACK 一起版本化、供测试结合显式 Context fixture 创建 Datasource Provider facade 的受控输入与非敏感配置。它不是平台持久状态，也不规定 Source executor 的存储格式。

## 数据与事实

**Data Import**:
迁移期由一个显式选定的旧 Datasource type 把外部来源完整转换为新 Dataset，或整体替换已有 Dataset 的用户操作。新 PACK Datasource/Provider 不注册成这种封闭类型，也不通过 `kat import` 创建 Provider Table。

**Datasource**:
新模型中由 Workflow 直接 import 的 PACK 普通 Python 模块或工厂，定义外部事实如何定位、解释并暴露为可查询关系。Provider factory 显式接收当前 `ctx`，把 Datasource-owned Source executor 组合成 operation-bound KAT Provider facade；除此之外它没有 KAT 平台身份、注册或发现机制。跨 Datasource 的关系组合属于 Workflow Runtime。迁移期旧 Data Import Datasource type 仍独立存在；它不是这里的插件机制或 Provider variant。

**Datasource Provider**:
Datasource factory 把 Source executor 交给唯一入口 `ctx.provider(executor)` 后得到的 operation-bound KAT facade，通过同步的 `query(SQL, name=...) -> Table` 提供统一单表查询能力。它可以组合远端数据库 executor，也可以组合使用私有 DataFusion 查询本地多表根目录的 executor；`query()` 返回前已经完成来源执行、Runtime 默认本地化、自动注册和 query-local 资源关闭。Provider facade 由 KAT 固定实现，PACK 不能自行构造、继承或替换；它不是持久平台状态，不能跨 operation 复用，并在 operation 结束时关闭 Source executor。

**Source executor**:
Datasource-owned、由 PACK 作者按公开结构协议实现并由 Provider facade 调用的来源执行组件，拥有 SQL 方言、内部 catalog、解析、缓存、连接、类型转换和来源特定长期物化策略。其 `execute()` 只以 context-managed `pyarrow.RecordBatchReader` 或 `ParquetSource` 交付一个具有明确 Schema 的单表结果；它不接收 Query result name，不知道 Runtime landing path、Run Output 发布或融合 DataFusion Session。query-local 资源随 `execute()` context 退出而关闭，可复用的 operation-scoped 资源由 facade 在 operation 结束时通过 executor `close()` 关闭。PACK 不需要继承 KAT 基类，通过 `ctx.provider(executor)` 把它接入当前 operation。

**Source catalog**:
Source executor 私有的来源 relation 集合及名称映射，只供该 executor 的 Source query 使用。它可以来自 Parser 返回的显式表索引、普通 Mapping、Datasource 自己的目录约定或远端 Database catalog；KAT 不扫描、枚举或规定其目录布局，也不把其中关系自动注册到融合 Session。一次查询必须先由 executor 把它收敛成一张 Source execution result。

**Source execution result**:
Source executor 在一次 `execute()` context 中交给 Runtime sink 的单表结果，只能是提供 Schema 与流式 batches 的 `pyarrow.RecordBatchReader`，或消费期间保持不可变的既有单表 `ParquetSource`。它不是整表 `pyarrow.Table`、DataFusion DataFrame、远端 cursor、Runtime Output 路径或延迟来源查询计划；Runtime 必须在退出 context 前完成消费。

**Source query**:
Workflow 调用 `Provider.query()` 同步执行的一条来源内只读 SQL statement。它使用对应 Source executor 的方言、占位符、参数绑定与执行方式；来源结果先写入私有 partial，只有 sink、executor context、最终文件与自动注册全部成功后，调用才返回一张具有明确 Schema 的本地 `Table`。此前任一步失败都会使当前 Context 不可发布，不提供 Workflow 级捕获后重试。PostgreSQL SQL 在目标 Database 内完成同库 Join 与过滤，本地 Parquet executor 则在自己的多表 catalog 上使用 DataFusion。KAT 不解析、改写、翻译、拆分或下推 Source query；executor 必须使用底层执行器安全绑定值而不是拼接 SQL，并负责把调用限制为一个只读表结果。Source query 来自受信任 PACK，不是不可信 SQL 的通用安全沙箱。

**Query result name**:
`Provider.query(name=...)` 为一张来源查询结果指定的 operation-local 名称，同时作为候选 `outputs/<name>.parquet` backing path 名、`Table.name` 和本地 DataFusion 关系名；若该 Table 被 Workflow 返回，它也成为 Run Output 名。缺省值是仅基于原始 SQL UTF-8 字节的 `q_<sha256>`；名称重复在来源 I/O 前失败，不覆盖或复用已有结果。它不是 Provider 内部表名、远端关系身份或跨 Run 持久身份。

**Fusion query**:
Workflow 在需要联合多张 `Table` 时通过 `ctx.sql(sql)` 定义的本地 DataFusion SQL。它直接按 Query result name 引用当前 operation 中已成功落盘并自动注册的结果，不接收表 Mapping；不存在的关系直接 table-not-found，不会触发 Provider 查找、来源 SQL 执行或其他回退。Fusion query 不能透明引用 Provider 的内部 catalog 或远端原始表，并保持现有 `datafusion.DataFrame` 返回类型。

**Datasource materialization**:
Datasource 可选生成在当前 PACK 的 `ctx.datasource_root` 下、用于跨 Workflow 重用的来源特定文件或目录。生产根固定为 `KAT_DATA_HOME/datasources/<pack-name>/`，测试映射到当前 pytest test 的 `tmp_path`；更深层布局、artifact key、格式、完整性、版本、重用与清理语义均属于 Datasource。它不是 KAT 平台状态、统一 Provider 能力或 Run Output，也不同于 Runtime 在 `Provider.query()` 返回前生成的候选 `outputs/<name>.parquet`。

**Dataset**:
迁移期由旧 Data Import 完整生成或整体替换的本地事实集合，是现有 `required_tables` Workflow 的事实输入。其具体位置就是身份，不另设 Dataset ID；Provider Table、Run Output 和 Datasource materialization 都不属于 Dataset。

**Source table**:
旧 Dataset 在迁移期通过 Table Grant 向 Workflow 提供的不可变事实关系。它与新 Provider Table 共用当前 operation catalog，但 Provider 不覆盖、不 shadow 或转换它。

**Table**:
一次 `Provider.query()` 返回的已本地化、不可变具名单表关系句柄，只公开只读 `name` 与 Arrow `schema`。它的 `name` 已用于候选 Parquet backing path 和 operation-local 自动注册；该 backing 可以是单文件或单表 Parquet dataset 目录，但路径与物理差异不进入 Table 或 Manifest。直接返回时同一名称就是 Run Output 名，Mapping key 也必须与之相同。它不表示整表已驻留内存，不包含待执行的来源 SQL，也不持有来源连接。它可以直接成为 Workflow Output，也可以由 Fusion query 按名读取；所有消费者复用同一 backing，Runtime 不会再次访问来源。只有被 Workflow 返回并随成功 Run 发布的 Table 才成为 Run Output；仅用于融合或未使用的 backing 在 DataFrame Output 完成后尽力清理，清理失败只产生 Operation log warning。创建它的 eager `query()` 即使结果最终未使用也已经执行。Table 不是远端原始表、Provider 内部 catalog、现有 DataFusion DataFrame 或持久平台身份。

**Workflow DataFrame**:
现有 `ctx.sql()`、`ctx.from_arrow(pyarrow.Table)` 或其他既有 Workflow DataFusion API 产生的 `datafusion.DataFrame`。它继续使用当前 Session、惰性执行和 Run Output materializer；单个 DataFrame 的 Output 名仍是 `main`，Mapping 中由 key 命名，且这些名字不得与当前 operation 中任何 Provider `Table.name` 重复。Provider 大结果不能借此接口先收集成内存 `pyarrow.Table`。Workflow DataFrame 与 KAT Table 是 Runtime 支持的两种关系值，不要求作者互相包装。

**ParquetSource**:
Source executor 通过 `ParquetSource(path)` 向 Runtime sink 交付既有稳定 Parquet 的不可变值对象，只公开指向一个文件或单表分片目录的只读 `path`。query scratch 内的来源通过 move/rename 向 Runtime 转移所有权；scratch 外的借用来源必须通过具有写时分离语义的 clone 或字节复制形成独立 backing。hard link 和 symbolic link 不允许，因为它们不能保证 `Table` 在 executor context 结束后仍不可变。单表分片目录可以原样成为 `outputs/<name>.parquet` backing path，Runtime 与 Manifest 不公开它和单文件的差异。该路径避免解码后重写，但不承诺任意文件系统零拷贝。整个多表根目录不是 ParquetSource；它由 Source executor 的私有 catalog 解释，必须先经来源 SQL 收敛成一张表。

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
随 Run 持久发布的具名结构化程序产物，可供后续 Output Query 使用。直接返回的 Provider Table 使用 `Table.name`；若位于 Mapping 中，key 必须与 `Table.name` 相同。DataFrame 继续使用单值 `main` 或 Mapping key，但不得与任意 Provider Table 名重名。Run Output 不是 Datasource materialization、Query Result 或面向用户的 Analysis Result。
_Avoid_: Artifact、Result

**Output Query**:
针对已发布 Run Output 发起的本地只读后续查询，不创建新 Run，也不重新执行当时的 Provider query。查询若使用 Run 关联的旧 Dataset，看到的是该位置当前可用的事实，而不是 Run 输入的历史快照；用户 SQL、输出规模、等待时间与本机资源消耗由调用方和用户负责。

**Query Result**:
一次 Output Query 返回的短命结构化数据。它不会成为新的 Run Output，也不是模型面向用户形成的 Analysis Result。

**Analysis Result**:
模型基于 Run Output 和必要的 Query Result 形成的面向用户判断、报告或结论。它不由 Workflow 生成，也不写入 Run Manifest。
