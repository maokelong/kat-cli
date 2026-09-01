---
status: superseded by ADR-0075
---

# Datasource types 是封闭的 KAT 能力

Datasource types 是编译进 KAT CLI、随 Platform Payload 发布的封闭集合。预发布阶段只包含作为长期主线的 Hitrace 与过渡性的 Trace Streamer，不包含 Langfuse。Trace Streamer 不与 Hitrace 共享数据模型，也不增加来源识别、版本白名单、Schema 兼容层或稳定表界面；它通过 Import 内部的只读 SQLite 查询能力枚举 `main` schema 的全部非系统实体表、跳过 view，并物化为同名 Parquet table，而不是声明固定表白名单。该实现不把 SQLite 注册为独立 Datasource，也不把第二套查询引擎暴露给 Workflow。它从首次交付即在 CLI help 与 KAT Skill 中标为 `Deprecated`，并必须在第一次正式发布前连同 SQLite 读取依赖一起删除；届时依赖它的预发布 PACK 必须移植到已经成熟的长期 facts、重写场景或一同删除，不提供兼容入口。所有其他 Deprecated Datasource 与命令也服从同一发布门。这是当前明确接受的阶段性交付例外，不成为以后把验证性能力并入主干的先例，也不为此增加 experimental namespace、feature flag、兼容期、迁移协议或自动发布一致性系统。每种类型拥有静态名称、强类型参数和帮助信息；External PACK 只扩展 Dataset 之上的 Workflow 与领域策略，不能注册 Datasource、增加 `kat import` 变体或注入 CLI 参数。

这一边界使 Data Import 能在 CLI 解析阶段提供真实约束，但私有格式不能仅靠部署 External PACK 接入；它必须先转换为受支持输入，或修改并重新发布 KAT。KAT 不同时承诺静态强类型导入界面和动态 Datasource 插件系统。
