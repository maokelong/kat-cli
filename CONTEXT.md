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
由一个明确组织或团队拥有并独立维护的一组领域来源与分析能力，是 KAT 的自包含扩展与发布边界。它可以包含 Sources、Analysis 与 Workflows 三个能力区；边界由所有权和发布责任决定，而不是由输入格式或目录结构决定。

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

**Analysis Module**:
PACK 中把 Source table 转换为可复用分析关系或算法的可组合单元。它表达经过验证的稳定分析语义，不承担来源解释或具体用户任务编排。

**Analysis Library**:
PACK 所拥有的 Analysis Modules 集合。它是可复用分析能力的组织方式，不是独立发布边界。
_Avoid_: LIB Layer、Library Layer

**KAT Trace Analysis Library**:
KAT 向所有 PACK 平等提供的公共 Trace 分析语义，只接纳经过多个真实消费者和真实 Trace 验证的复用能力。来源解码、具体用户问题和单个 PACK 内尚未验证的候选算法不属于它。
_Avoid_: KAT Trace Library、`kat.stdlib`

## Workflow 与测试

**Workflow**:
PACK 中回答一个具体分析问题的可运行入口，解释用户输入、调用 Analysis Modules 并产生一个或多个 Run Output。Workflow 不重复声明事实依赖或拥有可复用算法，PACK 才是所有权与发布边界。

**Workflow Context**:
KAT 在一次 Workflow 执行内显式提供的执行能力入口。它只在当前执行期间有效，不是用户输入，也不存在隐式的全局当前 Context。

**Workflow arguments**:
调用 Workflow 时提供的原始具名文本输入。它们尚未表达类型、默认值或业务语义，只有选定的 Workflow 才能把它们解释为 Workflow input values。

**Workflow input values**:
Workflow arguments 经选定 Workflow 的约束解析后得到的具名、带类型且包含默认值的实际控制值。事实数据通过 Source table 提供，稳定分析策略属于 PACK，不应伪装成大量 Workflow 输入。

**PACK test**:
针对 PACK 生产 Interface 的 KAT 集成测试，通过现有 pytest `kat_run` fixture 以生产形态的 Workflow arguments 执行真实 Workflow、Source Resolution、Analysis 与输出行为。测试可以按当前 PACK Source name 提供原始 Source argv；它必须经过真实 Source Input Compiler 与 Source Entry，不能注入 Provider/SchemaProvider 对象，也不能覆盖其他 PACK 的 Source。普通 Provider、Parser、Decoder 与 Analysis Module 仍由 pytest 直接单测；PACK test 产生测试证据而不发布生产 Run，也不建立 Source 专用测试入口。

**Test Dataset**:
随 PACK 一起版本化、供 `kat_run(dataset=...)` 按需使用的普通 Dataset 工作目录。它和生产 Dataset 一样以明确的 `(PACK identity, Source name)` 空间提供 Binding 或本地物化表，不注册匿名 `dataset` namespace。它不是测试专用存储格式，也不是不运行 Workflow 的测试所必需的结构。

## 数据与事实

**Source Resolution**:
KAT Runtime 依据本次 Source Configuration，把 Dataset 中的 PACK identities 映射为 DataFusion catalogs、各 PACK 的 Source names 映射为对应 catalog 中彼此独立的 schemas，并延迟注册进当前执行的只读查询环境。Workflow 执行时所选 PACK 是当前 catalog，因此 Analysis 通常使用 `<source>.<table>`；跨 PACK 查询使用标准 SQL quoted catalog 表达 `<pack>.<source>.<table>`。生产 `kat run` 不接收临时 Source arguments 或 Provider override；一个 Source 的 Provider 只从本次显式选择的 Dataset 内对应 Source Binding 解析得到。`kat run --dataset` 语法可选且没有隐式默认：显式 Dataset 无效时在 Workflow 启动前失败；没有 Dataset 或合法 Dataset 缺少目标 Binding 时，只在 Analysis 首次实际解析该 Source schema 时以完整 PACK/Source 身份失败，在此之前不实例化 Provider、扫描表、访问远端或创建 Source staging。省略 Dataset 不会查找当前目录、Data Home、最近使用位置或创建匿名空 Dataset。Analysis 首次查询任意 PACK 的某个 Source schema 时，Runtime 才依据 Binding 的唯一当前形态取得 Materialized Source 的本地 Parquet Provider，或用当次 discovery 唯一选中的 PACK 解释 External Binding arguments、调用 Source Entry，并按限定名解析具体 TableProvider；多个 PACK 的 Source modules 使用彼此隔离的 Runtime 私有 namespace。该过程只处理已采集数据，不表示现场采集、全量搬运或内容哈希，也不是独立 CLI 命令或数据源协议。

**Source Configuration**:
一次执行中按 PACK Source name 决定如何取得唯一 Source Provider 的短命配置。生产执行只由本次显式选择的 Dataset 上对应 Source Binding 解析得到；PACK test 可以按当前 PACK Source name 显式提供原始 argv 作为隔离配置，但不会发现或读取开发者机器上的任意生产 Dataset。`kat_run(dataset=..., sources=...)` 同时提供二者时，显式 `sources` 覆盖 Test Dataset 中同名的当前 PACK External Binding 或 Materialized Source，Dataset 的其他 Bindings 继续可用；覆盖只影响本次调用，不修改 Test Dataset。测试 argv 与 Binding argv 一样经过真实 Source Input Compiler 与 Source Entry，并且都是 KAT 实际收到的 token strings；KAT 不再展开环境变量、模板或 response file。显式 override 编译失败使本次 `kat_run` 失败，不回退到同名 Dataset Binding。测试 Interface 不接受 Provider/SchemaProvider 对象，也不接受其他 PACK 的 Source override。配置不存在时不根据本地文件推断 Provider。Source Configuration 不是 Workflow input、Source table、持久 catalog 或可重建来源，也不建立同一个 Source name 的多个 alias。KAT 不按参数名或内容分类密码、Token、DSN 等凭据；Provider 可以像使用其他 Source arguments 一样使用 Binding 中保存的凭据，并继续服从外部设施自身的认证行为。
_Avoid_: DIGEST、Data Import、Ingestion

**Source Documentation**:
PACK 提供给人和 AI、用于说明已采集来源、可用事实、表与字段含义、来源入口及能力限制的文档集合。它可以只引用供应方文档，结构化 Schema 不是 PACK 合法性的前提。

**Source Guide**:
一个 PACK 的 Source Documentation 入口，供人和 AI 理解该 PACK 能提供的 Source tables，并选择或开发适用的 Workflow。PACK 只要拥有并对外发布自己的逻辑 Source namespace，就必须提供 Source Guide；这与本地是否已有结构化数据、是否需要 Source Entry 无关。KAT 对此只采用最小可执行规则：只要 PACK 存在任一 Source Entry，缺失或不可读的 `SOURCES.md` 就使精确 PACK inspection、test、bind 与 materialize 失败；没有 Source Entry 时允许 `source_guide: null`。`kat run` 不独立执行 Guide 门禁，既有 Binding 的执行不把文档变成运行依赖；正常 Skill 选择路径仍会在前置 inspection 中发现文档问题。对于仅说明并交付现有 Materialized Source、未提供 Source Entry 的 PACK，其文档责任由作者与评审保证；KAT 不为此增加 manifest 能力或扫描 Dataset。只消费其他 PACK Sources 的 Analysis-only PACK 不需要为它们重复提供 Source Guide。

**Sources**:
PACK 中把已采集数据解释并提供为 Source tables 的能力区，拥有 Source Documentation、必要的 Source Entry 与 Parser、机械解码、来源追溯及来源语义规范化。它可以复用外部设施，但不包含 Analysis Module 结果或 Workflow 产物。
_Avoid_: Fact Layer、FACT Layer

**Source Entry**:
PACK 在 `sources/` 中以 `@kat.source(name=...)` 声明普通 Python factory。它用稳定的 Source name 标识来源，并返回一个自行定义表集合、可由 DataFusion 注册的 schema-provider 值；这是 Source Entry 唯一的返回边界。Runtime 接受公开 `Schema`、Python `SchemaProvider` 与官方 FFI SchemaProvider 值，不定义自有 Provider 协议。Source Entry 的具名函数参数、受支持的 Python 类型注解和默认值共同构成 Source input contract；Source Configuration 提供的原始 Source arguments 依据该合同解释，并复用 Workflow Input Compiler 已有的标量转换规则，另外支持 Source-only 的 `pathlib.Path` 与表示可重复多文件 option 的 `tuple[pathlib.Path, ...]`。普通 `str` 不被猜测为路径，其他容器、结构化 model 与自定义 parser 不属于第一版合同。用户说明仍只写在 `SOURCES.md`，decorator 不增加 `parameters=` mapping；`kat materialize` 也直接依据同一合同解释其 `--` 后参数。Source name 是直接用于 DataFusion schema 的 lowercase snake_case identity，完整匹配 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`，在 PACK 内唯一，且不能使用 KAT 保留的 `dataset` 或 DataFusion 保留的 `information_schema`；PACK 与 Workflow identity 继续使用各自既有的 kebab-case。移动实现文件不改变 Source identity。Source operation 可以同时加载多个 PACK，因此 `sources/` 及其内部 helper 使用包内相对导入；公开 `kat.pack` 只表示当前 Workflow PACK，不是跨 PACK Source module identity。只有 KAT 需要根据外部输入构造 Provider、建立 External Binding 或新建/替换 Materialized Source 时才要求匹配的 Source Entry；已经合规交付的 Materialized Source 可以在没有 Source Entry、甚至没有对应 PACK 的情况下被检查、查询和复制，但 KAT 不能重建它，丢失或失效后的重新交付由数据供应方或操作者负责。Source Entry 不是 Workflow 或 Analysis Module 调用的 KAT 可执行入口，也不形成独立于 DataFusion 的跨 PACK Provider 协议；它只作为 Runtime 内部来源入口和显式 Materialization 目标。

**Source Provider**:
Source Resolution 为本次执行取得的 DataFusion SchemaProvider，拥有来源表的定义并按名称提供具体 TableProvider。它直接来自 Materialized Source 的 Dataset Provider，或来自以 Binding/测试 argv 调用的 Source Entry；`kat_run` 不接受绕过 Entry 的 Provider 对象。每个 Source name 在一次 Source operation 中最多取得一个 Provider，并只在该次执行内复用。未查询的 Source 不实例化，缺少必填 Source arguments 也只在首次使用该 Source 时失败。Provider 可以绑定数据库、文件集合或 Parser，并把读取继续推迟到实际表查询；其行为服从 DataFusion 及所用设施，而不是 KAT 自定义 Provider 协议。

同一个 Source Provider 可以明确支持多种物理取得方式，例如原始文件、远端设施或已有 Materialized Source，并继续提供同一组逻辑 Source tables。KAT 不依据 Dataset 内容或来源记录自动推断这种替换，也不把 Dataset 隐式改名为某个 Source namespace。

一个 Source Provider 也可以聚合一份或多份同类物理输入。文件路径、连接方式与解析策略等物理配置留在 Provider 内；设备、采集、实验、快照等 Analysis 确实需要区分的来源事实以稳定字段或关联 Source table 表达。`baseline`、`candidate` 等只属于具体分析的临时角色，不形成 Source namespace 或持久来源身份。第一版一次 Run 或 Dataset Query 只选择一个 Dataset，不组合两个分别包含 Materialized Sources 的既有 Dataset；需要横向比较的数据必须先由同一个 Source Provider 聚合，并进入同一 Dataset 的一个 Binding。KAT 不增加多 `--dataset`、Dataset alias、merge 或 overlay。

**Source namespace**:
当前执行中标识一个具体 Source Provider 的 DataFusion schema。PACK identity 原样映射为 DataFusion catalog，SQL-safe Source name 原样映射为其中的 schema；KAT 不创建 kebab-to-snake alias、清洗名或第二身份。Workflow 执行时所选 PACK 是当前 catalog，因此同 PACK Analysis 使用 `<source>.<table>`，例如 `raw_smaps.mappings`；跨 PACK 或 Dataset Query 使用标准 SQL identifier quoting 表达完整三段名称，例如 `"kat-kernel".raw_smaps.mappings`。不同 PACK catalogs 或 Source schemas 可以拥有同名表，Runtime 不把它们扁平合并、按优先级覆盖或隐式选择。目标架构不注册匿名 `dataset` schema；所有持久事实必须归入明确的 PACK catalog 与 Source schema。

**Source Binding**:
由一个具体 Dataset location、PACK identity 与 Source name 共同标识的、当前唯一的 Provider 取得方式。Binding 只绑定 `(PACK identity, Source name)`，不保存 bind 时的 PACK directory、代码、版本或哈希；`--pack-dir` 也不持久化。以后每次需要解析 External Binding 时，都由当次 discovery 唯一选中的同名 PACK 解释 argv 并取得其中的凭据；多个不同的同名 candidate 没有优先级并使 discovery 失败，因此调用方必须信任执行时选择的 PACK。一个 Binding 恰好处于两种互斥形态之一：External Binding 保存能够按当前 Source input contract 调用 Source Entry 的原始 Source argv token array，以及 `kat bind` 时取得的绝对工作目录；Materialized Source 保存由 KAT 管理的本地 Parquet tables，并保留同样的原始 argv 与工作目录作为 REDO recipe。Recipe 只供再次 Materialization 取得 Provider，不参与查询，也不是权威 provenance、版本锁或一致性保证。原始 argv 是 KAT 进程实际收到、已经由 shell 或调用方完成分词和可能变量展开的 token strings；KAT 在 bind、run 与 materialize 重放时不再次展开环境变量、模板或 response file。argv 保持 token 边界且不补写调用方省略的默认值，可以包含密码、Token、DSN 等 Provider 声明的配置；普通相对 `pathlib.Path` 始终以保存的工作目录为基准，普通 `str` 不作路径解释。一个 Dataset 可以容纳多个由不同 `(PACK identity, Source name)` 标识的 Binding，但同一对标识最多一个，不形成同一 PACK Source 的多实例 catalog。已有 Binding 时，`kat bind` 与 `kat materialize` 默认拒绝改变它，只有显式 `--replace` 才授权以破坏性方式完整替换该 Source 的当前形态。`kat bind --replace` 绝不合并或继承旧配置：本次 `--` 后 argv 是完整的新显式配置，没有 argv 就表示零个显式参数并由当前合同使用当前默认值；不能只更新密码而沿用旧 host。PACK discovery、Guide、Source contract、参数编译、Dataset 目标或 `--replace` 授权等写入前机械检查失败时不修改 Binding；进入 Provider 执行后不再按阶段承诺保留原 Binding，任何写入开始后的失败都可能使 Source 空间或 Dataset 无效。任意内存 Provider 不能成为持久 Binding。Materialized Source 的通用 Parquet 查询不要求对应 PACK 已安装；运行其 Workflow、解析 External Binding 或执行保存的 REDO recipe 时仍必须取得对应 PACK。`kat run` 从显式选择的 Dataset 取得对应 Bindings，并解析为本次 Source Configuration，省略 Dataset 不选择默认位置；PACK test 只读取显式选择的 Test Dataset 和本次 `sources`，不发现任意本机生产 Binding。Binding metadata 由 KAT 管理并以明文保存在 Dataset 目录内；KAT 不增加加密、Keychain、Secret Manager 或凭据刷新，也不对 Source arguments 或凭据提供任何保密、防泄漏、凭据保护或安全擦除承诺。完整复制 Dataset 会复制 Binding 及其中的凭据，复制品以新位置成为独立 Dataset，但不重基相对 Path；移动或重命名 Dataset 是未定义行为，KAT 不检测、修复或改写引用。路径、连接配置或凭据在另一台机器上不可用时明确失败并由用户重新绑定，不做自动修复。PACK 修改 Source contract、默认值或实现后，既有 recipe 的行为未定义；PACK 作者与 Dataset 操作者负责同步调整并完整执行 `kat bind --replace` 或 `kat materialize --replace`，KAT 不检测差异、不冻结默认值、不维护版本、不迁移或兼容旧 Binding。`kat bind` 只建立 External Binding，并可以在目标路径不存在时创建合法的空 Dataset；Materialized Source 统一由 `kat materialize` 建立。两条命令都通过 `--pack`/`--source` 选择匹配 Entry，接受只作用于本次 discovery 的可重复 `--pack-dir`，并把 `--` 后 token 作为 Source arguments；bind 加载 PACK 并编译合同但不调用 Provider，materialize 才消费 Provider。Dataset Inspection 与 bind 的成功 KAT Response `result` 仍服从各自固定的精确字段形状；这只是接口 schema，不构成对 Source arguments、bind 工作目录或凭据的保密承诺。

**Materialized Source**:
Source Binding 的本地持久形态，由目标 Dataset 中对应 `(PACK identity, Source name)` 隔离空间内每表一个 `<table>.parquet` 提供统一查询面。它允许被后续进程继续查询，但只是可丢弃并通过 REDO 重新生成的本地投影或缓存，不是权威来源、来源的历史快照、独立 Dataset 或 KAT 系统状态。Materialized tables 是该 Source 唯一查询面并完全遮蔽保存的 REDO recipe；未物化表明确不存在，Runtime 不回退 Source Entry。任意外部 Parquet 目录不会自动获得该身份；新建或替换时必须由匹配 Source Entry 提供 Provider 并统一经过 Materialization。读取失败或 inspection 判定无效时，用户删除 Dataset 并用保存或重新提供的当前配置 REDO，KAT 不修复或恢复旧状态。

**Dataset**:
位于一个本地管理位置、以其位置为身份的持久数据工作目录。它可以容纳零个或多个按 `(PACK identity, Source name)` 隔离的 Source 管理空间；每个空间保存对应 Binding metadata，并可包含该 Source 已 Materialize 的 Parquet tables。空 Dataset 是合法状态，Binding metadata 不是 Source fact 或 table。完整目录复制产生一个以新位置为身份的独立 Dataset，并原样复制 Binding metadata；移动或重命名现有 Dataset 是未定义行为。Dataset 可以只覆盖外部来源中与任务有关的切片；外部来源、Source staging、Run Output 和 Analysis Result 都不是 Dataset。各 Source 空间的具体物理目录布局由 KAT 管理，不成为 PACK Interface。第一版不提供写入并发锁、CAS、读写快照、备份、回滚、崩溃恢复或 Dataset relocation；调用方必须串行化同一 Dataset 的写操作，写入失败或中断造成的无效 Dataset 由 inspection 明确拒绝并由用户删除后 REDO。第一版也不提供 Source 粒度的解绑、删除或物化回收操作；需要清理时只删除整个 Dataset，取得真实细粒度需求后再设计。

**Dataset Inspection**:
`kat inspect --dataset` 对一个 Dataset 当前管理事实的只读机械投影。成功 KAT Response `result` 精确包含 canonical `path` 与按 PACK identity、Source name 排序的 `sources` array；每项以 `kind` 区分 External Binding 与 Materialized Source。External 项精确只有 `pack`、`source` 与 `kind: "external"`，且不实例化 Provider；Materialized 项另含按表名排序的 `tables`，每张表从 Parquet metadata 投影 `name` 与 `columns`。该字段形状不是 Source arguments 或凭据的保密保证。该操作不发现或加载 PACK；任一 Binding metadata 或受管理 Parquet metadata 损坏时整体失败，不返回部分 inspection。

**Source table**:
当前执行环境以完整 `<pack>.<source>.<table>` 名称标识、并允许当前 PACK Analysis 简写为 `<source>.<table>` 的只读事实关系，以 DataFusion 的表查询面作为统一运行时边界。它可以来自 Dataset 或本次执行的短命来源；Analysis 明确依赖逻辑 Source namespace，但具体读取、查询下推、认证与一致性行为仍由 Provider 及所用设施决定。

**Source staging**:
Source Provider 把当前任务所需的外部查询或解析结果暂存为本地 Parquet，以便继续提供 Source table。它随执行丢弃，不是 Dataset、Materialization、Run Output 或可跨执行重建的来源状态。

**Arrow reader helper**:
KAT 为 Python Parser 提供的薄 helper：它接收“表名到零参数 reader factory”的 mapping，每个 factory 返回标准 `pyarrow.RecordBatchReader`，并构造一个 DataFusion SchemaProvider 供 Source Entry 返回。mapping 只是 helper 输入，不是 `@kat.source` 的第二种返回类型，Runtime 不对 Entry 结果做 union 或 duck typing。表首次被查询时，helper 构造的 Provider 按 RecordBatch 增量写入本次 Source operation 的 staging，成功后交给既有 Parquet Provider，并在该次执行内复用；普通作者不负责组装 staging、Parquet 注册、失败失效与清理，高级作者直接返回既有 SchemaProvider。它不是新的 Parser、row、generator 或多表 sink 协议。

**Fragment**:
现有设施或领域 Parser 在一份已采集输入中识别的可独立扫描或解析部分。多个 Fragment 可以在外部 Provider 内共同组成一个 Source table，但本身不表达顺序、去重或完整性承诺；KAT 管理的 Materialized Source 不用 Fragment directory，每张表只保存一个 Parquet 文件。

**Materialization**:
用户显式完整消费一个 Source Provider 的表，并将目标 `(Dataset location, PACK identity, Source name)` 的当前 Binding 替换为 Materialized Source 的操作。Provider 输入按固定顺序选择：`--` 后存在 Source arguments 时使用本次 arguments，并以本次调用的工作目录解释相对 Path；否则当前是 External 或 Materialized Binding 时重放其中保存的原始 argv，以保存的工作目录解释相对 Path，并由当前 Source contract 补齐当前默认值；否则目标没有 Binding 时调用零参数或全默认值 Source Entry，并在缺少必填参数时失败。既有 recipe 只描述 PACK 未修改 Source contract、默认值或实现时的正常路径，发生修改后的行为未定义。已有任何 Binding 时仍必须显式 `--replace`。省略 table 选择时，发布 Provider 在本次操作开始时枚举的全部表；显式给出一个或多个 table 时只发布该子集，并由该子集构成新的完整 Materialized Source。Dataset 目标、PACK/Source、Guide、Source contract、arguments 与 `--replace` 授权等可在执行第三方代码和写入前判断的机械前置条件必须先完成，失败时不修改 Binding。进入 Provider 执行后，Provider 创建、枚举、读取、解析或扫描失败不再享有独立的保旧合同；一旦任何写入开始，后续任一 Parquet 写入、机械检查、删除、移动或 metadata 写入失败，以及进程崩溃、强制终止或断电，都可能使目标 Source 空间或 Dataset 无效。第一版不承诺持久原子性、备份、回滚或恢复。只有所选表集合各自完整写入一个 Parquet 文件，并能通过既有 Dataset marker、命名与 Parquet metadata 机械读取检查时，Materialization 才成功；这不证明来源一致性、覆盖范围或业务完整性。成功后 Materialized Binding 保存实际使用的原始 argv 与工作目录作为 REDO recipe，同时只用本次表集合形成查询面；旧本地表不再可见，但不承诺安全擦除底层残留、历史副本、日志或备份。其他 Source 不被有意修改。成功 KAT Response `result` 精确投影 canonical `path`、`pack`、`source`、`kind: "materialized"` 与按名称排序的 `tables` string array，不携带 Schema、行数、字节数、耗时或 `replaced`；完整 Schema 通过 Dataset Inspection 取得。Materialization 复用与普通查询相同的 Source Provider，不要求 Source 声明 eager/lazy 模式，也不在第一版增加 bulk execution profile、分区 Parser 或 KAT 自有并行协议；性能暂由 DataFusion、Arrow、Parquet 与 Provider 的既有行为决定。它不会由 KAT 自动创建、更新、刷新或跟随外部来源变化。

**Derived DataFrame**:
Workflow 在当前执行中从 Source table 或其他关系派生的临时关系。它只有被 Workflow 返回并随成功 Run 发布后才成为 Run Output，否则随本次执行结束而消失。

**Trace fact**:
PACK Sources 从原始 Trace 直接解码或跨记录规范化得到的来源事实。它仍是来源数据，不是预先计算的分析结果。

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
一个 Run 的唯一持久清单，记录 Run 身份及其 PACK、Workflow、可选 Dataset 关联、实际 Workflow inputs 和 Run Output 引用。Source Configuration 与 Source Binding 都不构成可重建来源或来源快照；清单也不保存权威 REDO 配方，不记录失败状态或承载 Analysis Result。

**Run Output**:
随 Run 持久发布的具名结构化程序产物，可供后续 Output Query 使用。它允许被后续进程继续查询，但作为可丢弃并通过 REDO 重新生成的本地投影或缓存，不是权威事实或历史快照；读取失败时丢弃并 REDO。它即使以 Parquet 保存也不是 Dataset、Query Result 或面向用户的 Analysis Result，当前也不能直接充当另一个 Workflow 的 Source table。
_Avoid_: Artifact、Result

**REDO**:
由调用方、数据供应方或自动化重新执行 `kat bind`、`kat materialize` 或 `kat run`，重新生成当前可用的 Materialized Source 或 Run Output。Materialized Binding 可以保留上次 Source argv 与工作目录作为便利 recipe，但它不是权威重建配方、版本锁或来源快照；KAT 不自动修复或恢复，也不保证 REDO 与历史产物在字节、数据快照、PACK 语义或查询结果上相同。调用方可以显式替换配置；没有 Source Entry 的预制 Materialized Source 只能由供应方重新交付。

**Output Query**:
以 `kat query --run` 针对已发布 Run 发起的本地只读后续查询，不创建新 Run。它始终注册 `output.*`，并在 Run 关联的 Dataset 当前可用时注册其中的 External 与 Materialized Sources；只有 SQL 实际解析 External Source 时才按当次 discovery 加载对应 PACK。查询看到的是该 Dataset 位置当前的 Bindings，而不是 Run 输入的历史快照；用户 SQL、输出规模、等待时间与本机资源消耗由调用方和用户负责。

**Dataset Query**:
以 `kat query --dataset` 直接查询一个 Dataset 中 External 或 Materialized Sources 的只读操作，不需要 Run。纯 Materialized 查询不要求对应 PACK；实际解析 External Binding 时，Runtime 才从 Bundled PACK、Data Home PACK 与可重复 `--pack-dir` candidates 中唯一发现并加载对应 PACK。SQL 使用标准 quoted catalog 的完整三段名称，例如 `"kat-kernel".raw_smaps.mappings`；Dataset Query 不创建 Run、Run Output 或新的 Dataset。

**Query Result**:
一次 Output Query 或 Dataset Query 返回的短命结构化数据。它不会成为新的 Run Output，也不是模型面向用户形成的 Analysis Result。

**Analysis Result**:
模型基于 Run Output 和必要的 Query Result 形成的面向用户判断、报告或结论。它不由 Workflow 生成，也不写入 Run Manifest。
