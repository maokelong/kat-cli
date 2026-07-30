---
status: accepted
---

# Workflow 声明可强制的表依赖

每个 Workflow 都必须在入口旁以 `required_tables` 声明完整、精确的 PACK-visible Dataset 表依赖，无依赖也显式声明空列表；它是需要 review 的 PACK 源码事实，authoring flow 可以辅助生成，但运行观察或 helper 自身都不能替代该声明。Inspect 与自动选择消费同一声明，Runtime 在选定 Workflow 后核对 Dataset 并只把已声明表作为不可变 Source tables 授予本次 Execution Lease；缺表、未声明访问或改写都会在受支持执行面失败，使 Required tables 成为正确性约束而非可漂移文档或安全沙箱。第一版只接受与 Datasource 表名一致的可移植精确名称，不增加 Schema、optional/alternative 表或调用图 DSL；平台时钟换算可以私下读取 `clock_domain` 证据，但 PACK 直接读取它仍必须声明。
