# sched.proto Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 接入上游 `types/plugins/ftrace_data/sched.proto`，让现有 `sched_switch` 查询路径使用真实 sched proto 生成类型。

**Architecture:** 保留当前 hitrace 外层解析和 `sched_switch` SQL 表，只把 sched 事件消息从手写结构替换为上游 `sched.proto`。no-package 的上游 sched 类型在 `proto` 模块根部生成，`kat.hitrace` 生成代码放在 `proto::kat::hitrace` 下，并在 `proto` 根部 re-export 现有调用方需要的类型。

**Tech Stack:** Rust 2024, prost/prost-build, serde/serde_arrow, DataFusion, Cargo integration tests.

---

## File Structure

- Create: `crates/kat-rs-datasource/proto/ftrace_data/sched.proto`
  - 从 `D:\项目\trace_streamer\src\protos\types\plugins\ftrace_data\sched.proto` 原样复制。
- Modify: `crates/kat-rs-datasource/proto/hitrace.proto`
  - 增加 `option optimize_for = LITE_RUNTIME;`。
  - import `ftrace_data/sched.proto`。
  - 删除本地手写 `SchedSwitchFormat`，让 `FtraceEvent` 使用 `.SchedSwitchFormat`。
- Modify: `crates/kat-rs-datasource/build.rs`
  - 编译 `proto/hitrace.proto` 和 `proto/ftrace_data/sched.proto`。
  - 为 `.SchedSwitchFormat` 派生 serde。
  - 增加 sched proto 的 rerun 规则。
- Modify: `crates/kat-rs-datasource/src/lib.rs`
  - include no-package 生成文件 `_.rs`。
  - 将 `kat.hitrace.rs` 包在 `kat::hitrace` 模块下。
  - re-export `ProfilerPluginData`、`TracePluginResult`，让现有调用方继续使用 `crate::proto::...`。
- Modify: `crates/kat-rs-datasource/tests/proto_contract.rs`
  - 新增 `SchedBlockedReasonFormat` round-trip，证明 sched.proto 全文件参与生成。
  - 保持 `SchedSwitchFormat` 字段契约。

---

### Task 1: Write Failing Proto Contract

**Files:**
- Modify: `crates/kat-rs-datasource/tests/proto_contract.rs`

- [ ] **Step 1: Add the failing sched proto test**

Replace `crates/kat-rs-datasource/tests/proto_contract.rs` with:

```rust
use prost::Message;

#[allow(dead_code)]
mod proto {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));

    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }
    }

    pub use kat::hitrace::{ProfilerPluginData, TracePluginResult};
}

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
    assert_eq!(decoded.prev_pid, 42);
    assert_eq!(decoded.prev_prio, 120);
    assert_eq!(decoded.prev_state, 1);
    assert_eq!(decoded.next_comm, "main");
    assert_eq!(decoded.next_pid, 7);
    assert_eq!(decoded.next_prio, 100);
}

#[test]
fn generated_proto_includes_upstream_sched_messages() {
    let value = proto::SchedBlockedReasonFormat {
        pid: 42,
        caller: 0xfeed_beef,
        io_wait: 1,
    };

    let decoded = proto::SchedBlockedReasonFormat::decode(value.encode_to_vec().as_slice())
        .expect("decode");

    assert_eq!(decoded.pid, 42);
    assert_eq!(decoded.caller, 0xfeed_beef);
    assert_eq!(decoded.io_wait, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p kat-rs-datasource --test proto_contract
```

Expected: FAIL because `OUT_DIR/_.rs` does not exist yet and sched messages are not generated from upstream `sched.proto`.

---

### Task 2: Wire Upstream sched.proto Into prost

**Files:**
- Create: `crates/kat-rs-datasource/proto/ftrace_data/sched.proto`
- Modify: `crates/kat-rs-datasource/proto/hitrace.proto`
- Modify: `crates/kat-rs-datasource/build.rs`
- Modify: `crates/kat-rs-datasource/src/lib.rs`

- [ ] **Step 1: Copy the upstream sched proto**

Run:

```powershell
New-Item -ItemType Directory -Force crates\kat-rs-datasource\proto\ftrace_data
Copy-Item -LiteralPath 'D:\项目\trace_streamer\src\protos\types\plugins\ftrace_data\sched.proto' -Destination crates\kat-rs-datasource\proto\ftrace_data\sched.proto
```

Expected: `crates/kat-rs-datasource/proto/ftrace_data/sched.proto` exists and contains `message SchedBlockedReasonFormat` and `message SchedSwitchFormat`.

- [ ] **Step 2: Update hitrace.proto**

Set `crates/kat-rs-datasource/proto/hitrace.proto` to:

```proto
syntax = "proto3";

package kat.hitrace;

option optimize_for = LITE_RUNTIME;

import "ftrace_data/sched.proto";

message ProfilerPluginData {
  string name = 1;
  uint32 status = 2;
  bytes data = 3;
  int32 clock_id = 4;
  uint64 tv_sec = 5;
  uint64 tv_nsec = 6;
  string version = 7;
  uint32 sample_interval = 8;
}

message TracePluginResult {
  repeated FtraceCpuDetailMsg ftrace_cpu_detail = 2;
}

message FtraceCpuDetailMsg {
  uint32 cpu = 1;
  repeated FtraceEvent event = 2;
  uint64 overwrite = 3;
}

message FtraceEvent {
  .SchedSwitchFormat sched_switch_format = 2417;
}
```

- [ ] **Step 3: Update build.rs**

Set `crates/kat-rs-datasource/build.rs` to:

```rust
fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    let proto_files = ["proto/hitrace.proto", "proto/ftrace_data/sched.proto"];
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config.type_attribute(
        ".kat.hitrace.ProfilerPluginData",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );
    config.type_attribute(
        ".SchedSwitchFormat",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );
    config.field_attribute(
        ".kat.hitrace.ProfilerPluginData.data",
        "#[serde(with = \"serde_bytes\")]",
    );
    config
        .compile_protos(&proto_files, &["proto"])
        .expect("hitrace and sched protos compile");

    for proto_file in proto_files {
        println!("cargo:rerun-if-changed={proto_file}");
    }
}
```

- [ ] **Step 4: Update generated module include**

Set `crates/kat-rs-datasource/src/lib.rs` to:

```rust
mod hitrace;
mod json;
mod mmap;
mod query;

pub use query::TraceDatasource;

pub(crate) mod proto {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));

    pub(crate) mod kat {
        pub(crate) mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }
    }

    pub(crate) use kat::hitrace::{ProfilerPluginData, TracePluginResult};
}
```

- [ ] **Step 5: Run proto contract test to verify it passes**

Run:

```powershell
cargo test -p kat-rs-datasource --test proto_contract
```

Expected: PASS, including `generated_proto_includes_upstream_sched_messages`.

---

### Task 3: Verify Existing sched_switch Query Path

**Files:**
- No production file changes expected.

- [ ] **Step 1: Run datasource sched_switch test**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_datasource_query query_extracts_sched_switch_from_ftrace_plugin_result -- --exact
```

Expected: PASS. The JSON row still contains `RenderThread`, `42`, `com.tencent.mm`, and `100`.

- [ ] **Step 2: Run CLI sched_switch test**

Run:

```powershell
cargo test -p kat-rs-cli --test query_e2e query_prints_sched_switch_fields -- --exact
```

Expected: PASS. CLI output remains the current `sched_switch` JSON shape.

---

### Task 4: Run Full Verification

**Files:**
- No production file changes expected.

- [ ] **Step 1: Run workspace tests**

Run:

```powershell
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 2: Run clippy**

Run:

```powershell
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS with no warnings.

- [ ] **Step 3: Run real trace query**

Run:

```powershell
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from sched_switch'
```

Expected: command exits successfully and prints a JSON array containing one object with numeric `count`.

---

### Task 5: Commit, Push Branch, and Create PR

**Files:**
- Commit all tracked implementation files and docs.

- [ ] **Step 1: Inspect final diff**

Run:

```powershell
git status --short --branch
git diff --check
git diff --stat origin/main...HEAD
```

Expected: branch is `codex/sched-proto-issue-25`; diff contains only docs, datasource proto/build/module/test changes.

- [ ] **Step 2: Commit implementation**

Run:

```powershell
git add docs/superpowers/plans/2026-06-10-sched-proto.md crates/kat-rs-datasource/proto/ftrace_data/sched.proto crates/kat-rs-datasource/proto/hitrace.proto crates/kat-rs-datasource/build.rs crates/kat-rs-datasource/src/lib.rs crates/kat-rs-datasource/tests/proto_contract.rs
git commit -m "feat: use upstream sched proto"
```

Expected: commit succeeds on `codex/sched-proto-issue-25`.

- [ ] **Step 3: Push working branch**

Run:

```powershell
git push -u origin codex/sched-proto-issue-25
```

Expected: branch is pushed; `main` is not pushed.

- [ ] **Step 4: Create PR**

Use GitHub API or `gh` if available to create a PR:

```text
base: main
head: codex/sched-proto-issue-25
title: feat: 接入上游 sched.proto
```

PR body must include:

```markdown
Closes #25

## 本次完成的 checklist 项

- [x] sched.proto

## 新增或修改的 SQL 表

- 无新增表；`sched_switch` 继续使用原字段。

## 验证

- `cargo test -p kat-rs-datasource --test proto_contract`
- `cargo test -p kat-rs-datasource --test hitrace_datasource_query query_extracts_sched_switch_from_ftrace_plugin_result -- --exact`
- `cargo test -p kat-rs-cli --test query_e2e query_prints_sched_switch_fields -- --exact`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- 真实 trace: `select count(*) as count from sched_switch`
```

Expected: PR targets `main` from `codex/sched-proto-issue-25`.
