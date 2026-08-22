---
status: accepted
---

# 公共 PACK 能力先在所属 PACK 内孵化

可能被多个 PACK 复用的分析策略先作为所属 PACK 的私有能力随该 PACK 验证；只有语义稳定、经过真实数据回归且已有多个真实 PACK 消费者后，才晋升为平台公共能力。由 KAT 统一打包第三方依赖、且必须先建立一次 Host 交付边界才能让 External PACK 消费的基础设施适配能力，可以由明确的公共 Interface、一个真实 tracer 和 Bundled Host 验证直接进入 common；ADR-0081/0082 的 PostgreSQL 执行能力是首个这种基础设施切片，不把示例 PACK 的业务策略提升为 common。

晋升后的公共 common 是 KAT Platform 与私有 Workflow Host wheel 的组成部分，随 KAT Skill 原子发布并向 Bundled PACK 与 External PACK 平等开放；它不形成独立 PACK、独立 wheel、独立版本、在线安装入口或依赖求解，第三方依赖由同一 KAT Host 锁定和交付。KAT 不建立 common PACK 或运行时跨 PACK import；这保留 PACK 的自包含发布边界，同时让经过证据验证的复用能力不必在每个 PACK 中永久复制。本决定把 ADR-0049 已用于 Trace 能力的成熟度路径推广到其他分析能力，但不预先规定公共模块的最终源码目录或 Python namespace。
