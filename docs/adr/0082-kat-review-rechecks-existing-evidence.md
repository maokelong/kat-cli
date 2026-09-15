---
status: accepted
---

# KAT Skills 成套发布，分析、创作与复核分别承接任务

用户在问题分析完成后，需要按需总结原问题并独立复核结论。KAT 已发布的 Run Output、已有查询结果和可用日志可以支持这项工作；首版提供独立 `kat-review` Skill，接收原报告与明确关联的 Session/Run，必要时通过现有 Output Query 补充证据。

复核区分证据支持、证据不足、与证据矛盾和无法验证。KAT 现有材料不保证覆盖全部历史调用或当时的 Guide，因此缺失材料时报告其对结论的限制，不补造历史过程，也不改变原分析的完成状态。当前 Guide 可以指导复核，但不能冒充历史快照。复核产生的新查询与说明同原分析证据区分，不改写原报告与 Run。

本决策局部替代 [ADR-0013](0013-one-public-kat-skill-hides-operational-verbs.md) 关于唯一 Skill 入口的约束：保留 `kat` 总路由，以 `kat-analyze`、`kat-author`、`kat-review` 分别承接问题分析、PACK 创作与维护、分析复核。四个 Skill 的源码位于 `kat/skills/<name>/`，成套安装并随同一 KAT 版本发布和升级；用户可以从总入口路由，也可以直接选择任务 Skill。没有匹配 Workflow 时不自动转入创作，分析完成也不自动触发复核。

[ADR-0002](0002-skill-and-runtime-ship-atomically.md) 的原子发布单元相应调整为 KAT Skills 集合，Skill 指令、CLI 与 Host 保持同版本交付。共享命令合同、`scripts/targets/` 下的 CLI 与 Bundled Python Host、`assets/packs/` 中的 Bundled PACK 继续放在 `kat` Skill 目录中。三个任务 Skill 从自身位置解析同级 `kat/`，读取公共命令合同并直接使用选中载荷的绝对路径，无需先执行总路由。Python 与 CLI 的相邻布局及 [ADR-0011](0011-skill-directly-selects-the-platform-payload.md) 的根定位合同保持不变，不需要修改 Rust 路径逻辑。

发布沿用单个 `kat-skill-<version>.tar.gz` 及其外置 SHA-256 文件，归档顶层为四个 Skill 目录。用户校验后解压到独立目录，再将四目录一起安装到目标 Agent 的 skills 目录；升级前停止相关任务，成套替换旧目录，不合并新旧文件，不改动其他 Skill 或 KAT Data Home。首版不提供安装器，不承诺安装四目录的文件系统原子事务。既有候选平台和 RC 发布规则继续有效。

`kat-review` 重查前遵循对应部署的命令合同和 Data Home，缺少可用部署时仍可复核已提供的材料，并明确没有重新查询输出。

首版不新增 Analysis Record、完整历史采集、记录执行入口、CLI 操作、Guide 历史快照、交付完成门禁或跨机器复现能力，也不在每次原分析交付前自动调用第二个 AI。需求与验收见 [Issue #278](https://github.com/maokelong/kat-cli/issues/278)。
