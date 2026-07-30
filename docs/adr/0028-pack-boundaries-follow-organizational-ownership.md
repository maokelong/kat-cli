---
status: accepted
---

# PACK 边界服从组织所有权

PACK 边界由稳定 owner 的维护与发布责任决定，不由 Workflow 数量、代码规模或技术分层决定；同一 owner 只有在责任与发布门确实独立时才拆成多个 PACK，以较大的团队内模块换取明确看护者和自包含交付。

同一 Skill 原子发布不代表各 PACK 发布门相同；`kat-openharmony-demo` 与同属 Kernel Team 的 `kat-kernel` 分离，是因为前者独立承担演示可信度、Deprecated Trace Streamer 依赖和正式发布前的退场责任。
