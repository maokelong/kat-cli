# PR 治理设计

## 问题

kat-rs 是新项目，新团队近期 PR 暴露出同一类问题：

- 没有先澄清问题、非目标和验证路径就开始写代码。
- 为想象中的未来场景加入过度设计。
- 把临时探索、AI 中间产物、生成物或未验证能力混进交付 PR。
- PR 动辄上万行，review 被迫同时承担需求澄清、架构裁剪、范围控制和代码审查。

当前仓库只有基础 Rust CI，缺少共同工作协议、issue 模板、PR 模板和 PR 级硬门禁。治理目标不是增加流程感，而是把思考前移，让每个要求在提出后真的可达成、可验证、可审查。

## 目标

- 把关键思考从 PR review 前移到 issue 和轻量 SDD。
- 让 PR 小到可以认真 review、必要时可以回滚。
- 让非目标和验证证据在实现前明确。
- 阻止临时物、生成物和未验证能力进入交付 PR。
- 用 CI 检查客观事实，把主观价值判断留给 reviewer。
- 文档、注释和协作说明中文优先，降低团队沟通损耗。

## 非目标

- 不建立重流程框架。
- 不把某次技术讨论中的阶段性架构方向写成长期规则。
- 不要求拼写修正、模板维护等 trivial change 都走完整 issue 流程。
- 不阻止探索，但探索结果默认是结论、证据和建议，不是混入交付 PR 的临时代码。
- 不要求中英双写。标识符、API、命令、crate 名称保持英文即可。

## 工作入口

非平凡变更必须先有 issue。非平凡变更指会新增或改变行为、公共 workflow、架构、依赖、数据格式、测试 fixture 或交付面的改动。

trivial change 可以由 maintainer 加 `no-issue-needed` 标签跳过 issue 绑定，例如：

- 拼写或排版修正；
- 模板或 CI 的小维护；
- maintainer 明确要求的机械清理；
- 不改变行为的小范围注释修正。

每个非平凡 issue 必须写清：

- 真实问题或用户价值；
- 为什么现在做；
- 明确非目标；
- 最小可 review 切片；
- 验证计划或预期证据；
- 预计修改的文件或模块；
- 如果可能超出 PR 预算，提前给出拆分计划。

允许 spike 或 exploration issue。它的输出应是发现、证据和下一步建议。临时探索代码默认不进入交付 PR。

## 轻量 SDD

实现前，issue 或关联文档必须包含轻量 SDD：

- 问题；
- 非目标；
- 考虑过的方案；
- 选中方案；
- 最小可 review 切片；
- 验证计划；
- PR 拆分计划。

SDD 要短。它的作用是逼迫取舍，不是生产大设计文档。小改动没有有意义的备选方案时，`考虑过的方案` 可以写 `N/A`。

这条规则对应 Karpathy 风格的四个落点：

- 先思考再编码：显式写假设、取舍和非目标。
- 简单优先：只做最小切片，不为想象中的未来加弹性。
- 外科手术式改动：只碰本次目标需要的文件。
- 目标驱动执行：每个 PR 必须给出可验证证据。

## 语言规则

团队主要开发者使用中文，因此协作文档、issue、PR 描述、设计说明和非显然注释中文优先。

建议：

- issue、PR、设计文档、review 说明使用中文。
- 代码标识符、公共 API、命令、crate、模块名保持英文。
- 注释可以中文，但只解释业务意图、架构取舍、边界约束或不容易从代码看出的原因。
- 不给显然代码加翻译式注释。
- 存在国际合作时，默认对方可以使用 AI 翻译；不为了假想读者牺牲团队主要沟通效率。

## AGENTS.md 上下文预算

`AGENTS.md` 是会进入人和 AI 常驻上下文的高优先级文件。写入其中的内容必须极为必要，因为每一条规则都会长期消耗注意力并影响后续任务判断。

新增或修改 `AGENTS.md` 内容前，必须同时满足：

- 长期有效，不是阶段性讨论结论；
- 跨任务高频触发，不是单个 issue 或单个模块的局部要求；
- 需要人或 AI 在动手前主动判断，不只是客观事实；
- 无法由更局部的 issue 模板、PR 模板、CI、脚本、普通文档或代码注释更好承载；
- 足够简短，不需要背景故事才能理解。

不应写入 `AGENTS.md` 的内容包括：阈值、标签细节、脚本行为、历史解释、一次性要求、局部实现细节、阶段性架构选择，以及能被自动门禁检查的事实。

## PR 预算

默认 PR 预算：

- changed files: `<= 20`；
- additions: `<= 800`；
- total diff: `<= 1200`；
- 单个 source 文件 additions: `<= 300`；
- fixture 单文件: `<= 100KB`；
- fixture 总增量: `<= 300KB`；
- binary files: 默认不允许。

删除代码通常是好事，因此删除量只 warning：

- deletions `> 1000`：warning，不 hard fail。

文档和 spec 也要控制体量：

- 单个文档 additions `> 400`：warning；
- 超长文档必须说明摘要和不能拆分的理由。

PR 作者不手填这些数字。CI 自动计算，并输出当前值、阈值和下一步动作。

## 例外机制

`approved-large-change` 只能由 maintainer 添加，只豁免 PR 规模限制。PR 必须说明为什么不能拆。

初始例外标签：

- `no-issue-needed`：trivial change 跳过 issue 绑定。
- `approved-large-change`：只豁免 PR 规模限制。
- `approved-large-fixture`：允许超过 fixture 默认预算。
- `approved-binary-artifact`：允许有意提交的二进制产物。

这些标签不豁免：

- 删除、禁用或削弱 CI；
- temporary probe；
- generated dump；
- AI 中间产物；
- 未批准的交付面；
- 未验证能力进入 production path。

production path 指从已交付 CLI、library API、默认 workflow 或默认测试/验证路径可达的代码路径。未验证 parser、format 或实验能力只能存在于 issue、设计说明、spike 分支、test helper 或 example 中，并且必须与生产行为清晰隔离。

## 交付 PR 禁止项

交付 PR 不得包含：

- 临时 probe 或本地调试程序；
- AI 中间产物；
- generated dump；
- 未批准的大 fixture；
- 未批准的二进制产物；
- 从 production path 可达的未验证 parser 或 format；
- 未经 issue/SDD 批准的 UI、服务、命令或其他交付面；
- 不支撑当前 MVP 决策或用户 workflow 的文档。

## CI Guard

新增 PR guard job，检查客观事实：

- PR 是否绑定 issue，除非存在 `no-issue-needed`；
- PR 是否超过规模预算，除非存在 `approved-large-change`；
- 单个 source 文件 additions 是否超过预算；
- fixture、binary、generated、temporary 文件是否符合策略；
- CI workflow 是否被删除、禁用或削弱。

失败输出必须可行动：

```text
PR exceeds review budget:
- additions: 20648 > 800
- changed files: 113 > 20

Split this PR, remove low-value scope, or ask a maintainer for
approved-large-change with a reason why it cannot be split.
```

warning 不阻塞合入，但必须在 job summary 中明确展示，方便 reviewer 判断。

## 计划新增文件

- `AGENTS.md`：中文团队和 AI 工作协议，只放长期、高频、跨任务、需要动手前主动判断的极必要原则。
- `.github/ISSUE_TEMPLATE/feature-slice.yml`：中文 issue 入口模板，要求验证计划或预期证据，不要求 issue 阶段提供实际执行结果。
- `.github/pull_request_template.md`：中文 PR 模板，要求实际验证证据。
- `.github/scripts/pr_guard.py`：确定性 PR 检查脚本。
- `.github/workflows/pr-guard.yml`：PR guard workflow。

## 成功标准

- 没有 issue 绑定的非平凡 PR 会失败，除非 maintainer 显式豁免。
- 近期 10k+、20k+ 行级别 PR 会被 CI 明确拦住，并给出具体原因。
- 超长文档不会静默变成新的大 PR 模式。
- maintainer 可以批准真实不可拆的大改，但不能绕过安全底线。
- 规范足够短，成员能在编码前读完并照做。
- 新要求提出时，作者必须能说明最小切片和验证证据；说不清时，不能进入实现。
