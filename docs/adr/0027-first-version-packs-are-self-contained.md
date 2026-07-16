---
status: accepted
---

# 第一版 PACK 完全自包含

第一版把每个 PACK 定义为可独立 `kat inspect --pack`、`kat test` 和 `kat run` 的发布单位：`pack.toml` 不声明 PACK dependencies，不提供 Exported Capability，一次 Runtime 操作只加载精确选中的目标 PACK。封闭的 manifest schema 对 `dependencies` 明确报错；`capabilities/` 等未成为现行 Interface 的目录不扫描也不因存在而失败。其他 PACK 不进入 KAT 建立的 module search path，实际跨 PACK import 只会按 Python 普通规则自然失败，KAT 不扫描源码、识别调用或增加 import 拦截器。可被多个分析复用的 source facts 属于 Datasource 和 Dataset，同一领域内的代码复用留在 PACK 私有 helper，通用执行能力属于 Pack Authoring API。真实跨领域复用出现前，允许少量领域代码重复，但不创建 `common`、`utils` PACK，也不把领域逻辑转移到平台层。

这个决定以较少的当前机制换取更强的 PACK 自包含边界，并取代 ADR-0006 以及其他 ADR 中关于第一版 PACK dependency 和 Exported Capability 的局部设计。将来若出现具体且无法由 Dataset、helper 或 Pack Authoring API 承担的跨 PACK 复用，再增加 manifest dependencies、能力入口和入口级依赖注入；现有无依赖 Workflow、Dataset、Run Output 与 Skill Interface 不因此改变。
