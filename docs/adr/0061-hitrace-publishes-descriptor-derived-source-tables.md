---
status: accepted
---

# Hitrace 发布完整的 descriptor-derived Source tables

Hitrace Datasource 把完整、可独立消费的来源面视为平台能力。对每个显式注册的 fully-qualified protobuf payload root，KAT 必须依据随当前 KAT 固定的 descriptor，生成确定的关系映射，并将其 reachable closure 的完整 decoded protobuf semantics 发布为不可变 Source tables；不得再按当前 Workflow 或 PACK 的字段需求裁剪。只为至少产生一行来源 occurrence/fact 的 root 或 relation 创建表；空或全默认值的 bound root 仍算一个 occurrence。仅当实际发布的表含需要解释的 enum field 时创建 definition table，并保存该 field 的完整 descriptor definitions，不按本次出现值裁剪。

这里的“完整”只指当前固定 descriptor 和 typed protobuf decoder 能表达的语义，包括已知字段值、显式 presence、oneof 选择、repeated 元素及其顺序；它不是 protobuf wire archive，不保留未知 wire fields、原始字段顺序、原始编码或 proto3 implicit scalar 的 wire presence。已注册 root 存在无法映射的 reachable shape 时构建失败；运行时对已绑定 payload 的解码、关系化或写入失败时，整个 Data Import 失败。合法但未注册的 plugin 或 section 继续按 ADR-0025 报告并跳过。

每个成功绑定的 profiler payload occurrence 必须发布一条独立 envelope occurrence provenance，并使 payload root 可连接到它实际来自的 `ProfilerPluginData` occurrence；即使 decoded root 为空或全是默认值，两行 occurrence 也必须保留。envelope 的 `data` 是被 typed root 解码替代的传输 bytes，不在 provenance relation 中重复保存；payload 内部的普通 `bytes` field 仍必须完整保留。`clock_id`、`tv_sec`、`tv_nsec` 等来源字段可以作为原始来源值出现，但不因此成为 payload event time、`UnifiedClock` 或可跨 clock domain 比较的 KAT 时间。规范化时间事实仍只使用 ADR-0042 的 `clock_domain + clock_value`。

Descriptor-derived Source tables 与 `sched_switch` 等规范化 Trace facts 是两种不同契约：前者机械保留单条 protobuf 来源语义，后者只在来源解释、跨记录规范化或复用价值已有证据时由 Datasource 另行发布。两者可以来自同一次 decode 并共存，但必须使用不同表名；机械表不取得分析语义，规范化表也不成为第二份完整 protobuf 镜像。跨记录的分析策略继续属于 `kat.trace` 或 PACK。

关系键只用于恢复当前 Dataset 内的来源结构，不是 protobuf identity、业务 identity 或跨 Import 稳定键。Dataset 不保存 descriptor digest、mapping revision、catalog 或 parent-table metadata。表名、列 Schema 和每张 child relation 的唯一父表由当前版本的静态映射合同规定；Parquet Schema 只承载列结构，不承载 parent-table metadata。Descriptor-derived 表不承诺在不同 KAT 版本间保持表名、Schema 或仍可由新版映射按同一合同解释，也不提供迁移；这不改变 ADR-0020 对合法 Dataset 的物理解析与 inspection 合同。

本决定补充 ADR-0024 的直接事件表合同：descriptor-derived 直接解码表不适用其中针对跨记录、可复用规范化 facts 的多消费者门槛，该门槛本身不变。本决定部分取代 ADR-0042 对 `ProfilerPluginData` 来源字段和重复来源读数的发布限制，以及 ADR-0048 中“当前线程 CPU 闭环以外字段不发布”和“ftrace loss statistics 只用于准入、不发布表”的限制；这些内容将来可以出现在另名的 descriptor-derived tables 中，但不进入既有 `sched_switch`，并继续参与原有严格准入。ADR-0042 的时钟语义与换算失败合同、ADR-0048 的 `sched_switch` 与 Workflow 合同，以及 ADR-0020、ADR-0025、ADR-0049、ADR-0051、ADR-0058 和 ADR-0059 的其余决定继续有效。
