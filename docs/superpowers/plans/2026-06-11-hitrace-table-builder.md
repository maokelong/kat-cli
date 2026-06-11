# hitrace TableBuilder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 direct sched 明细表从 `Vec<Row>` 批量转换改为基于 `serde_arrow::ArrayBuilder` 的逐行构建。

**Architecture:** 新增通用 `TableBuilder<T>`，继续复用 serde_arrow 从 Row 类型推导 schema。`SchedRows` 只把 direct sched 字段类型替换为 `TableBuilder<Sched*Row>`，派生表仍在 `hitrace/derived.rs` 中使用现有行向量。

**Tech Stack:** Rust 2024, serde/serde_arrow, Arrow RecordBatch, DataFusion, Cargo integration tests.

---

## File Structure

- Modify: `crates/kat-rs-datasource/src/hitrace.rs`
  - 引入 `serde_arrow::ArrayBuilder` 和 `PhantomData`。
  - 新增 `TableBuilder<T>`。
  - 将 direct sched 字段从 `Vec<Sched*Row>` 改为 `TableBuilder<Sched*Row>`。
  - 将 direct sched `push` 改为 `TableBuilder::push(row)?`。
  - 保留 `record_batch_from` / `table_from_rows` 给 profiler rows 和派生表使用。
- Modify: `crates/kat-rs-datasource/tests/hitrace_architecture_contract.rs`
  - 增加 direct sched 表使用 streaming builder 的结构测试。
- No semantic change: `crates/kat-rs-datasource/src/hitrace/derived.rs`
  - 本计划不修改派生表算法。

---

### Task 1: Architecture Contract

**Files:**
- Modify: `crates/kat-rs-datasource/tests/hitrace_architecture_contract.rs`

- [x] **Step 1: Add failing architecture test**

Add this test:

```rust
#[test]
fn direct_sched_tables_use_streaming_table_builder() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");

    assert!(hitrace_rs.contains("struct TableBuilder<T>"));
    assert!(hitrace_rs.contains("sched_switch: TableBuilder<SchedSwitchRow>"));
    assert!(hitrace_rs.contains("sched_wakeup: TableBuilder<SchedWakeupRow>"));

    for marker in [
        "sched_switch: Vec<SchedSwitchRow>",
        "sched_wakeup: Vec<SchedWakeupRow>",
        "sched_blocked_reason: Vec<SchedBlockedReasonRow>",
    ] {
        assert!(
            !hitrace_rs.contains(marker),
            "{marker} should use TableBuilder instead of Vec<Row>"
        );
    }
}
```

- [x] **Step 2: Run failing test**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_architecture_contract direct_sched_tables_use_streaming_table_builder -- --exact
```

Expected: FAIL because `TableBuilder<T>` does not exist and `SchedRows` still contains `Vec<Sched*Row>`.

---

### Task 2: Implement TableBuilder

**Files:**
- Modify: `crates/kat-rs-datasource/src/hitrace.rs`

- [x] **Step 1: Add imports**

Add:

```rust
use std::{marker::PhantomData, path::Path};
use serde_arrow::{ArrayBuilder, schema::{SchemaLike, TracingOptions}};
```

Keep existing `serde::{Deserialize, Serialize}` and `arrow_array::RecordBatch`.

- [x] **Step 2: Add `TableBuilder<T>`**

Add near `table_from_rows`:

```rust
struct TableBuilder<T> {
    name: &'static str,
    builder: ArrayBuilder,
    _row: PhantomData<T>,
}

impl<T> TableBuilder<T>
where
    T: Serialize,
    for<'de> T: Deserialize<'de>,
{
    fn new(name: &'static str) -> Result<Self> {
        let fields = Vec::<arrow_schema::FieldRef>::from_type::<T>(TracingOptions::default())?;
        Ok(Self {
            name,
            builder: ArrayBuilder::from_arrow(&fields)?,
            _row: PhantomData,
        })
    }

    fn push(&mut self, row: T) -> Result<()> {
        self.builder.push(row)?;
        Ok(())
    }

    fn into_table(self) -> Result<HitraceTable> {
        let name = self.name;
        Ok(HitraceTable {
            name,
            batches: vec![
                self.builder
                    .into_record_batch()
                    .with_context(|| format!("failed to convert {name} table to Arrow"))?,
            ],
        })
    }
}
```

- [x] **Step 3: Change `SchedRows` construction**

Replace `#[derive(Default)] struct SchedRows` with a manual `fn new() -> Result<Self>` because each `TableBuilder` initialization is fallible.

Use:

```rust
impl SchedRows {
    fn new() -> Result<Self> {
        Ok(Self {
            sched_blocked_reason: TableBuilder::new(SchedBlockedReasonRow::TABLE_NAME)?,
            sched_kthread_stop: TableBuilder::new(SchedKthreadStopRow::TABLE_NAME)?,
            ...
            thread_state: ThreadStateBuilder::default(),
            instant: Vec::new(),
        })
    }
}
```

- [x] **Step 4: Update parser construction**

Change:

```rust
let mut sched_rows = SchedRows::default();
```

to:

```rust
let mut sched_rows = SchedRows::new()?;
```

- [x] **Step 5: Update `push_event`**

Change signature:

```rust
fn push_event(&mut self, cpu: u32, event: FtraceEvent) -> Result<()>
```

Replace each direct sched push with:

```rust
self.sched_switch.push(row)?;
```

Keep derived table pushes as they are. End with:

```rust
Ok(())
```

- [x] **Step 6: Update decode loop**

Change:

```rust
sched_rows.push_event(detail.cpu, event);
```

to:

```rust
sched_rows.push_event(detail.cpu, event)?;
```

- [x] **Step 7: Update `into_tables`**

Use `into_table()` for direct sched tables:

```rust
self.sched_switch.into_table()?
```

Keep derived tables:

```rust
table_from_rows(THREAD_STATE_TABLE, self.thread_state.into_rows())?
table_from_rows(INSTANT_TABLE, self.instant)?
```

---

### Task 3: Verify Behavior

**Files:**
- Existing tests only.

- [x] **Step 1: Run architecture contract**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_architecture_contract
```

Expected: PASS.

- [x] **Step 2: Run sched datasource test**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_datasource_query query_extracts_sched_event_tables_and_derived_tables -- --exact
```

Expected: PASS with the same JSON expectations as before.

- [x] **Step 3: Run workspace verification**

Run:

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all PASS.

- [x] **Step 4: Run real trace queries**

Run:

```powershell
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from sched_switch'
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from sched_wakeup'
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from thread_state'
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from instant'
```

Expected: each command exits successfully and returns one numeric `count`.

---

### Task 4: Commit And PR Update

**Files:**
- All changed files.

- [x] **Step 1: Review diff**

Run:

```powershell
git diff --stat
git diff -- crates\kat-rs-datasource\src\hitrace.rs crates\kat-rs-datasource\tests\hitrace_architecture_contract.rs
```

Expected: diff only covers the planned TableBuilder change, SDD, and plan.

- [x] **Step 2: Commit**

Run:

```powershell
git add crates\kat-rs-datasource\src\hitrace.rs crates\kat-rs-datasource\tests\hitrace_architecture_contract.rs docs\superpowers\specs\2026-06-11-hitrace-table-builder-design.md docs\superpowers\plans\2026-06-11-hitrace-table-builder.md
git commit -m "refactor: stream sched rows into arrow builders"
```

- [x] **Step 3: Push and update PR**

Run:

```powershell
git push
```

Update PR #26 to mention:

- direct sched 明细表现在使用 `serde_arrow::ArrayBuilder` 逐行构建；
- `thread_state` / `instant` 仍保留在 `hitrace/derived.rs`，语义修正留给后续。
