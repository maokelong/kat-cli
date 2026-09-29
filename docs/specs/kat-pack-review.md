# KAT PACK PR 检视 Skill

状态：已实现并验证。
关联：[Issue #308](https://github.com/maokelong/kat-cli/issues/308)。

## 问题与目标

KAT Skills 现有 `kat-review` 实际复核已经完成的分析报告，名称无法清楚区分“分析复核”和“PACK 代码审查”。同时，外部开发者提交 PACK PR 时缺少一套随 KAT Skills 交付、可重复执行的技术检视流程。

本切片把分析复核入口直接改名为 `kat-analysis-audit`，并新增 `kat-pack-review`。两者都可以直接调用，也由 `kat` 总入口按用户目标路由；旧名不保留兼容别名。六个 Skill 继续作为一个集合装配、验证和发布。

## 范围边界

`kat-pack-review` 只读检视一个完整 PR 中交付的 PACK。它聚焦本 PR 新增、扩大或实际依赖的 PACK 缺陷，不把纯历史问题算作本次 finding。它不评价业务价值，不要求目标 PR 关联 Issue、SDD 或明确验收标准，也不输出分数、通过/失败、合入建议或门禁结论。

首版不限制目标 PACK 声明的 KAT 版本。算法或领域结论只有在 PR 提供领域规格、权威参考或 golden data 时才判断正确性；缺少判据时记录为无法验证，不把个人经验变成 finding。

默认不执行待审 PACK 代码。只有用户明确表示代码可信，或提供可丢弃的隔离环境时，才允许按同版本公共命令合同执行检查或测试。无论是否执行，都不得创建 GitHub review、评论、状态或其他外部写入。

已有 reviewer 结论不是证据。检视者先独立完成核查，再把作者提供的可验证事实和当前提交 CI 结果作为证据。可以用多个 Agent 并行提高效率，但不是流程依赖；主检视者必须重新核对所有最终 findings。

## 固定检查表

检查表随 `kat-pack-review` 交付，按以下七个章节组织：

1. PR 范围与基线；
2. PACK 边界与静态结构；
3. Workflow 合同；
4. Datasource / Provider；
5. Workflow / Provider 知识；
6. 测试与可验证证据；
7. 宿主风险与实现缺陷。

每一项只能标记为“已核查”“不适用”或“无法验证”。这些状态描述覆盖度，不代表通过/失败。finding 必须同时具备可复现或可定位的触发条件、实现与被违反合同或行为之间的联系，以及对使用者或运行结果的实际影响；风格偏好、无依据猜测和无影响的琐碎问题不进入 findings。

## 检视流程与输出

1. 确定 PR 基线、完整 diff、PACK 根目录和本次边界；缺少完整材料时说明限制。
2. 在不读取旧 reviewer 判断的前提下先完成独立静态核查，并逐项记录检查表状态和证据。
3. 只有满足信任或隔离条件时才执行检查、测试或最小复现；执行时读取相邻 `kat` 的公共命令合同。
4. 对候选问题重新核对 PR 归因、证据链和实际影响，删除不满足 finding 门槛的内容。
5. 交付范围与限制、按 High/Medium/Low 排列的 findings、覆盖表和整体风险。每个 finding 包含证据、影响、最小修复建议及文件/行锚点。没有 finding 只表示本次未确认具体缺陷，不等于批准合入。

## 实现切片

- 将 `kat/skills/kat-review/` 改为 `kat/skills/kat-analysis-audit/`，同步名称、元数据、内部引用和路由。
- 新增 `kat/skills/kat-pack-review/`，`SKILL.md` 只承载边界、流程和报告格式，详细固定检查表放在 `references/checklist.md`。
- 更新装配器、集合校验、发布 workflow、README、离线手册、领域词汇和相关 ADR，使六个目录作为唯一合法集合。
- 不修改 `.agents/skills/kat-pr-review`；它继续只服务本仓库 kat-cli 开发过程，不进入发行包。

## 验证

- 结构：六目录可装配、重定位和校验，路由同时发现新入口，归档中不存在 `kat-review`。
- 元数据：两个新名称通过 Skill 结构校验，`agents/openai.yaml` 与入口一致。
- 行为：分别用“存在真实缺陷”“没有确认缺陷”“领域正确性无法验证”三个离线场景独立调用，核对 findings 门槛、覆盖状态与非门禁措辞。
- 回放：选择一个历史 PACK PR，在不读取旧 review 结论的前提下检视，再与已知结果比较并记录差异。

## 实施证据

- 装配、集合校验和发布 workflow 的 38 项定向测试通过，其中 5 项平台符号链接场景按当前 Windows 条件跳过；旧 `kat-review` 不再是合法集合成员。
- 六个 Skill 均通过 `skill-creator` 的结构校验，本次 diff 通过 whitespace 检查。
- 三个独立离线场景分别验证了真实合同缺陷、无确认缺陷和领域正确性无法验证；修订后，测试、规格或 oracle 缺失只进入覆盖状态与整体风险，不再单独产生 finding。
- 对真实 PR [#246](https://github.com/maokelong/kat-cli/pull/246) 完成盲检回放，未执行代码、未读取 reviewer 结论，未确认 PACK 交付缺陷。盲检后核对到该 PR 没有 GitHub review、issue comment 或 inline review comment，因此不存在可逐条比较的历史 reviewer 结果；当前 checks 缺少 Full CI 仍作为无法验证范围保留。
- Standards 与 Spec 两路独立 review 的已确认问题均已修复，复审无剩余 findings。全量 `build/tests` 在本机发现 89 项后因既有 `test_sdk_build` 缺少 `griffe` 导入依赖中止；该环境限制不影响上述定向测试，但全量套件未在本机完整跑通。
