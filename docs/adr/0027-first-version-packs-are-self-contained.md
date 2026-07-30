---
status: accepted
---

# 第一版 PACK 完全自包含

第一版每个 PACK 都是可独立 inspect、test、run 的发布单位，不声明 PACK dependency 或 Exported Capability，Runtime 一次只加载选中的 PACK，也不为跨 PACK import 建立特殊路径或拦截器。

共享 source facts 属于 Datasource 与 Dataset，领域内复用留在 PACK 私有 helper，通用执行能力属于 Pack Authoring API；真实跨领域需求出现前宁可少量重复，也不创建 `common`、`utils` PACK 或把领域逻辑移入平台。

这一决定以更少机制换取自包含边界，并取代 ADR-0006 及其他 ADR 中第一版依赖与导出能力的局部设计；未来若证明确需跨 PACK 复用再增加相应机制，现有无依赖 Interface 不变。
