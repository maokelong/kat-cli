---
status: accepted
---

# 平台维护权不赋予 PACK 特权

内核团队可以同时担任 KAT 平台基础设施维护者和内核 PACK 所有者，但这两个角色在架构上严格分离：KAT 平台只提供所有 PACK 共用的执行、authoring 与 KAT Trace Library，内核 PACK 与其他 Bundled PACK、External PACK 遵守相同约束，不拥有私有 Runtime 入口、额外数据权限或特殊加载路径。内核团队建设的公共 Trace 分析能力必须通过 `kat.trace` 或其他已正式定义的公共 Interface 平等发布，不能让其他 PACK 直接 import `kat-kernel` 或 Runtime 私有源码，也不能借平台维护身份建立捷径。
