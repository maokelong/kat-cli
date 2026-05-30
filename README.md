# 鸿蒙 Trace Codex

`htrace` 是一个确定性的 Rust CLI runtime，用于支撑 Codex skill 分析鸿蒙性能 trace。

CLI 从 `skill/` 加载配置、知识库、原子能力和策略，执行确定性的 trace 查询，写入有界 artifact，并生成 replay plan。CLI 不调用 LLM。

## 快速开始

```bash
cargo build --release
./target/release/htrace profile list --skill-root ./skill
./target/release/htrace profile route --skill-root ./skill --question "冷启动主线程调度等待很高"
./target/release/htrace atomic run --skill-root ./skill --engine mock trace_sanity_check --trace sample.pftrace --json
./target/release/htrace replay run sample-replay.yaml --skill-root ./skill --trace sample.pftrace --engine mock --json
```

流程状态命令：

```bash
./target/release/htrace run init --out runs --trace sample.pftrace --question "冷启动为什么慢" --json
./target/release/htrace run go runs/<run-id> --json
./target/release/htrace run status runs/<run-id> --json
./target/release/htrace run validate runs/<run-id> --json
./target/release/htrace run guard runs/<run-id> --action write_final_report --json
```

`run init --out runs` 会把 `.last-run` 写到 `runs/` 的父目录；恢复时读取 `.last-run` 得到 run_dir，并显式传给 `run go/status/validate/guard/advance`。Agent 每轮应先调用 `run go --json`，按返回的当前阶段、允许动作和允许产物行动；阶段产物写入后先 `run validate --json`，没有 error finding 时再 `run advance`。

分析 Perfetto-compatible trace 时，先配置：

```bash
export HTRACE_TRACE_PROCESSOR=/path/to/trace_processor
./target/release/htrace atomic run --skill-root ./skill --engine perfetto trace_sanity_check --trace app.pftrace --json
```

`htrace` 只做确定性执行。Topdown Brief、策略选择和最终报告由 Codex skill 中的 LLM 工作流完成。

## Codex skill 安装

仓内 `skill/` 目录可以直接安装到 Codex：

```powershell
powershell -ExecutionPolicy Bypass -File .\skill\install.ps1
```

默认安装位置是 `%USERPROFILE%\.codex\skills\harmony-trace-analysis`。安装后重启 Codex 或开启新的 Codex 会话，即可通过 `$harmony-trace-analysis` 或相关 trace 分析请求触发该 skill。

## 迭代交接

继续演进前，先阅读：

- `docs/NEXT_ITERATION_HANDOFF.md`：当前实现状态、`test.htrace` 验证结论、测试方法和下一轮优先级。
- `docs/RUST_CLI_ARCHITECTURE.md`：Rust CLI 架构、工作流、边界和演进方向。
