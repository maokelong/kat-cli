---
status: accepted
---

# 公共 PACK 能力先在所属 PACK 内孵化

可能被多个 PACK 复用的能力先作为所属 PACK 的私有能力随该 PACK 验证；只有语义稳定、经过真实数据回归且已有多个真实 PACK 消费者后，才晋升为随 KAT Skill 和 Workflow Host 原子发布、向 Bundled PACK 与 External PACK 平等开放的平台公共能力。晋升后的公共 common 是 KAT Platform 与私有 Workflow Host wheel 的组成部分，不形成独立 PACK、独立 wheel、独立版本、在线安装入口或依赖求解；其第三方依赖由同一 KAT Host 锁定和交付。KAT 不建立 common PACK 或运行时跨 PACK import；这保留 PACK 的自包含发布边界，同时让经过证据验证的复用能力不必在每个 PACK 中永久复制。本决定把 ADR-0049 已用于 Trace 能力的成熟度路径推广到其他分析能力，但不预先规定公共模块的最终源码目录或 Python namespace。
