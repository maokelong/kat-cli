---
status: accepted
---

# 分离分析审计与 PACK PR 检视入口

`kat-review` 的实际职责是复核已完成的 KAT 分析，却容易被理解为代码或 PR review。将它直接改名为 `kat-analysis-audit`，不保留旧名兼容入口；其已有只读证据复核合同保持不变。新增 `kat-pack-review`，用于对外部 PACK PR 做只读、非门禁的技术检视。仓库内 `.agents/skills/kat-pr-review` 继续只审查 kat-cli 开发 PR，不随产品 Skill 集合交付。

`kat-pack-review` 检查完整 PR，但只报告本次新增、扩大或依赖的 PACK 交付问题。目标 PR 不必关联 Issue、SDD 或验收标准；首版不限制 KAT 版本，也不在缺少领域规格、权威参考或 golden data 时判断算法正确性。默认只做静态检视；只有用户明确信任代码或提供可丢弃隔离环境时才执行 PACK。报告使用固定七段检查表记录“已核查 / 不适用 / 无法验证”，findings 按 High/Medium/Low 给出证据、影响、最小修复和定位，不输出评分、批准或合入判断。

`kat` 总入口同时路由 `kat-analysis-audit` 与 `kat-pack-review`，两者也都可直接调用。产品集合由五个任务 Skill 加总入口组成六个同级目录，继续共用 `kat` 的命令合同和载荷并作为一个版本单元发布。该决定局部替代 [ADR-0082](0082-kat-review-rechecks-existing-evidence.md) 中的公开名称和入口数量，保留其分析复核证据边界；同时更新 [ADR-0002](0002-skill-and-runtime-ship-atomically.md) 与 [ADR-0011](0011-skill-directly-selects-the-platform-payload.md) 所描述集合的当前成员，但不改变相邻部署和同版本发布原则。详细范围及验收见[设计规格](../specs/kat-pack-review.md)与 [Issue #308](https://github.com/maokelong/kat-cli/issues/308)。
