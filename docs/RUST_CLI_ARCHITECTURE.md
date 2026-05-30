# Rust CLI 架构设计

本文描述 `htrace` Rust CLI 的当前架构、工作流、边界和后续演进方向。CLI 的定位是 OpenCode skill 背后的确定性执行运行时，不承担 LLM 推理职责。

## 设计定位

`htrace` 不是分析智能体，而是确定性 runtime：

- 负责加载 skill 配置。
- 负责执行 atomic。
- 负责调用 trace backend。
- 负责批量重放 replay。
- 负责输出可被 LLM 或上层工作流消费的结构化结果。

LLM/OpenCode skill 负责：

- 理解用户问题。
- 加载领域知识。
- 生成 Topdown Brief。
- 选择或生成策略。
- 根据前序结果决定后续步骤。
- 编写最终分析报告。

因此 CLI 应保持可复现、可测试、低资源占用，并避免写入策略推理逻辑。

## 目录结构

关键源码目录：

```text
cli/src/
  main.rs
  lib.rs
  commands/
    atomic.rs
    profile.rs
    replay.rs
    run.rs
    strategy.rs
  config/
    loader.rs
    models.rs
    paths.rs
  engine/
    mod.rs
    mock.rs
    perfetto_shell.rs
  executor/
    params.rs
    artifacts.rs
  replay/
    model.rs
  run/
    model.rs
    workflow.rs
    progress.rs
```

顶层 workspace：

```text
Cargo.toml
cli/Cargo.toml
```

`cli` crate 同时提供 library 和 binary：

- library：`htrace`
- binary：`htrace`

## 分层架构

### 1. 入口层

文件：

- `cli/src/main.rs`

职责：

- 使用 `clap` 定义顶层命令。
- 解析 CLI 参数。
- 将命令分发给对应 command module。

当前顶层命令：

```text
htrace version
htrace profile ...
htrace strategy ...
htrace atomic ...
htrace replay ...
htrace run ...
```

入口层不应包含业务逻辑。

### 2. 命令层

目录：

- `cli/src/commands/`

当前命令：

```text
profile list
profile route
strategy list
strategy render
strategy lint
atomic list
atomic run
replay run
replay batch
run init
run go
run status
run validate
run guard
run advance
```

职责：

- 接收 CLI 参数。
- 调用配置加载、参数准备、engine 执行等底层能力。
- 将结果打印为 human-readable 或 JSON。

主要文件：

- `profile.rs`：列出 profile，并根据用户问题 route 到领域。
- `strategy.rs`：列出、渲染、lint approved strategy。
- `atomic.rs`：执行单个 atomic。
- `replay.rs`：按 replay plan 重放步骤，支持单 trace 和 batch。

### 3. 配置层

目录：

- `cli/src/config/`

核心类型：

- `SkillRoot`
- `Profile`
- `RoleRouter`
- `Atomic`
- `Strategy`
- `StrategyMetadata`

职责：

- 从 `skill/` 目录加载配置。
- 聚合 role router、profile、atomic、approved strategy。
- 提供按 id 查询能力。

当前加载路径：

```text
skill/config/role-router.yaml
skill/config/profiles/*.yaml
skill/atomics/<domain>/*.yaml
skill/strategies/approved/*.md
```

这一层体现机制与策略分离：Rust CLI 只读取策略元信息和正文，不把策略流程硬编码进代码。

### 4. 参数与执行辅助层

目录：

- `cli/src/executor/`

当前能力：

- `params.rs`：解析 `key=value` 参数，检查 atomic 必需参数，替换 SQL 中的 `:param`。
- `artifacts.rs`：创建运行目录。

当前参数替换方式：

```text
:process_name -> 'wechat'
:start_ts -> '245644541000'
```

这是原型实现，后续需要升级为类型化模板。

### 5. Engine 层

目录：

- `cli/src/engine/`

核心 trait：

```rust
pub trait TraceQueryEngine {
    fn query(
        &self,
        atomic_id: &str,
        trace_path: &Path,
        sql: &str,
        resources: &AtomicResources,
    ) -> Result<QueryEnvelope>;
}
```

当前 engine：

- `MockTraceQueryEngine`：测试用，返回固定 ok 行。
- `PerfettoShellEngine`：通过外部 `trace_processor_shell.exe` 查询。

当前 `PerfettoShellEngine` 调用方式：

```text
trace_processor_shell.exe -q <sql-file> <trace>
```

该层是后续替换 trace backend 的核心隔离点。未来可新增：

- `RustTraceEngine`
- `PerfettoRpcEngine`
- `CachedPerfettoEngine`
- `RemoteTraceEngine`

只要实现 `TraceQueryEngine`，上层命令不应感知具体 backend。

### 6. Replay 层

目录：

- `cli/src/replay/`

当前模型：

```rust
pub struct ReplayPlan {
    pub problem_signature: String,
    pub source_strategy: String,
    pub steps: Vec<ReplayStep>,
}

pub struct ReplayStep {
    pub atomic: String,
    pub params: BTreeMap<String, String>,
}
```

当前 replay 能力：

- 读取 replay YAML。
- 顺序执行每个 step。
- 返回每步 status。
- batch 模式通过 rayon 并发处理多份 trace。

当前 replay 还不是完整判定器，只是重放执行器。

### 7. Run 状态层

目录：

- `cli/src/run/`

职责：

- 维护用户分析任务的 `run-state.yaml`。
- 从 run state 渲染 `progress.md`。
- 通过固定 8 阶段提供 `go/validate/guard/advance`，确保 Agent 先读取当前阶段事实，再执行允许动作。
- `htrace run go` 是 Agent 主入口，返回当前阶段、允许动作、允许产物、必需输入和 findings。
- `htrace run validate` 输出结构化中文 findings，用于阶段推进前检查。
- `htrace run advance` 是阶段硬门禁；关键产物缺失时拒绝推进，`run advance --to completed` 只允许从 `final_report` 终结 run，并要求 `artifacts/final-report.md` 已存在。
- 支持 OpenCode 上下文压缩后，从 `.last-run` 恢复最近一次 run。
- `.last-run` 写在 `runs/` 的同级父目录；当前 CLI 仍要求调用者读取 `.last-run` 内容，并把得到的 `<run-dir>` 显式传给 `go/status/validate/guard/advance`。

当前命令：

- `htrace run init`
- `htrace run go`
- `htrace run status`
- `htrace run validate`
- `htrace run guard`
- `htrace run advance`

边界：

Run 层不执行 trace 查询，也不生成分析报告；它只维护流程状态和阶段合法性。

## 核心数据流

### atomic run

```text
CLI 参数
  -> SkillRoot::load(skill_root)
  -> 查找 atomic
  -> parse_params
  -> prepare_sql
  -> 选择 engine
  -> engine.query(trace, sql)
  -> 输出 QueryEnvelope
```

当前 JSON 输出模型：

```text
QueryEnvelope
  status
  atomic_id
  engine
  trace
  rows
  artifacts
  stats
```

### replay run

```text
读取 replay.yaml
  -> 解析 ReplayPlan
  -> SkillRoot::load(skill_root)
  -> 遍历 steps
  -> 查找 atomic
  -> prepare_sql
  -> engine.query
  -> 收集 status
  -> 输出 ReplayRunSummary
```

当前输出：

```text
ReplayRunSummary
  problem_signature
  source_strategy
  step_count
  statuses
```

### replay batch

```text
多份 trace
  -> 创建 rayon thread pool
  -> 每个 trace 调 run_one
  -> 每份 trace 输出一行 JSON summary
```

batch 的并发度由 `--jobs` 控制。面向 16G 内存环境时，不应默认开过大并发，因为每个 trace processor 进程都会加载 trace。

## 当前优点

- CLI 与 skill 配置分离，职责边界清楚。
- atomic 以 YAML 声明，是稳定的最小执行单元。
- strategy 以 Markdown 声明，没有硬编码进 Rust。
- trace backend 已通过 `TraceQueryEngine` 隔离。
- CLI 不调用 LLM，便于测试和复现。
- replay batch 已具备并行执行基础。
- profile/role-router 已初步支持根据问题选择领域。

## 当前限制

### 1. Replay 还不能自动判定同类问题

当前 `replay.yaml` 里可以写 `target`、`capture`、`assertions`、`evidence` 等字段，但 Rust 模型还没有接收这些字段。

结果是：

- CLI 可以执行步骤。
- CLI 不会自动绑定 capture。
- CLI 不会自动比较 assertions。
- CLI 不会输出 `same_problem: true/false`。

### 2. Perfetto 输出尚未结构化

当前 `PerfettoShellEngine` 只把 stdout 放入：

```json
{"raw_stdout": "..."}
```

它还没有：

- CSV parser
- column schema 校验
- 数字类型转换
- row limit
- artifact 落盘

这会限制 replay judge 和 metric extraction。

### 3. resources 尚未完整生效

atomic YAML 中已有：

```yaml
resources:
  timeout_ms
  max_rows
  max_result_bytes
  priority
```

但当前实现中：

- `timeout_ms` 没有真正限制子进程。
- `max_rows` 没有截断输出。
- `max_result_bytes` 只用于标记 truncated。
- `priority` 尚未参与调度。

### 4. 参数替换仍是原型实现

当前直接字符串替换 `:key`，并给所有参数加单引号。

风险：

- 类型不精确。
- 可能误替换相似参数名。
- 不支持变量上下文，例如 `{{capture.start_ts}}`。

### 5. Engine 选择逻辑重复

`atomic.rs` 和 `replay.rs` 都有 mock/perfetto 的选择逻辑。后续应抽出 `EngineFactory`。

## 下一阶段设计方向

### 1. 增加 replay judge

目标命令：

```text
htrace replay judge signature.yaml --trace app.htrace --engine perfetto --out runs/app
```

目标输出：

```json
{
  "trace": "app.htrace",
  "problem_signature": "cold_start_blocking_dominant_v1",
  "same_problem": true,
  "evidence_dir": "runs/app",
  "captures": {
    "process_name": ".tencent.wechat",
    "upid": 89,
    "pid": 15040,
    "start_ts": 245644541000,
    "end_ts": 257886174999
  },
  "passed_assertions": [
    "main_thread_max_runnable_wait_ms < 5"
  ],
  "failed_assertions": []
}
```

### 2. 扩展 replay/signature 模型

建议把本次执行记录和可复用问题签名拆开：

- `replay.yaml`：某一次 trace 的执行记录。
- `signature.yaml`：跨 trace 复用的问题判定规则。

建议新增模型：

```rust
pub struct SignaturePlan {
    pub problem_signature: String,
    pub domain: String,
    pub selector: SignatureStep,
    pub steps: Vec<SignatureStep>,
}

pub struct SignatureStep {
    pub atomic: String,
    pub params: BTreeMap<String, String>,
    pub capture: BTreeMap<String, CaptureExpr>,
    pub assertions: Vec<Assertion>,
}

pub struct Assertion {
    pub metric: String,
    pub op: CompareOp,
    pub value: serde_json::Value,
}
```

### 3. 结构化 QueryEnvelope

`PerfettoShellEngine` 应把 trace processor 输出解析为 rows：

```json
{
  "rows": [
    {
      "process_name": ".tencent.wechat",
      "upid": 89,
      "start_ts": 245644541000
    }
  ],
  "artifacts": [
    {
      "path": "raw/04_process_startup_candidates_wechat.csv",
      "format": "csv",
      "row_count": 1,
      "byte_size": 103
    }
  ]
}
```

### 4. Capture 与模板替换

支持 selector step 先 capture：

```yaml
capture:
  process_name: rows[0].process_name
  upid: rows[0].upid
  start_ts: rows[0].start_ts
  end_ts: rows[0].end_ts
```

后续 step 使用：

```yaml
params:
  process_name: "{{capture.process_name}}"
  start_ts: "{{capture.start_ts}}"
  end_ts: "{{capture.end_ts}}"
```

### 5. Assertions

支持比较：

```text
>
>=
<
<=
==
!=
contains
exists
```

并输出：

- passed assertions
- failed assertions
- missing metrics
- type conversion errors

### 6. Evidence 目录

每次执行应生成独立 evidence 目录：

```text
runs/<trace-name>-<timestamp>/
  command.json
  summary.json
  steps/
    00_trace_sanity_check/
      query.sql
      stdout.csv
      stderr.txt
      envelope.json
    01_process_startup_candidates/
      query.sql
      stdout.csv
      stderr.txt
      envelope.json
```

这样 OpenCode/LLM 只需要读取 summary 和必要 CSV，不必重新猜执行过程。

## 推荐演进顺序

1. 抽出 `EngineFactory`，消除 `atomic.rs` 和 `replay.rs` 的重复 engine 选择逻辑。
2. 为 `PerfettoShellEngine` 增加 artifact 输出选项。
3. 增加 CSV parser，把 stdout 转成结构化 rows。
4. 扩展 replay model，先兼容现有 `replay.yaml`。
5. 实现 capture context。
6. 实现模板替换 `{{capture.xxx}}`。
7. 实现 assertion evaluator。
8. 新增 `replay judge`。
9. 新增 `replay judge-batch` 或扩展现有 `batch`。
10. 用 `test.htrace` 做 golden case，并准备一个负样本 trace。

## 测试策略

### 单元测试

优先覆盖：

- `parse_params`
- `prepare_sql`
- CSV parser
- capture resolver
- template renderer
- assertion evaluator
- replay/signature YAML deserialize

### 集成测试

使用 `mock` engine：

```powershell
cargo test
.\target\release\htrace atomic run --skill-root .\skill --engine mock trace_sanity_check --trace sample.pftrace --json
.\target\release\htrace replay run sample-replay.yaml --skill-root .\skill --trace sample.pftrace --engine mock --json
```

### 真实 trace 回归

使用：

```powershell
$env:HTRACE_TRACE_PROCESSOR="D:\work\smartperf\SmartPerfetto\backend\prebuilts\trace_processor\win32-x64\trace_processor_shell.exe"
```

再运行：

```powershell
.\target\release\htrace atomic run `
  --skill-root .\skill `
  --engine perfetto `
  trace_sanity_check `
  --trace D:\work\smartperf\test\test.htrace `
  --json
```

### 判定器验收

下一阶段 `replay judge` 完成后，至少验证：

- `test.htrace` 输出 `same_problem=true`。
- 任意一个关键 assertion 阈值改到不满足时，输出 `same_problem=false`。
- 失败项能明确列出 metric、actual、expected。
- batch 模式中每份 trace 独立输出 JSON。

## 设计边界

CLI 不应做：

- 不确定性推理。
- 自主生成策略。
- 编写自然语言最终报告。
- 根据模糊语义修改分析路径。

CLI 应做：

- 加载配置。
- 执行 atomic。
- 解析结构化结果。
- 判定明确 assertions。
- 生成可复现 evidence。
- 为 OpenCode skill 提供稳定接口。
