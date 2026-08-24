---
status: superseded by ADR-0062
---

# 一次 Data Import 只使用一个 Datasource

一次 Data Import 将所选 Datasource 的强类型输入交给它，并由它一次生成或整体覆盖 Dataset。KAT 不在一次导入中拆分输入、调度多个 Datasource 或合并它们的表。这个约束只作用于单次导入；后续整体覆盖可以选择另一种 Datasource，Dataset 不持久绑定来源类型。预发布输入范围只有 Hitrace 与从首次交付即标为 `Deprecated`、必须在第一次正式发布前删除的 Trace Streamer，不包含 Langfuse。

KAT CLI 以 `kat import <datasource-type> <typed arguments>` 显式选择 Datasource。Hitrace 使用 `--trace <path>`，Trace Streamer 使用 `--database <path>`；SQLite 只是后者的输入存储机制，不是独立 Datasource。每种成功 Data Import 都在 operation-specific `result.path` 返回最终 Dataset 的 canonical 绝对 Unicode 路径；Hitrace 在同一 result 中另加自身必需的两个 `unsupported_*` arrays，Trace Streamer 不增加其他字段。Import 不复制 tables 或 Schema，Skill 使用返回的 path 调用既有 Dataset inspection。每种 Datasource type 随 KAT 二进制静态声明自己的单文件输入和参数关系；第一版不自动探测输入类型，也不使用通用 `--source`、`--datasource <value>`、`role=path`、动态参数或 manifest。

过渡性的 Trace Streamer Datasource 不以固定 relation 白名单裁剪源库：它只读枚举 `main` schema 的全部非系统实体表、跳过 view，并将每个符合范围的 relation 物化为同名 Dataset table。SQLite view 没有该入口可依赖的稳定来源声明类型，因此不属于 Dataset relation 范围。该源驱动 Schema 是 Deprecated 入口的明确例外，不形成稳定表合同，也不把通用 SQLite 导入提升为新的 Datasource type。

这一限制暂不支持多个 Datasource 共同组成的异构 Dataset，但避免重新引入 source 分组、表名冲突、多方失败回滚和 Dataset extension 语义。Workflow 仍只依赖 Dataset 表，不把 Datasource 身份写入 Required tables。
