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
KAT 在一次 Workflow 执行内显式提供的执行能力入口。它只在当前执行期间有效，不是用户输入，也不存在隐式的全局当前 Context。

**Required tables**:
Workflow 对其入口及同 PACK 内部调用所需 Source table 的完整、精确声明。KAT 在调用 Workflow 前强制受支持的表访问；这是正确性约束而不是安全沙箱，空声明表示确实不需要表。

**Workflow arguments**:
调用 Workflow 时提供的原始具名文本输入。它们尚未表达类型、默认值或业务语义，只有选定的 Workflow 才能把它们解释为 Workflow input values。

**Workflow input values**:
Workflow arguments 经选定 Workflow 的约束解析后得到的具名、带类型且包含默认值的实际控制值。数据属于 Dataset，稳定分析策略属于 PACK，不应伪装成大量 Workflow 输入。

**PACK test**:
针对 PACK 生产 Interface 的 KAT 集成测试，可以使用 Test Dataset 执行真实 Workflow 行为。它产生测试证据而不发布生产 Run。

**Test Dataset**:
随 PACK 一起版本化、供测试按需使用的普通 Dataset。它不是测试专用存储格式，也不是不运行 Workflow 的测试所必需的结构。

## 数据与事实

**Data Import**:
由一个显式选定的 Datasource 把外部来源完整转换为新 Dataset，或整体替换一个已有 Dataset 的用户操作。

**Datasource**:
识别、读取并规范化一种外部来源的代码与语义边界。它拥有来源事实、重复数据以及失败、缺席表或零行表之间的判断，并通过 Data Import 产生 Dataset。

**Dataset**:
由一个 Datasource 通过 Data Import 完整生成或整体替换的本地事实集合，是 Workflow 的事实输入。其具体位置就是身份，不另设 Dataset ID；Run Output 和 Analysis Result 都不属于 Dataset。

**Source table**:
Dataset 向 Workflow 提供的不可变事实关系。Workflow 可以从中派生数据，但不能增加、更新、删除或替换它；只有显式 Data Import 可以整体替换所属 Dataset。

**Derived DataFrame**:
Workflow 在当前执行中从 Source table 或其他关系派生的临时关系。它只有被 Workflow 返回并随成功 Run 发布后才成为 Run Output，否则随本次执行结束而消失。

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
一个 Run 的唯一持久清单，记录 Run 身份及其 PACK、Workflow、可选 Dataset 关联、实际输入和 Run Output 引用。它不记录失败状态，也不承载 Analysis Result。

**Run Output**:
随 Run 持久发布的具名结构化程序产物，可供后续 Output Query 使用。它不是 Dataset、Query Result 或面向用户的 Analysis Result。
_Avoid_: Artifact、Result

**Output Query**:
针对已发布 Run Output 发起的有界、只读后续查询，不创建新 Run。查询若使用 Run 关联的 Dataset，看到的是该位置当前可用的事实，而不是 Run 输入的历史快照。

**Query Result**:
一次 Output Query 返回的短命结构化数据。它不会成为新的 Run Output，也不是模型面向用户形成的 Analysis Result。

**Analysis Result**:
模型基于 Run Output 和必要的 Query Result 形成的面向用户判断、报告或结论。它不由 Workflow 生成，也不写入 Run Manifest。
