---
status: superseded by ADR-0079
---

# Source Knowledge 作为 wheel 外 resources 交付

Source Knowledge Package 是平台无关资源，不进入私有 Workflow Host wheel：Bundled 源码位于 `kat/resources/<source-package>/`，Skill 部署视图位于 `assets/resources/<source-package>/`，用户本地部署位于 `<KAT_DATA_HOME>/resources/<source-package>/`，开发或临时使用通过可重复的 `--resource-dir <directory>` 精确加入一个直接包含 `source.toml` 的 package。两个默认 `resources/` 根只扫描直接子目录中的 `source.toml`，不递归搜索任意仓库，也不建立持久 registry；Bundled 与 External package 使用同一清单、名称冲突和发现规则。

`resources` 只命名共享资源的物理发现根，领域对象仍是 Source Knowledge Package，清单仍是 `source.toml`。把这些 Markdown、Schema 和 SQL 留在 wheel 外，避免在各平台 Payload 中重复、让入口 Skill 可以直接按需读取，并允许 External Source Knowledge Package 在不重建 KAT Host 的情况下独立交付。
