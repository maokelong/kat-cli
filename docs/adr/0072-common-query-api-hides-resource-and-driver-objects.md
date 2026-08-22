---
status: superseded by ADR-0079
---

# common 查询 API 隐藏资源与驱动对象

公共 common 的第一版查询 Interface 提供一步式 Query Asset 与 Ad Hoc Query 函数：Workflow 传入显式 `kat.Context`、稳定 source/query 名称、SQL 或参数，common 内部完成 Resource 解析与校验、连接、值绑定、执行、Arrow 转换和 DataFusion DataFrame 构造。PACK 不获得 Source/QueryAsset handle、ResourceCatalog、物理路径、Psycopg connection/cursor 或 common 私有 model；这让 Workflow 保持业务编排入口，也保留 common 在不改变 PACK Interface 的情况下替换内部资源和驱动实现的空间。
