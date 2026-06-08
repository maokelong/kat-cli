# maokelong 作为唯一 Code Owner 放行人设计

## 问题

近期有 PR 在没有明确人工评审的情况下进入主干。当前仓库已有 PR guard 和 CI，但这些检查只能验证客观规则，不能表达“必须由指定维护者放行”。

本次目标是采用 GitHub 原生机制，让 `main` 的合入必须经过 `maokelong` 评审批准。

## 非目标

- 不在本次设计中新增自定义 approval guard 脚本。
- 不把 GitHub 平台权限规则复制进 `AGENTS.md`。
- 不改变现有 PR 规模、issue 绑定、临时产物和 CI 弱化检查。
- 不扩大 reviewer 组；唯一放行账号为 `maokelong`。

## 选中方案

采用 GitHub 原生分支保护和 CODEOWNERS：

1. 在仓库新增 `CODEOWNERS`：

   ```text
   * @maokelong
   ```

2. 在 GitHub 的 `main` 保护规则或 ruleset 中启用：
   - Require a pull request before merging。
   - Require approvals，最少 1 个 approval。
   - Require review from Code Owners。
   - Dismiss stale pull request approvals when new commits are pushed。
   - Require status checks to pass before merging，并保留现有 `pr-guard` 和 `test`。
   - Restrict who can push to matching branches，避免绕过 PR 直接 push `main`。

## 最小切片

仓库内只提交 `CODEOWNERS`，并在 PR 描述中列出需要维护者手动配置的 GitHub 分支保护项。

GitHub 分支保护属于仓库设置，不应伪装成仓库代码。仓库代码负责声明 owner，GitHub 设置负责强制执行。

## 验证计划

合入前验证：

```text
git check-ignore CODEOWNERS
git status --short
```

人工验证：

- 在 GitHub UI 中确认 `main` 保护规则开启 Code Owners review。
- 创建测试 PR，确认没有 `maokelong` approval 时不可合入。
- 由 `maokelong` approve 当前 HEAD 后，确认合入按钮解除 review 阻塞。
- 给同一个 PR 推送新 commit，确认旧 approval 被 dismiss。

## 风险与边界

- `CODEOWNERS` 只有在 GitHub 分支保护或 ruleset 开启 Code Owners review 后才会阻塞合入。
- 如果仓库管理员保留直接 push `main` 权限，仍可能绕过 PR review；因此需要限制 direct push。
- 如果未来需要多个维护者共同放行，可以把 `CODEOWNERS` 改成团队或多人列表，但这不是本次目标。
