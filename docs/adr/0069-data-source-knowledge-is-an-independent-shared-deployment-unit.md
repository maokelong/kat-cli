---
status: superseded by ADR-0079
---

# Data Source Knowledge 是独立共享部署单元

一份逻辑数据源说明、`knowledge.md`、按需 schema 文档和 Query Asset 共同组成独立部署的 Source Knowledge Package；它不再位于某个 PACK 内，可以由多个 Bundled PACK 或 External PACK 通过 KAT 公共 common 共同使用。PACK 继续拥有 Workflow 与 PACK 私有分析策略，并且仍不能依赖或导入其他 PACK；共享数据源知识是与 PACK、KAT Platform 和具体数据库连接实例并列的发布边界。

Source Knowledge Package 继续使用不执行 Python 的静态 Data Source Manifest，并保留 ADR-0065 已定义的 Query Asset；入口 Skill 按需读取选定 package 的知识文档，Workflow 通过公共 common 按稳定 source/query 身份加载运行时资源。`source.toml` 恰好保留非空根级字符串 `name`、`title`、`description` 和 `dialect`，不声明 owner；维护责任由 KAT 之外的仓库和交付流程承担，不成为身份、认证或权限依据。一次 discovery 中的全部 source name 共用扁平全局作用域，任意不同目录提供同名清单时整体失败，不建立来源覆盖顺序、`owner/name` namespace 或持久 registry。版本关系仍需另行确认，不在本决定中预设。

本决定取代 ADR-0064 的 PACK-local 身份与目录模型以及 ADR-0067 的“从当前 PACK 加载资源”模型，并修订 ADR-0027 中 PACK 必须随自身复制全部数据源知识才能运行的局部含义；不改变禁止跨 PACK Python import、common PACK 或 PACK 代码依赖的决定。
