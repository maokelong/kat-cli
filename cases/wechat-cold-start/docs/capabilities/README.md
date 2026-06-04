# Harmony Cold Start Capabilities

本目录沉淀鸿蒙 App 冷启动分析能力，按 SmartPerfetto 的建设方式拆成 atomic 与 composite 两层。

- [harmony-cold-start-atomics.md](harmony-cold-start-atomics.md): 冷启动基础原子能力，加任意区间关键路径候选查询和通用关键路径筛选原子能力。每个 atomic 只回答一个确定性问题，并给出输入、输出和核心 SQL。
- [harmony-cold-start-composites.md](harmony-cold-start-composites.md): 若干组合能力。Composite 只负责编排 atomic、做质量门禁和输出结构化结论。

这些文件当前是设计规格，面向 `kat-rs` 的 DataFusion/Web UI 查询模型编写。后续如果要迁移到 SmartPerfetto，可按其中的 `name/type/category/tier/inputs/steps/sql` 转成 `backend/skills/atomic/*.skill.yaml` 与 `backend/skills/composite/*.skill.yaml`。
