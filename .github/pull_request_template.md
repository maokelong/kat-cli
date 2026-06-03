## 关联 issue

Closes #

## SDD 摘要

问题：

非目标：

考虑过的方案：

选中方案：

最小切片：

## 验证证据

请写出实际执行过的命令、样例输入、关键输出，或说明为什么本次只需要较小验证。

```text
cargo check --locked
cargo test --locked
```

## 范围自检

- [ ] 本 PR 没有引入临时 probe、本地调试程序、AI 中间产物或 generated dump。
- [ ] 本 PR 没有引入未经 issue/SDD 批准的 UI、服务、命令或其他交付面。
- [ ] 本 PR 没有把未验证 parser、format 或实验能力接入 production path。
- [ ] 本 PR 的实际改动范围与 issue 中的预计文件 / 模块一致；不一致处已在上方解释。
- [ ] 如果本 PR 明显偏大，已说明为什么不能继续拆成更小的可 review 切片。
