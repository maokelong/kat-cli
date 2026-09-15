---
status: accepted
---

# 独立 kat-review Skill 基于现存证据事后复核

用户在问题分析完成后，需要按需总结原问题并独立复核结论。KAT 已发布的 Run Output、已有查询结果和可用日志可以支持这项工作；首版提供独立 `kat-review` Skill，接收原报告与明确关联的 Session/Run，必要时通过现有 Output Query 补充证据。

复核区分证据支持、证据不足、与证据矛盾和无法验证。KAT 现有材料不保证覆盖全部历史调用或当时的 Guide，因此缺失材料时报告其对结论的限制，不补造历史过程，也不改变原分析的完成状态。当前 Guide 可以指导复核，但不能冒充历史快照。复核产生的新查询与说明同原分析证据区分，不改写原报告与 Run。

本决策局部替代 [ADR-0013](0013-one-public-kat-skill-hides-operational-verbs.md) 关于唯一 Skill 入口的约束：`kat-review` 是用户明确选择的独立复核入口，原 `kat` Skill 文件和流程保持不动。首版将 Skill 源码放在仓库 `.agents/skills/kat-review/`，支持仓库级 Skills 的 Agent 可发现它，跨仓使用可将整个目录复制到个人 skills 目录；本切片不改变 KAT 发布装配。

[ADR-0002](0002-skill-and-runtime-ship-atomically.md) 的原子部署合同继续适用于 KAT Skill、CLI 与 Host。`kat-review` 不携带另一份 CLI 或运行时；重查前读取实际 KAT deployment 的命令说明，复用该部署的载荷和 Data Home。缺少可用部署时仍可复核已提供的材料，并明确没有重新查询输出。

首版不新增 Analysis Record、完整历史采集、记录执行入口、CLI 操作、Guide 历史快照、交付完成门禁或跨机器复现能力，也不在每次原分析交付前自动调用第二个 AI。需求与验收见 [事后复核规格](../specs/analysis-audit.md) 和 [Issue #278](https://github.com/maokelong/kat-cli/issues/278)。
