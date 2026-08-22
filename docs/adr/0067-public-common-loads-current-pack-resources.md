---
status: superseded by ADR-0069
---

# 公共 common 加载当前 PACK 的标准资源

> 历史决定，请勿实现；公共 common 应解析独立 Source Knowledge Package，当前设计见 ADR-0069。

KAT 的公共 common 统一解析和校验当前 PACK 中的 Data Source Manifest 与 Query Asset，使 Workflow 只使用稳定的 source/query 名称和参数，而不读取 PACK 物理路径或重复实现目录扫描、TOML 校验和路径约束。`knowledge.md` 和按需 schema 文档仍由入口 Skill 直接读取，不经 Workflow Host API 返回。公共 common 只拥有业务无关的运行时资源加载机制；清单、数据库知识和 SQL 仍由 PACK owner 拥有并随 PACK 版本化，不因被公共加载器读取而迁入 KAT Platform。
