---
status: accepted
---

# 一次 Data Import 只使用一个 Datasource

一次 `kat import` 显式选择一个内置 Datasource，将其强类型输入整体生成或覆盖为一个 Dataset；KAT 不自动探测、拆分或合并多个来源，也不把 Dataset 永久绑定到来源类型，Workflow 只依赖表。

预发布仅支持 Hitrace 和不晚于首次正式发布删除的 `Deprecated` Trace Streamer，不包含 Langfuse；后者只为联调而物化 SQLite 的全部非系统关系，不形成稳定 Schema 或通用 SQLite Datasource。

单源限制暂不支持异构 Dataset，但避免引入表名冲突、多方失败回滚和 Dataset extension 语义。
