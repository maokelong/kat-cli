---
status: accepted
---

# Workflow 构建单元使用并列 common 源码目录

`kat/platform/workflow` 的源码职责目录调整为并列的 `api/`、`common/` 和 `runtime/`：`api/` 保存最小 Pack Authoring API 声明与公共领域类型，`common/` 保存向 Bundled PACK 与 External PACK 平等开放的公共可执行能力，`runtime/` 保存 KAT 私有 Workflow Host 实现。三者仍由同一个 PEP 517 构建单元生成一个私有 wheel，并随 KAT Skill 原子发布；源码目录、Python import namespace 与 wheel 内布局不要求同构。本文只修订 ADR-0012 和 ADR-0045 中 Workflow 源码职责由 `api/`、`runtime/` 两个目录组成的局部描述，其余源码/部署分离及单 wheel 决定继续有效。
