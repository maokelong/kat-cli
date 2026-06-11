# Event Family Generator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 sched 专用 build 生成器重构为 `EventFamilySpec` 驱动的 event family generator，保持 sched 生成输出和运行时行为不变。

**Architecture:** `build.rs` 新增 build-time `EventFamilySpec`，用 `SCHED_FAMILY` 配置当前 sched family。原 `generate_sched_code` / `render_sched_rows` / `render_sched_table_builders` 改为通用 `generate_event_family_code` / `render_event_rows` / `render_event_table_builders`，渲染函数通过 spec 中的名字生成 `SchedEventMeta`、`SchedEventObserver` 和 `SchedDirectTableBuilders`。

**Tech Stack:** Rust build script, prost-build, generated Rust code, Cargo integration tests.

---

## File Structure

- Modify: `crates/kat-rs-datasource/build.rs`
  - 新增 `EventFamilySpec` 和 `SCHED_FAMILY`。
  - 将 sched 专用生成函数改成 family 通用函数。
- Modify: `crates/kat-rs-datasource/tests/hitrace_architecture_contract.rs`
  - 增加 build.rs 结构约束测试。
- Add: `docs/superpowers/specs/2026-06-11-event-family-generator-design.md`
  - 记录 SDD。
- Add: `docs/superpowers/plans/2026-06-11-event-family-generator.md`
  - 记录实施计划和执行状态。

---

### Task 1: Failing Architecture Test

**Files:**
- Modify: `crates/kat-rs-datasource/tests/hitrace_architecture_contract.rs`

- [x] **Step 1: Add build generator architecture test**

Add:

```rust
#[test]
fn sched_generation_uses_event_family_generator() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let build_rs = fs::read_to_string(format!("{manifest_dir}/build.rs"))
        .expect("build script source can be read");

    for marker in [
        "struct EventFamilySpec",
        "const SCHED_FAMILY: EventFamilySpec",
        "generate_event_family_code(&SCHED_FAMILY)",
        "fn generate_event_family_code(family: &EventFamilySpec)",
        "fn render_event_rows(family: &EventFamilySpec, messages: &[ProtoMessage])",
        "fn render_event_table_builders(family: &EventFamilySpec, messages: &[ProtoMessage])",
    ] {
        assert!(build_rs.contains(marker), "{marker} should exist");
    }

    for marker in [
        "fn generate_sched_code",
        "fn render_sched_rows",
        "fn render_sched_table_builders",
    ] {
        assert!(!build_rs.contains(marker), "{marker} should be generalized");
    }
}
```

- [x] **Step 2: Run failing test**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_architecture_contract sched_generation_uses_event_family_generator -- --exact
```

Expected: FAIL because `EventFamilySpec` does not exist yet and sched-specific function names still exist.

---

### Task 2: Refactor Build Generator

**Files:**
- Modify: `crates/kat-rs-datasource/build.rs`

- [x] **Step 1: Add family spec**

Add near `SCHED_PROTO`:

```rust
const SCHED_FAMILY: EventFamilySpec = EventFamilySpec {
    proto_path: "proto/ftrace_data/sched.proto",
    rows_file: "sched_rows.rs",
    builders_file: "sched_table_builders.rs",
    meta_name: "SchedEventMeta",
    observer_name: "SchedEventObserver",
    builders_name: "SchedDirectTableBuilders",
};

struct EventFamilySpec {
    proto_path: &'static str,
    rows_file: &'static str,
    builders_file: &'static str,
    meta_name: &'static str,
    observer_name: &'static str,
    builders_name: &'static str,
}
```

- [x] **Step 2: Use family spec in main**

Change:

```rust
const SCHED_PROTO: &str = "proto/ftrace_data/sched.proto";
let proto_files = ["proto/hitrace.proto", SCHED_PROTO];
generate_sched_code().expect("sched generated code is written");
```

to:

```rust
let proto_files = ["proto/hitrace.proto", SCHED_FAMILY.proto_path];
generate_event_family_code(&SCHED_FAMILY).expect("event family generated code is written");
```

- [x] **Step 3: Generalize generation function**

Rename and update:

```rust
fn generate_event_family_code(family: &EventFamilySpec) -> std::io::Result<()> {
    let source = fs::read_to_string(family.proto_path)?;
    let messages = parse_proto_messages(&source);
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    fs::write(out_dir.join(family.rows_file), render_event_rows(family, &messages))?;
    fs::write(
        out_dir.join(family.builders_file),
        render_event_table_builders(family, &messages),
    )
}
```

- [x] **Step 4: Generalize row rendering**

Rename `render_sched_rows` to:

```rust
fn render_event_rows(family: &EventFamilySpec, messages: &[ProtoMessage]) -> String
```

Use `family.meta_name` instead of hard-coded `SchedEventMeta` in generated code.

- [x] **Step 5: Generalize table builder rendering**

Rename `render_sched_table_builders` to:

```rust
fn render_event_table_builders(family: &EventFamilySpec, messages: &[ProtoMessage]) -> String
```

Use `family.observer_name` and `family.builders_name` instead of hard-coded `SchedEventObserver` and `SchedDirectTableBuilders`.

- [x] **Step 6: Run focused architecture test**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_architecture_contract sched_generation_uses_event_family_generator -- --exact
```

Expected: PASS.

---

### Task 3: Verify Generated Behavior

**Files:**
- Existing tests only.

- [x] **Step 1: Run generated code contracts**

Run:

```powershell
cargo test -p kat-rs-datasource --test proto_contract
```

Expected: PASS, 6 tests.

- [x] **Step 2: Run sched datasource behavior**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_datasource_query query_extracts_sched_event_tables_and_derived_tables -- --exact
```

Expected: PASS.

- [x] **Step 3: Run full verification**

Run:

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all PASS.

- [x] **Step 4: Run real trace counts**

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
git diff -- crates\kat-rs-datasource\build.rs crates\kat-rs-datasource\tests\hitrace_architecture_contract.rs
```

Expected: diff only generalizes build generator and adds tests/docs.

- [x] **Step 2: Commit and push**

Run:

```powershell
git add crates\kat-rs-datasource\build.rs crates\kat-rs-datasource\tests\hitrace_architecture_contract.rs docs\superpowers\specs\2026-06-11-event-family-generator-design.md docs\superpowers\plans\2026-06-11-event-family-generator.md
git commit -m "refactor: generate sched through event family"
git push
```

- [x] **Step 3: Update PR body**

Update PR #26 to mention:

- sched now uses build-time `EventFamilySpec`.
- generated output and SQL behavior are unchanged.
