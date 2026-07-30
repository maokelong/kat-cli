---
status: accepted
---

# Run Outputs 通过有界查询交付给模型

Workflow 产生的 Run Outputs 是保存在本地 Parquet 中的完整结构化数据，不是可直接塞入模型上下文的 Analysis Result；Run Manifest 与成功 KAT Response 只公开定位和理解 Output 所需的摘要，不复制完整数据、预览或 Schema，Run 发布与可移植 Output name 以 ADR-0055 为准。模型通过只读、无持久副作用的 Output Query 按需取得证据；按 ADR-0056，Runtime 独占 Arrow 到公开 Query Result 的投影，CLI 只附加自己拥有的当前 Dataset 状态，用户 SQL 可以读取本机来源。这里的“有界”由调用方通过投影、过滤、聚合或 `LIMIT` 自行实现，KAT 不再设置来源 allowlist、固定输出限制、deadline、分页或静默截断，资源消耗由调用方负责。
