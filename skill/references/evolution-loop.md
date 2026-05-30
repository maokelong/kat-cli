# 自进化循环入口

当用户要求改进、进化、评测或多轮优化本 skill 时，先读取这些持久状态，而不是依赖聊天上下文：

1. `docs/superpowers/specs/2026-05-29-skill-self-evolution-design.md`
2. `docs/superpowers/plans/2026-05-29-skill-self-evolution.md`
3. 仓库根目录的 `version.md`
4. 最近一轮 `D:\work\self_improved\test\evolution\round-*\handoff.md`

执行自进化循环时使用外置脚本目录：

- `D:\work\self_improved\test\evolution-tools\install-round-skill.ps1`
- `D:\work\self_improved\test\evolution-tools\verify-round.ps1`
- `D:\work\self_improved\test\evolution-tools\new-agent-prompts.ps1`
- `D:\work\self_improved\test\evolution-tools\collect-round-report.ps1`
- `D:\work\self_improved\test\evolution-tools\assert-round-submission.ps1`

这些脚本不进入代码仓。运行产物、agent prompts、round reports、handoff 和辅助日志都写入 `D:\work\self_improved\test\evolution`。

每轮必须满足：

- 使用当前仓库的 `skill/` 安装出轮次隔离 skill。
- E2E 分析必须使用 `D:\work\self_improved\test\test.htrace`。
- E2E 必须按顺序走完 8 个阶段：`collect_input`、`load_profile`、`overview_atomics`、`topdown_brief`、`strategy_selection`、`deep_analysis`、`replay_generation`、`final_report`。
- E2E 只能通过 `htrace run go`、`run guard`、`run validate`、`run advance` 等受控入口推进，不直接调用 trace processor 执行 SQL。
- 至少运行 Flow Auditor、Report Reviewer、Performance Reviewer 三类独立评审代理。
- Report Reviewer 的提示必须显式检查最终报告 `Validation notes` 中是否列出 `final_report validate`、`completed validate`、`final_report advance` 的 stdout/stderr command-output artifact，以及对应 command id/序号或简短命令文本；stderr 为空时也要检查报告是否写明 artifact 存在且为空。
- 修改源码或 skill 行为时，由 Implementation Worker 执行，并明确使用合适的 Superpowers workflow。
- 每轮结束前更新 `version.md`，记录分数、修复、发现、延期项、hard failures、下一轮候选和 handoff。
- 成功轮次必须提交并 push 当前分支；测试区产物不提交。
- 提交/push 是强制出口门禁，不是收尾建议。`version.md` 更新后必须创建轮次提交，push 当前分支，再运行 `assert-round-submission.ps1 -Round <N>` 并得到 `ok=true`。门禁通过前，本轮不得标记为完成，也不得启动下一轮。
- 遇到 E2E 未完成、流程违规、验证失败、报告缺证据、重复 hard failure、commit 或 push 失败时，停止自动循环并向用户报告。
