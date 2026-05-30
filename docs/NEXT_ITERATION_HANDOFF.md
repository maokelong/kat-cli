# 鸿蒙 Trace OpenCode 下一轮迭代交接

本文是无上下文继续迭代的入口文件。下次接手时，先读本文，再看 `README.md`、`docs/RUST_CLI_ARCHITECTURE.md`、`skill/SKILL.md` 和最近一次验证目录。

## 项目位置

- 项目根目录：`D:\work\smartperf\harmony-trace-opencode`
- 示例 trace：`D:\work\smartperf\test\test.htrace`
- SmartPerfetto 参考仓库：`D:\work\smartperf\SmartPerfetto`
- 可用 trace processor：
  `D:\work\smartperf\SmartPerfetto\backend\prebuilts\trace_processor\win32-x64\trace_processor_shell.exe`
- 最近一次验证目录：
  `D:\work\smartperf\harmony-trace-opencode\validation\test-htrace-opencode-e2e-20260526-212534`

## 当前目标

构建一个 OpenCode skill，用于鸿蒙性能 trace 分析。核心工作流是：

1. 用户在 OpenCode 中调用 skill，输入分析问题。
2. 根据用户角色和领域，加载对应知识库。
3. 先用 overview atomic 观察当前 trace 真实存在什么问题，生成 Topdown Brief。
4. 基于 Topdown Brief 和问题选择分析策略。
5. 若没有合适的 approved strategy，由 LLM 生成 draft strategy，提交用户审核。
6. 按策略逐步执行 atomic；后续阶段是否执行、如何执行，取决于前面阶段输出。
7. 生成 replay/signature，用确定性步骤判断其他 trace 是否存在同类问题。
8. 输出最终分析报告。

## 已确定的架构原则

- 机制与策略分离。
- 原子能力使用 YAML 描述，放在 `skill/atomics/`。
- 分析策略使用 Markdown 描述，放在 `skill/strategies/`；不要把策略流程硬塞进 YAML。
- 领域知识放在 `skill/knowledge/`。
- role/profile/config 放在 `skill/config/`。
- skill 自身资源全部放在 `skill/` 下，避免和其他 skill 混淆。
- Rust CLI runtime 只做确定性执行，不调用 LLM。
- LLM 负责 Topdown Brief、策略选择、分支判断和最终报告。
- trace backend 通过 engine 接口隔离；当前使用 `trace_processor_shell`，后续要能替换为 Rust parser。
- 面向 16G 内存，批量分析时应限制并发、结果大小和 trace processor 生命周期。

## 当前实现状态

代码目录：

- `cli/src/commands/atomic.rs`：执行单个 atomic。
- `cli/src/commands/replay.rs`：重放 replay plan，支持单 trace 和 batch。
- `cli/src/engine/perfetto_shell.rs`：通过 `trace_processor_shell.exe -q <sql-file> <trace>` 查询。
- `cli/src/config/`：加载 skill 配置、atomic、strategy、profile。
- `cli/src/executor/params.rs`：把 replay/CLI 参数填入 atomic SQL。
- `skill/atomics/`：已放入调度/内核冷启动打样所需 atomic。
- `skill/strategies/approved/cold-start-scheduler-topdown.md`：已批准的冷启动调度 topdown 策略。
- `skill/knowledge/scheduler-kernel/`：调度/内核领域知识。

Run workflow state 已作为核心能力落地；用户分析任务写入 `runs/<run-id>/run-state.yaml` 和 `progress.md`；用 `htrace run status/guard/advance` 恢复和推进阶段；`validation/` 仍只用于开发验证。

CLI 当前已有入口：

```powershell
htrace profile list
htrace profile route
htrace atomic list
htrace atomic run
htrace strategy list
htrace strategy render
htrace replay run
htrace replay batch
htrace run init
htrace run status
htrace run guard
htrace run advance
```

Run 恢复步骤：`htrace run init --out runs ...` 会把 `.last-run` 写到 `runs/` 的父目录。恢复时先用 `Get-Content .last-run` 取得 run_dir，再把该路径显式传给 `htrace run status/guard/advance`。

注意：当前环境曾出现 `cargo`/`rustc` 不可用，因此 Rust 编译测试可能需要先安装 toolchain。

## 已验证的 trace 结论

使用 `D:\work\smartperf\test\test.htrace` 打样，目标进程识别为：

- `process_name=.tencent.wechat`
- `upid=89`
- `pid=15040`
- `start_ts=245644541000`
- `end_ts=257886174999`

关键证据：

- trace 基础表可用：
  - `process=155`
  - `thread=1707`
  - `sched=230895`
  - `thread_state=287441`
- `process.name` 和 `process.start_ts` 在该 trace 中缺失。
- 需要通过 main thread 名称和线程活动窗口回填进程名与启动窗口。
- 主线程 runnable 总等待约 `18.103ms`，最大 runnable wait 约 `1.608ms`。
- 当前证据不支持“主线程 runnable latency 是主要瓶颈”。
- blocking 信号更明显：
  - `unknown_block` 约 `1748.444879ms`
  - `uninterruptible_block` 约 `344.976ms`
  - `io_wait` 约 `37.517ms`
  - `lock_futex` 约 `0.415ms`

当前问题签名：

```yaml
problem_signature: cold_start_blocking_dominant_v1
```

## 当前 replay.yaml 的含义

最近一次生成的 replay：

`validation\test-htrace-opencode-e2e-20260526-212534\replay.yaml`

它记录的是本次 trace 上已确认的确定性分析步骤，包括：

- `trace_sanity_check`
- `process_startup_candidates`
- `main_thread_state_overview`
- `sched_latency_overview`
- `blocking_category_overview`
- `cpu_contention_summary`

重要限制：

- 当前 `replay.yaml` 里包含 `test.htrace` 的固定 `start_ts/end_ts`。
- 不能原封不动套到其他 trace 上直接判断同类问题。
- 其他 trace 必须先重新 capture 目标进程和窗口，再执行后续步骤。
- 当前 CLI 的 `replay run/batch` 只是重放执行 atomic，还没有自动执行 `capture` 和 `assertions` 判定。

## 下一轮最高优先级

先做真正的 `replay judge`，把当前 replay 从“执行计划”升级为“同类问题判定器”。

目标输入：

```text
signature/replay yaml + one or many traces
```

目标输出：

```yaml
trace: D:\path\to\trace.htrace
problem_signature: cold_start_blocking_dominant_v1
same_problem: true
evidence_dir: D:\path\to\evidence
captures:
  process_name: .tencent.wechat
  upid: 89
  pid: 15040
  start_ts: 245644541000
  end_ts: 257886174999
passed_assertions:
  - main_thread_max_runnable_wait_ms < 5
failed_assertions: []
```

需要补齐的能力：

1. `capture` 变量绑定。
2. `{{capture.start_ts}}` 这类变量引用。
3. atomic 输出结构化解析。
4. assertions 自动比较。
5. 每个 trace 独立 evidence 目录。
6. batch 模式输出每份 trace 的 `same_problem: true/false`。

## 建议的 replay/signature 设计调整

建议把“本次执行记录”和“可复用问题签名”拆开：

- `replay.yaml`：某次 trace 的确定性执行记录。
- `signature.yaml`：跨 trace 复用的问题判定规则。

示例：

```yaml
problem_signature: cold_start_blocking_dominant_v1
domain: scheduler-kernel
selector:
  atomic: process_startup_candidates
  params:
    process_name: wechat
  capture:
    process_name: rows[0].process_name
    upid: rows[0].upid
    pid: rows[0].pid
    start_ts: rows[0].start_ts
    end_ts: rows[0].end_ts
steps:
  - atomic: main_thread_state_overview
    params:
      process_name: "{{capture.process_name}}"
      start_ts: "{{capture.start_ts}}"
      end_ts: "{{capture.end_ts}}"
    assertions:
      - metric: main_thread_runnable_ms
        op: "<"
        value: 50
  - atomic: sched_latency_overview
    params:
      process_name: "{{capture.process_name}}"
      start_ts: "{{capture.start_ts}}"
      end_ts: "{{capture.end_ts}}"
    assertions:
      - metric: main_thread_max_runnable_wait_ms
        op: "<"
        value: 5
  - atomic: blocking_category_overview
    params:
      process_name: "{{capture.process_name}}"
      start_ts: "{{capture.start_ts}}"
      end_ts: "{{capture.end_ts}}"
    assertions:
      - metric: unknown_block_ms
        op: ">"
        value: 1000
      - metric: uninterruptible_block_ms
        op: ">"
        value: 100
```

## 测试方法

### 1. 基础仓库检查

```powershell
cd D:\work\smartperf\harmony-trace-opencode
git status --short --branch
```

预期：

- 源码改动应清晰可解释。
- `validation/` 和 `.last-validation-dir` 可能是验证产物，提交前需要决定是否纳入版本库。

### 2. Rust 编译测试

如果已安装 Rust toolchain：

```powershell
cd D:\work\smartperf\harmony-trace-opencode
cargo test
cargo build --release
```

如果环境没有 `cargo`：

```powershell
cargo --version
rustc --version
```

记录缺失即可，不要声称 Rust 编译已通过。

### 3. Mock engine 快速测试

编译成功后：

```powershell
.\target\release\htrace profile list --skill-root .\skill
.\target\release\htrace profile route --skill-root .\skill --question "冷启动主线程调度等待很高"
.\target\release\htrace atomic run --skill-root .\skill --engine mock trace_sanity_check --trace sample.pftrace --json
.\target\release\htrace replay run sample-replay.yaml --skill-root .\skill --trace sample.pftrace --engine mock --json
```

预期：

- 命令退出码为 `0`。
- 输出 JSON 可被解析。
- replay 返回 step_count 和每步 status。

### 4. 真实 trace processor 基础测试

PowerShell：

```powershell
$env:HTRACE_TRACE_PROCESSOR="D:\work\smartperf\SmartPerfetto\backend\prebuilts\trace_processor\win32-x64\trace_processor_shell.exe"
& $env:HTRACE_TRACE_PROCESSOR --version
```

预期：

- 能输出 Perfetto 版本。
- 当前验证过的版本为 `Perfetto v54.0-7616314b3`。

### 5. 单 atomic 真实 trace 测试

编译成功后：

```powershell
cd D:\work\smartperf\harmony-trace-opencode
$env:HTRACE_TRACE_PROCESSOR="D:\work\smartperf\SmartPerfetto\backend\prebuilts\trace_processor\win32-x64\trace_processor_shell.exe"

.\target\release\htrace atomic run `
  --skill-root .\skill `
  --engine perfetto `
  trace_sanity_check `
  --trace D:\work\smartperf\test\test.htrace `
  --json
```

预期：

- 命令退出码为 `0`。
- 输出 status 为 `ok`。

### 6. replay run 真实 trace 测试

```powershell
cd D:\work\smartperf\harmony-trace-opencode
$env:HTRACE_TRACE_PROCESSOR="D:\work\smartperf\SmartPerfetto\backend\prebuilts\trace_processor\win32-x64\trace_processor_shell.exe"

.\target\release\htrace replay run `
  .\validation\test-htrace-opencode-e2e-20260526-212534\replay.yaml `
  --skill-root .\skill `
  --trace D:\work\smartperf\test\test.htrace `
  --engine perfetto `
  --json
```

当前预期：

- 所有 replay step 能执行。
- 只能说明 atomic 重放成功。
- 还不能说明 `same_problem=true`，因为 assertions 尚未自动执行。

### 7. batch 测试

```powershell
cd D:\work\smartperf\harmony-trace-opencode
$env:HTRACE_TRACE_PROCESSOR="D:\work\smartperf\SmartPerfetto\backend\prebuilts\trace_processor\win32-x64\trace_processor_shell.exe"

.\target\release\htrace replay batch `
  .\validation\test-htrace-opencode-e2e-20260526-212534\replay.yaml `
  --skill-root .\skill `
  --trace D:\traces\a.htrace `
  --trace D:\traces\b.htrace `
  --jobs 2 `
  --engine perfetto
```

约束：

- 16G 内存机器上不要盲目把 `--jobs` 开太大。
- 每个 trace_processor 进程都会加载 trace，批量时优先 `--jobs 2` 或 `--jobs 4` 试探。

### 8. 无 Rust toolchain 时的 trace 查询回归

如果当前环境没有 `cargo`，仍可以用最近验证目录里的 SQL 手动回归：

```powershell
$tp="D:\work\smartperf\SmartPerfetto\backend\prebuilts\trace_processor\win32-x64\trace_processor_shell.exe"
$trace="D:\work\smartperf\test\test.htrace"
$raw="D:\work\smartperf\harmony-trace-opencode\validation\test-htrace-opencode-e2e-20260526-212534\raw"
$out="D:\work\smartperf\harmony-trace-opencode\validation\manual-regression"

New-Item -ItemType Directory -Force -Path $out | Out-Null

& $tp -q "$raw\00_trace_sanity_check.sql" $trace 1> "$out\00_trace_sanity_check.csv" 2> "$out\00_trace_sanity_check.stderr.txt"
& $tp -q "$raw\04_process_startup_candidates_wechat.sql" $trace 1> "$out\04_process_startup_candidates_wechat.csv" 2> "$out\04_process_startup_candidates_wechat.stderr.txt"
& $tp -q "$raw\05_main_thread_state_wechat.sql" $trace 1> "$out\05_main_thread_state_wechat.csv" 2> "$out\05_main_thread_state_wechat.stderr.txt"
& $tp -q "$raw\06_sched_latency_wechat.sql" $trace 1> "$out\06_sched_latency_wechat.csv" 2> "$out\06_sched_latency_wechat.stderr.txt"
& $tp -q "$raw\07_blocking_category_wechat.sql" $trace 1> "$out\07_blocking_category_wechat.csv" 2> "$out\07_blocking_category_wechat.stderr.txt"
```

预期：

- 每条命令 `$LASTEXITCODE` 为 `0`。
- 输出 CSV 非空。

## OpenCode E2E 验证记录

最近一次验证中：

- `opencode run` 能读取项目和验证目录。
- `opencode run` 能实际执行 shell 并写入文件。
- 强制查询会话中，opencode 实际调用过 `trace_processor_shell.exe`，并生成了部分 atomic 的 `stdout/stderr/perf`。
- 长链路 opencode 代理受 10 分钟超时和 `server_is_overloaded` 影响，未能独立写完完整报告。
- 因此不要把最近一次结果表述为“opencode 完整自主闭环已通过”；应表述为“deterministic atomic 证据链已跑通，opencode shell/二进制调用能力已验证，长链路代理稳定性仍需优化”。

相关文件：

- `validation\test-htrace-opencode-e2e-20260526-212534\opencode-e2e-notes.md`
- `validation\test-htrace-opencode-e2e-20260526-212534\opencode\opencode-run.jsonl`
- `validation\test-htrace-opencode-e2e-20260526-212534\opencode\opencode-confirm.jsonl`
- `validation\test-htrace-opencode-e2e-20260526-212534\opencode\opencode-probe.jsonl`
- `validation\test-htrace-opencode-e2e-20260526-212534\opencode-actual\run-status.txt`

## 下一轮建议实施顺序

1. 为 `ReplayPlan` 扩展模型：保留 `target`、`capture`、`assertions`、`evidence` 字段。
2. 为 `PerfettoShellEngine` 增加 CSV 解析，输出结构化 rows。
3. 新增 metric extractor：从 rows 中按 metric 名称提取数值。
4. 新增 assertion evaluator：支持 `>`, `>=`, `<`, `<=`, `==`, `!=`。
5. 新增 capture resolver：支持把 selector atomic 的输出写入上下文。
6. 新增模板替换：支持 `{{capture.xxx}}`。
7. 新增 `htrace replay judge` 或扩展 `replay run --judge`。
8. 新增 batch judge 输出：

   ```json
   {"trace":"...","same_problem":true,"problem_signature":"cold_start_blocking_dominant_v1","evidence_dir":"..."}
   ```

9. 用 `test.htrace` 做 golden case。
10. 再准备一个负样本 trace，验证 `same_problem=false`。

## 完成标准

下一轮如果做 `replay judge`，至少满足：

- `cargo test` 通过，或明确记录当前环境没有 Rust toolchain。
- `htrace replay judge` 能对 `test.htrace` 输出 `same_problem=true`。
- evidence 目录包含每步 atomic 的 SQL、CSV、stderr、结构化 JSON。
- 任一 assertion 失败时，最终结果为 `same_problem=false`，并列出失败项。
- batch 模式可以并发处理多 trace，且 `--jobs` 可控。
- 文档更新 `README.md` 或本文。
