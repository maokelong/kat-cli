# KAT Skills 集合与分析事后复核

任务：[Issue #278](https://github.com/maokelong/kat-cli/issues/278)。决策：[ADR-0082](../adr/0082-kat-review-rechecks-existing-evidence.md)。

交付 `kat` 总路由与 `kat-analyze`、`kat-author`、`kat-review` 三个并列任务入口，成套安装和同版本发布升级；共享运行环境继续放在 `kat/`。发布单一压缩包，用户将四目录成套安装到目标 Agent 的 skills 目录；首版不提供安装器。

## 问题与最小交付

KAT 应交付产品 Skill 集合。保留 `kat` 总路由，将原有问题分析、PACK 创作与维护拆为独立入口，新增并列的分析复核入口 `kat-review`；四个 Skill 随同一版本成套发布、安装和升级。复核不再作为只在仓库 checkout 中发现或需要额外复制的开发辅助 Skill。

| 入口 | 职责 |
|---|---|
| [`kat`](../../kat/skills/kat/SKILL.md) | 根据用户任务选择对应 Skill，并承载集合共用的命令合同和运行环境 |
| [`kat-analyze`](../../kat/skills/kat-analyze/SKILL.md) | 发现与执行已有 Workflow，查询 Run Output，形成问题结论 |
| [`kat-author`](../../kat/skills/kat-author/SKILL.md) | 理解、创建、修改、诊断和验证 PACK、Provider、Workflow，拥有 scaffold 脚本与作者示例 |
| [`kat-review`](../../kat/skills/kat-review/SKILL.md) | 总结原问题，检查原结论与现存证据，按需只读重查并交付复核报告 |

现有分析和作者流程已经分在不同 reference 中，可按任务归属拆分；原结果合同中的分析和创作要求也分别归回对应入口。平台选择、部署定位、Data Home、Response 和精确命令规则统一放在 `kat/references/command-reference.md`，总路由不重复保存三个任务流程。拆分保留现有任务授权边界：没有匹配 Workflow 时说明能力缺口，不能自动转入 PACK 创作；分析完成也不自动触发复核。

`kat-review` 接收原问题、原报告以及能明确关联的 Session/Run、查询结果和日志；材料也可来自当前对话，复核不依赖原始聊天记录。它确认复核范围、检查证据、必要时重新查询 Run Output，最后形成逐条结论判断和证据缺口说明。没有原报告时只能做证据概览，不能声称验证了原结论；只有 Run ID 时补问 Session ID，不跨 Session 扫描猜测。

源码以 `kat/skills/` 收纳四个并列产品 Skill，作者脚本和示例随 `kat-author` 迁移。装配器收纳完整集合，向 `kat/` 加入 Bundled PACK 和双平台载荷，继续在 staging 内完成后发布到调用方独占的全新输出路径。

## 共享运行环境

部署中的四个 Skill 保持同级；`kat/` 保留现有载荷内部结构：

```text
<skills-directory>/
├── kat/
│   ├── SKILL.md
│   ├── references/command-reference.md
│   ├── scripts/targets/<target>/
│   │   ├── kat[.exe]
│   │   └── python/
│   └── assets/packs/
├── kat-analyze/SKILL.md
├── kat-author/SKILL.md
└── kat-review/SKILL.md
```

`<target>` 分别对应既有 Linux 与 Windows 载荷。Python 是由 CLI 启动的私有 Host，不因 Skill 拆分而变成直接调用入口。CLI 仍从自身位置查找相邻 Python，并反推含 `SKILL.md` 的 `kat/` 根来发现 Bundled PACK，因此无需改变这些 Rust 路径合同。

三个任务 Skill 从自身位置解析相邻 `kat/`，读取公共命令合同并使用选中载荷的绝对路径；公共文档中的根目录必须明确指向 `kat/SKILL.md` 的父目录。直接调用任务 Skill 不要求先触发总路由。成套搬移和升级保持四目录同级，避免对子 Skill 单独复制载荷或依赖当前工作目录。

## 归档与安装

沿用 `kat-skill-<version>.tar.gz` 及其外置 SHA-256 文件，归档顶层为 `kat/`、`kat-analyze/`、`kat-author/`、`kat-review/` 四目录。用户先校验并解压到独立目录，再将四目录一起放入目标 Agent 的 skills 目录。升级时停止使用这套部署的任务，成套替换旧目录，避免将新旧文件合并；不触碰其他 Skill 或 KAT Data Home。

同版本交付与装配 staging 不等于安装四目录时的文件系统事务。首版由用户管理替换过程，不新增安装器、版本注册表或恢复机制。

既有 Linux 支持范围、Windows 预发布候选状态及 [RC 发布规则](../release-rehearsal.md) 继续有效；集合拆分不提升平台支持等级，也不跳过最终公开资产的发布验证。

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

集合拆分从最终归档校验四个顶层目录、Skill 入口、共享载荷与引用；解压并整体搬移后验证总路由分派及任务 Skill 直接调用，PACK scaffold 和示例仍能定位，相邻 Python 与 Bundled PACK 发现仍有效。装配测试覆盖缺少入口、已有输出、输入重叠及失败清理，实际平台 smoke 验证查询链路；模拟载荷只能证明路径装配，不能替代真实运行时验收。

分析复核的行为验收：

1. 在未参与原分析的 Agent 中，仅提供 Skill、原问题、报告和明确关联的证据，可以总结问题并逐条复核结论，无需原始聊天记录。
2. 原报告称全部正常，但全表聚合存在慢帧时，复核指出反证，不用局部正常样本支持全局结论；没有主线程等待证据时，也不据此排除等待原因。
3. 缺少原报告时输出证据概览；只有 Run ID 时补问 Session ID，既不猜测身份，也不扫描其他 Session。
4. 部署不可用、原查询文件丢失或历史 Guide 未保存时，复核区分仍可验证的结论和无法验证的部分，不声称恢复完整历史或执行了重查。
5. 具备完整 KAT deployment 和保留的 Session 时，按该部署的命令合同检查既有 Run 并只读重查 Output，报告记录实际 SQL、结果及其与原证据的区别；不产生新 Run 或改写原分析。

验证包括 Skill frontmatter、命名与文档链接检查，以及独立 Agent 的材料复核和缺项演练。真实 KAT 重查链路以完整部署上的实际执行为证；离线演练不能代替。具体执行结果记录在 PR 中。
