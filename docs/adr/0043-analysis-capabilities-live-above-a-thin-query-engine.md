---
status: accepted
---

# 分析能力优先建立在薄 Query Engine 之上

Workflow Runtime 中的 DataFusion engine 只拥有执行、注册、授权、资源和跨进程数据面等机制；KAT 分析能力优先作为 `kat.trace`、Runtime 私有库或 UDF 建在其上，通过所需的窄接口供 PACK 组合，且不向 PACK 暴露底层 SessionContext 或为不同入口维护平行实现。实现优先复用随 Host 固定发布的成熟向量化库并在 Arrow batch 上工作，不为假设的性能问题预建原生扩展或双实现。只有代表性真实负载证明这里是关键瓶颈时，才在保持公共类型与失败语义不变的前提下把实现私有地下沉到 Rust/DataFusion 层。
