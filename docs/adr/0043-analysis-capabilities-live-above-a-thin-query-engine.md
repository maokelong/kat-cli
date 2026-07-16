---
status: accepted
---

# 分析能力优先建立在薄 Query Engine 之上

Workflow Runtime 中的 DataFusion engine 只集中拥有 SessionContext、表与 UDF 注册、执行生命周期、Table Grant、资源限制和跨进程数据面等机制。KAT 自有分析能力优先作为公开 KAT Trace Library、Runtime 私有库或注册 UDF 建在引擎之上，再通过 `kat.trace`、SQL、DataFrame Expr 或必要的窄 Pack Authoring API 供 PACK 使用；同一能力不为不同入口维护平行实现。PACK 可以感知和组合公开的函数与类型，但不能获得底层 SessionContext 或任意修改注册表。

实现优先复用随 Bundled Python Host 固定发布的成熟向量化库。Python 实现必须在 Arrow batch 上调用 PyArrow 等原生 kernels，不能把大列转换为 Python 对象逐行循环。KAT 不为尚未证实的性能问题预建 PyO3 module、FFI capsule、native wheel、Rust port trait 或双实现；这会扩大 Linux 与 Windows 的构建、测试和发布闭包，却不增加当前产品能力。

只有代表性真实负载证明引擎之上的实现成为关键瓶颈时，能力才保持 SQL、Expr、类型与失败语义不变地下沉到 Rust/DataFusion engine layer。性能下沉是私有 Implementation 替换，不是新增一套用户 Interface；是否下沉以实际证据决定，不提前设置脱离负载的阈值或扩展点。

首个应用是 `kat_convert_clock`：第一版使用 Runtime 私有的 `stable` Python/PyArrow scalar UDF，对整批 Arrow arrays 使用 checked compute kernels；SQL 与 `ctx.convert_clock(...)` 调用同一个 UDF object。首版所有准入时钟均为每秒十亿 tick，因此只实现同频 checked 平移，不实现异频缩放、舍入、PyO3 或 Rust 原生扩展。以后若真实性能或新的已准入时钟要求更底层实现，再按上述规则替换。
