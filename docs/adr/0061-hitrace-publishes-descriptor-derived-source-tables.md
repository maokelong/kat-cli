---
status: accepted
---

# Hitrace 发布完整的 descriptor-derived Source tables

Hitrace Datasource 把完整、可独立消费的来源面视为平台能力。对每个显式注册的 fully-qualified protobuf payload root，KAT 必须依据随当前 KAT 固定的 descriptor，生成确定的关系映射，并将其 reachable closure 的完整 decoded protobuf semantics 发布为不可变 Source tables；不得再按当前 Workflow 或 PACK 的字段需求裁剪。只为至少产生一行来源 occurrence/fact 的 root 或 relation 创建表；空或全默认值的 bound root 仍算一个 occurrence。仅当实际发布的表含需要解释的 enum field 时创建 definition table，并保存该 field 的完整 descriptor definitions，不按本次出现值裁剪。

这里的“完整”只指当前固定 descriptor 和 typed protobuf decoder 能表达的语义，包括已知字段值、显式 presence、oneof 选择、repeated 元素及其顺序；它不是 protobuf wire archive，不保留未知 wire fields、原始字段顺序、原始编码或 proto3 implicit scalar 的 wire presence。已注册 root 存在无法映射的 reachable shape 时构建失败；运行时对已绑定 payload 的解码、关系化或写入失败时，整个 Data Import 失败。合法但未注册的 plugin 或 section 继续按 ADR-0025 报告并跳过。

每个成功绑定的 profiler payload occurrence 必须发布一条独立 envelope occurrence provenance，并使 payload root 可连接到它实际来自的 `ProfilerPluginData` occurrence；即使 decoded root 为空或全是默认值，两行 occurrence 也必须保留。`profiler_payload_occurrence` 是 transport envelope 的受控 provenance projection，不是 descriptor-derived protobuf root，也不进入 relational plan。私有 capture adapter 独占其固定 Schema、typed row、逻辑字节估算、descriptor-derived enum definitions、enum origin binding 与 relation slot；调用者只能提交成功绑定的 envelope 并通过 adapter 写入 root，不得组合 relation specifications、推导 relation slot 或绑定 enum origin。envelope 的 `data` 是被 typed root 解码替代的传输 bytes，不在 provenance relation 中重复保存；payload 内部的普通 `bytes` field 仍必须完整保留。`clock_id`、`tv_sec`、`tv_nsec` 等来源字段可以作为原始来源值出现，但不因此成为 payload event time、`UnifiedClock` 或可跨 clock domain 比较的 KAT 时间。规范化时间事实仍只使用 ADR-0042 的 `clock_domain + clock_value`。

Descriptor-derived Source tables 与 `sched_switch` 等规范化 Trace facts 是两种不同契约：前者机械保留单条 protobuf 来源语义，后者只在来源解释、跨记录规范化或复用价值已有证据时由 Datasource 另行发布。两者可以来自同一次 decode 并共存，但必须使用不同表名；机械表不取得分析语义，规范化表也不成为第二份完整 protobuf 镜像。跨记录的分析策略继续属于 `kat.trace` 或 PACK。

关系键只用于恢复当前 Dataset 内的来源结构，不是 protobuf identity、业务 identity 或跨 Import 稳定键。Dataset 不保存 descriptor digest、mapping revision、catalog 或 parent-table metadata。表名、列 Schema 和每张 child relation 的唯一父表由当前版本的静态映射合同规定；Parquet Schema 只承载列结构，不承载 parent-table metadata。Descriptor-derived 表不承诺在不同 KAT 版本间保持表名、Schema 或仍可由新版映射按同一合同解释，也不提供迁移；这不改变 ADR-0020 对合法 Dataset 的物理解析与 inspection 合同。

## 方案选择与后果

本决定比较了三种关系映射路径：runtime descriptor 加 generic value tree、按 root 手写 Arrow tables，以及 build-time relational plan 加 generated typed emitter。比较重点是运行时是否需要 descriptor 解释、字符串路径查找或通用中间值，Schema 与 emitter 是否共享唯一映射规则，对 prost generated naming 的耦合如何受控，以及新增 root 或 protobuf shape 时的维护与构建诊断成本。

选择 build-time relational plan 加 generated typed emitter：planner 只把 reachable descriptor closure 转换为关系计划，该计划是 descriptor-derived payload roots 及 descendants 的 table topology、Schema、emitter 和 enum origin 的唯一映射真相；prost binding 只把计划连接到 generated Rust types，不决定物理 topology；renderer 只根据计划生成 typed field access，不建立第二套映射规则。这使不支持的 reachable shape 及 Schema/binding 不一致在构建期失败，并使运行时无需遍历 descriptor、查找字符串 field path 或构建 generic value tree。runtime descriptor 方案不依赖 generated naming，但会把映射解释和通用值表示带入运行时；手写 Arrow tables 则会让每个 root 的 generated Rust field access、Schema 与写入逻辑各自直接耦合，重复 descriptor 映射规则并增加漂移风险。

Profiler envelope provenance 不扩展成通用 projection language。它由一个私有 capture adapter 在 descriptor-derived layout 外追加固定 occurrence relation，并复用构建期生成的 descriptor enum symbols。Relational plan 与 capture adapter 分别是各自作用域的唯一合同来源；adapter 必须隐藏 relation vector、slot 和 origin 组合，使 Native Hook、ftrace、fixed-result 等 bound roots 只依赖同一 `append bound payload` Interface。这个例外避免把排除 transport `data` 的投影伪装成完整 `ProfilerPluginData` root，也避免为单一固定 provenance 合同扩大 planner 的表达面。

Arrow 行序列化不属于关系映射规则。renderer 从同一 plan 生成强类型、借用输入值的 relation row，由仓库已有的 `serde_arrow::ArrayBuilder` 按显式 Arrow Schema 增量构建 `RecordBatch`；项目代码只保留关系键、枚举定义、逻辑字节估算和有界 Parquet spool。显式 Schema 固定 `Utf8`、`Binary`、nullable Struct 与数值物理类型，不采用 `serde_arrow` 的 Schema 推导，因此不会改变本决定的 protobuf 映射。合同测试覆盖 presence、oneof、非 UTF-8 bytes、nullable Struct 与跨 flush 后的 Schema 和逐值结果。

本选择接受更高的构建期复杂度，以及对 prost generated naming 的受控耦合。升级 prost、`serde_arrow` 或支持新 protobuf shape 时，必须重新验证 plan、binding、renderer 和 contract test 的一致性。本决定不据此声称已获得未经 release A/B 验证的性能收益。

本决定补充 ADR-0024 的直接事件表合同：descriptor-derived 直接解码表不适用其中针对跨记录、可复用规范化 facts 的多消费者门槛，该门槛本身不变。本决定部分取代 ADR-0042 对 `ProfilerPluginData` 来源字段和重复来源读数的发布限制，以及 ADR-0048 中“当前线程 CPU 闭环以外字段不发布”和“ftrace loss statistics 只用于准入、不发布表”的限制；这些内容将来可以出现在另名的 descriptor-derived tables 中，但不进入既有 `sched_switch`，并继续参与原有严格准入。ADR-0042 的时钟语义与换算失败合同、ADR-0048 的 `sched_switch` 与 Workflow 合同，以及 ADR-0020、ADR-0025、ADR-0049、ADR-0051、ADR-0058 和 ADR-0059 的其余决定继续有效。
