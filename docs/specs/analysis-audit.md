# 独立 kat-review Skill：事后复核

任务：[Issue #278](https://github.com/maokelong/kat-cli/issues/278)。决策：[ADR-0082](../adr/0082-kat-review-rechecks-existing-evidence.md)。交付入口：[kat-review Skill](../../.agents/skills/kat-review/SKILL.md)。

## 问题与最小交付

用户完成一次 KAT 分析后，需要按需总结原问题并独立检查原结论。首版交付独立的 `kat-review` Skill，复用现存证据及 KAT 只读查询能力，不修改原 `kat` Skill、CLI、运行时或发布装配。

Skill 接收原问题、原报告以及能明确关联的 Session/Run、查询结果和日志；材料也可来自当前对话，复核不依赖原始聊天记录。它确认复核范围、检查证据、必要时重新查询 Run Output，最后形成逐条结论判断和证据缺口说明。没有原报告时只能做证据概览，不能声称验证了原结论；只有 Run ID 时补问 Session ID，不跨 Session 扫描猜测。

Skill 源码位于 `.agents/skills/kat-review/`，支持仓库级 Skills 的 Agent 可直接发现。跨仓使用时，将整个 `kat-review` 目录复制到所用 Agent 的个人 skills 目录；例如本机 Codex 的 `$CODEX_HOME/skills/kat-review/`。安装后可用 `$kat-review` 加上原报告与 Session/Run 发起复核，完整 KAT deployment 仍需独立具备。

## 运行与证据边界

需要重查时，定位用户已安装的完整 KAT deployment，读取该版本的命令说明，复用其平台载荷、Data Home 和 KAT Response 合同。只通过既有 Session/Workflow inspection 与 Output Query 读取事实；Cargo 输出和系统 Python 不能替代完整部署。部署不可用时继续检查已有材料，并标明没有重新查询。

已有 Run、查询文件和可用日志都是复核材料，不证明历史完整。Skill 只使用用户、原报告或 KAT Response 明确引用的日志与文件，不通过全盘扫描拼装历史。当前 Guide 不是历史快照；新查询必须标为本次复核证据。材料缺失、来源不可读或记录不完整时，明确受影响的结论，不反向改变原分析的完成状态。

复核不创建新 Run、不重跑 Workflow、不修改 PACK、原报告或原 Run；只读 query 自身可以正常产生新的日志和查询文件。用户要求保存复核报告时，写入指定的新文件。首版不新增 Analysis Record、完整历史采集、交付完成门禁或跨机器复现能力，也不要求每次分析后自动复核。

## 复核报告

- 总结原问题、原结论与本次实际核查范围。
- 逐条判断为证据支持、证据不足、与证据矛盾或无法验证，说明理由并引用具体证据。
- 对补充查询给出实际 SQL、Session/Run、Output 和必要结果行；未执行 SQL 不能作为证据。
- 报告已知过程疑点、证据缺口，以及解决缺口所需的最小下一步。

缺少完整日志并不自动否定已有表格事实；局部正常样本也不能证明整个分析范围都正常。现有材料不能确认的原分析动机和历史范围不由模型补写。

## 验收场景

1. 在未参与原分析的 Agent 中，仅提供 Skill、原问题、报告和明确关联的证据，可以总结问题并逐条复核结论，无需原始聊天记录。
2. 原报告称全部正常，但全表聚合存在慢帧时，复核指出反证，不用局部正常样本支持全局结论；没有主线程等待证据时，也不据此排除等待原因。
3. 缺少原报告时输出证据概览；只有 Run ID 时补问 Session ID，既不猜测身份，也不扫描其他 Session。
4. 部署不可用、原查询文件丢失或历史 Guide 未保存时，复核区分仍可验证的结论和无法验证的部分，不声称恢复完整历史或执行了重查。
5. 具备完整 KAT deployment 和保留的 Session 时，按该部署的命令合同检查既有 Run 并只读重查 Output，报告记录实际 SQL、结果及其与原证据的区别；不产生新 Run 或改写原分析。

验证包括 Skill frontmatter、命名与文档链接检查，以及独立 Agent 的材料复核和缺项演练。真实 KAT 重查链路以完整部署上的实际执行为证；离线演练不能代替。具体执行结果记录在 PR 中。
