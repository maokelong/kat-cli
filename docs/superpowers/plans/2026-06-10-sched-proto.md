# sched.proto Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 接入上游 `sched.proto`，为所有 sched event 建明细表，并生成最小 `thread_state` / `instant` 派生表。

**Architecture:** `hitrace.proto` 只补 sched message 字段，保持非 sched ftrace events 不进入本次切片。datasource 在解码 `TracePluginResult` 时把每个 sched event 拆到对应 `sched_*` 原始表，同时从 `sched_switch`、`sched_wakeup`、`sched_wakeup_new`、`sched_waking` 生成派生表。

**Tech Stack:** Rust 2024, prost/prost-build, serde/serde_arrow, DataFusion, Cargo integration tests.

---

## File Structure

- Create: `crates/kat-rs-datasource/proto/ftrace_data/sched.proto`
  - 从 `D:\项目\trace_streamer\src\protos\types\plugins\ftrace_data\sched.proto` 原样复制。
- Modify: `crates/kat-rs-datasource/proto/hitrace.proto`
  - import sched proto。
  - 给 `FtraceEvent` 添加公共字段 `timestamp`、`tgid`、`comm` 和 sched message 字段。
- Modify: `crates/kat-rs-datasource/build.rs`
  - 编译 `hitrace.proto` 与 `ftrace_data/sched.proto`，并从 `sched.proto` 生成 `OUT_DIR/sched_rows.rs`。
- Modify: `crates/kat-rs-datasource/src/lib.rs`
  - include no-package sched protobuf 生成文件、`kat.hitrace` 生成文件和 sched Row 生成文件。
- Modify: `crates/kat-rs-datasource/src/hitrace.rs`
  - 使用生成的 sched Row，保留 table container、decode dispatcher、`thread_state` 与 `instant` 最小派生逻辑。
- Modify: `crates/kat-rs-datasource/src/query.rs`
  - 注册所有 sched 明细表和派生表。
- Modify: `crates/kat-rs-datasource/tests/proto_contract.rs`
  - 覆盖 upstream sched proto 生成契约。
- Modify: `crates/kat-rs-datasource/tests/hitrace_datasource_query.rs`
  - 构造包含多个 sched event 的最小 htrace，并验证新增表。
- Modify: `crates/kat-rs-cli/tests/query_e2e.rs`
  - 验证 CLI 能查询新增 sched 明细表。

---

### Task 1: Proto Contract And Baseline

**Files:**
- Modify: `crates/kat-rs-datasource/tests/proto_contract.rs`

- [ ] **Step 1: Ensure upstream sched proto contract exists**

Keep these tests:

```rust
#[test]
fn generated_proto_includes_sched_switch_format() {
    let value = proto::SchedSwitchFormat {
        prev_comm: "render".to_string(),
        prev_pid: 42,
        prev_prio: 120,
        prev_state: 1,
        next_comm: "main".to_string(),
        next_pid: 7,
        next_prio: 100,
    };

    let decoded =
        proto::SchedSwitchFormat::decode(value.encode_to_vec().as_slice()).expect("decode");

    assert_eq!(decoded.prev_comm, "render");
    assert_eq!(decoded.next_comm, "main");
}

#[test]
fn generated_proto_includes_upstream_sched_messages() {
    let value = proto::SchedBlockedReasonFormat {
        pid: 42,
        caller: 0xfeed_beef,
        io_wait: 1,
    };

    let decoded =
        proto::SchedBlockedReasonFormat::decode(value.encode_to_vec().as_slice()).expect("decode");

    assert_eq!(decoded.pid, 42);
    assert_eq!(decoded.caller, 0xfeed_beef);
    assert_eq!(decoded.io_wait, 1);
}
```

- [ ] **Step 2: Run contract test**

Run:

```powershell
cargo test -p kat-rs-datasource --test proto_contract
```

Expected: PASS after upstream sched proto is wired.

---

### Task 2: Write Failing sched Table Tests

**Files:**
- Modify: `crates/kat-rs-datasource/tests/hitrace_datasource_query.rs`

- [ ] **Step 1: Extend test protobuf helpers**

Add `timestamp`, `tgid`, and `comm` to `TestFtraceEvent`, and add optional sched fields for at least:

```rust
#[prost(message, optional, tag = "2400")]
sched_kthread_stop_format: Option<TestSchedKthreadStopFormat>,
#[prost(message, optional, tag = "2402")]
sched_migrate_task_format: Option<TestSchedMigrateTaskFormat>,
#[prost(message, optional, tag = "2417")]
sched_switch_format: Option<TestSchedSwitchFormat>,
#[prost(message, optional, tag = "2420")]
sched_wakeup_format: Option<TestSchedWakeupFormat>,
#[prost(message, optional, tag = "2421")]
sched_wakeup_new_format: Option<TestSchedWakeupFormat>,
#[prost(message, optional, tag = "2422")]
sched_waking_format: Option<TestSchedWakeupFormat>,
#[prost(message, optional, tag = "4002")]
sched_blocked_reason_format: Option<TestSchedBlockedReasonFormat>,
```

- [ ] **Step 2: Add failing datasource test**

Add a test named `query_extracts_sched_event_tables_and_derived_tables`. It should:

```rust
let rows = datasource
    .query_json("select event_timestamp, event_cpu, event_comm, pid, caller, io_wait from sched_blocked_reason")
    .await
    .expect("query succeeds");
assert_eq!(rows, json!([{ "event_timestamp": 20, "event_cpu": 3, "event_comm": "blocked_source", "pid": 42, "caller": 3735928559u64, "io_wait": 1 }]));

let rows = datasource
    .query_json("select event_timestamp, event_cpu, comm, pid, prio, orig_cpu, dest_cpu from sched_migrate_task")
    .await
    .expect("query succeeds");
assert_eq!(rows, json!([{ "event_timestamp": 30, "event_cpu": 3, "comm": "RenderThread", "pid": 42, "prio": 120, "orig_cpu": 1, "dest_cpu": 3 }]));

let rows = datasource
    .query_json("select count(*) as count from sched_process_exec")
    .await
    .expect("empty table query succeeds");
assert_eq!(rows, json!([{ "count": 0 }]));

let rows = datasource
    .query_json("select ts, cpu, tid, state, comm from thread_state order by ts, tid")
    .await
    .expect("thread_state query succeeds");
assert_eq!(rows, json!([
    { "ts": 10, "cpu": 3, "tid": 100, "state": "Running", "comm": "main" },
    { "ts": 10, "cpu": null, "tid": 42, "state": "prev_state:1", "comm": "RenderThread" },
]));

let rows = datasource
    .query_json("select ts, name, ref, wakeup_from, ref_type, value from instant order by ts, name")
    .await
    .expect("instant query succeeds");
assert_eq!(rows, json!([
    { "ts": 40, "name": "sched_wakeup", "ref": 100, "wakeup_from": 500, "ref_type": "tid", "value": 0.0 },
    { "ts": 50, "name": "sched_wakeup_new", "ref": 101, "wakeup_from": 500, "ref_type": "tid", "value": 0.0 },
    { "ts": 60, "name": "sched_waking", "ref": 102, "wakeup_from": 500, "ref_type": "tid", "value": 0.0 },
]));
```

- [ ] **Step 3: Run failing datasource test**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_datasource_query query_extracts_sched_event_tables_and_derived_tables -- --exact
```

Expected: FAIL because the new tables are not registered yet.

---

### Task 3: Implement sched Event Tables

**Files:**
- Modify: `crates/kat-rs-datasource/proto/hitrace.proto`
- Modify: `crates/kat-rs-datasource/src/hitrace.rs`
- Modify: `crates/kat-rs-datasource/src/query.rs`

- [ ] **Step 1: Add direct sched message fields**

`FtraceEvent` must include:

```proto
message FtraceEvent {
  uint64 timestamp = 1;
  int32 tgid = 2;
  string comm = 3;
  .SchedKthreadStopFormat sched_kthread_stop_format = 2400;
  .SchedKthreadStopRetFormat sched_kthread_stop_ret_format = 2401;
  .SchedMigrateTaskFormat sched_migrate_task_format = 2402;
  .SchedMoveNumaFormat sched_move_numa_format = 2403;
  .SchedPiSetprioFormat sched_pi_setprio_format = 2404;
  .SchedProcessExecFormat sched_process_exec_format = 2405;
  .SchedProcessExitFormat sched_process_exit_format = 2406;
  .SchedProcessForkFormat sched_process_fork_format = 2407;
  .SchedProcessFreeFormat sched_process_free_format = 2408;
  .SchedProcessWaitFormat sched_process_wait_format = 2409;
  .SchedStatBlockedFormat sched_stat_blocked_format = 2410;
  .SchedStatIowaitFormat sched_stat_iowait_format = 2411;
  .SchedStatRuntimeFormat sched_stat_runtime_format = 2412;
  .SchedStatSleepFormat sched_stat_sleep_format = 2413;
  .SchedStatWaitFormat sched_stat_wait_format = 2414;
  .SchedStickNumaFormat sched_stick_numa_format = 2415;
  .SchedSwapNumaFormat sched_swap_numa_format = 2416;
  .SchedSwitchFormat sched_switch_format = 2417;
  .SchedWaitTaskFormat sched_wait_task_format = 2418;
  .SchedWakeIdleWithoutIpiFormat sched_wake_idle_without_ipi_format = 2419;
  .SchedWakeupFormat sched_wakeup_format = 2420;
  .SchedWakeupNewFormat sched_wakeup_new_format = 2421;
  .SchedWakingFormat sched_waking_format = 2422;
  .SchedBlockedReasonFormat sched_blocked_reason_format = 4002;
}
```

- [ ] **Step 2: Generate row structs from sched.proto**

`build.rs` should parse `proto/ftrace_data/sched.proto` and generate `OUT_DIR/sched_rows.rs`. Each generated row struct derives `Serialize` and `Deserialize`, has a `TABLE_NAME` constant, and begins with:

```rust
event_timestamp: u64,
event_cpu: u32,
event_tgid: i32,
event_comm: String,
```

Then append the message fields listed in the spec. `hitrace.rs` should not hand-write direct sched event row structs; it should import the generated rows. `ThreadStateRow` uses nullable `dur` and `cpu`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ThreadStateRow {
    ts: u64,
    dur: Option<u64>,
    cpu: Option<u32>,
    tid: i32,
    state: String,
    comm: String,
}
```

`InstantRow` uses:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
struct InstantRow {
    ts: u64,
    name: String,
    r#ref: i32,
    wakeup_from: i32,
    ref_type: String,
    value: f64,
}
```

- [ ] **Step 3: Decode sched events**

Replace the single `sched_switch_rows` vector with a `SchedRows` struct containing one `Vec<_>` per table plus `thread_state` and `instant`. Check each direct sched message field on `FtraceEvent` and push rows.

- [ ] **Step 4: Register all tables**

`query.rs` should register `profiler_plugin_data`, every `sched_*` table, `thread_state`, and `instant`.

- [ ] **Step 5: Run datasource test**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_datasource_query query_extracts_sched_event_tables_and_derived_tables -- --exact
```

Expected: PASS.

---

### Task 4: CLI Coverage

**Files:**
- Modify: `crates/kat-rs-cli/tests/query_e2e.rs`

- [ ] **Step 1: Extend CLI fixture**

Add sched event metadata and at least one non-switch sched event to the CLI htrace fixture.

- [ ] **Step 2: Add CLI assertion**

Add a CLI test that queries:

```sql
select event_timestamp, event_cpu, pid, caller, io_wait from sched_blocked_reason
```

Expected JSON contains the row from the fixture.

- [ ] **Step 3: Run CLI tests**

Run:

```powershell
cargo test -p kat-rs-cli --test query_e2e
```

Expected: PASS.

---

### Task 5: Full Verification And PR

**Files:**
- All changed files.

- [ ] **Step 1: Run formatting**

Run:

```powershell
cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 2: Run workspace tests**

Run:

```powershell
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 3: Run clippy**

Run:

```powershell
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Run real trace queries**

Run:

```powershell
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from sched_switch'
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from sched_wakeup'
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from thread_state'
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from instant'
```

Expected: each command exits successfully and returns a JSON array with one numeric count.

- [ ] **Step 5: Commit, push branch, create PR**

Run:

```powershell
git add .
git commit -m "feat: expose sched event tables"
git push -u origin codex/sched-proto-issue-25
```

Create PR against `main` with title:

```text
feat: 接入 sched proto 并暴露 sched 表
```

PR body includes issue #25 checklist item, issue #22 derived table relationship, SQL table list, and all verification outputs.
