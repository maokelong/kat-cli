---
status: accepted
---

# PACK 边界服从组织所有权

每个 PACK 对应一个稳定的所有者组织或团队及其维护、发布责任，并通过 `pack.toml` 的 owner 展示该责任；同一 owner 可以拥有多个 PACK，但只有彼此具备独立维护与发布责任时才拆分，不因 Workflow 数量、代码规模或问题数量而拆 PACK。技术领域可以在组织职责恰好与其一致时形成 PACK，但决定边界的是 ownership 而不是技术分层；这以较大的团队内模块换取明确看护者和自包含交付，并避免产生无人负责的 `common` 或 `utils` PACK。

同一 Skill 中原子发布不等于其中每个 PACK 具有相同发布门。预发布 `kat-openharmony-critical-path`、`kat-openharmony-thread-cpu-time` 与同属 Kernel Team 的正式 `kat-kernel` 分离，是因为前两者分别承担各自分析职责的可信度门，并共同承担 Deprecated Trace Streamer 依赖闭包以及正式发布前必须决定迁移、晋升或删除的退场责任；这些是实际维护与发布责任，不是按单个 Workflow 或成熟度随意拆分。
