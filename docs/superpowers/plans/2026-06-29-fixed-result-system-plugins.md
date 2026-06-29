# Fixed Result System Plugins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 覆盖 issue #53 中 6 个系统资源 fixed result profiler plugins，从 upstream proto 到 domain decode 再到 Arrow/SQL direct tables。

**Architecture:** 新增 `fixed_result` domain，处理普通 config/result protobuf payload。构建期用静态 `FixedResultPluginSpec` 生成 domain record/decoder 和 Arrow table set，descriptor 只用于补齐 serde derive 的 message 路径，不引入运行时反射或通用 schema framework。

**Tech Stack:** Rust, prost/prost-build, serde/serde_arrow, Arrow/DataFusion, synthetic `.htrace` datasource tests.

---

## File Structure

- Create `crates/kat-rs-datasource/proto/{cpu_data,memory_data,process_data,diskio_data,network_data,gpu_data}/...`
  - 迁入 upstream proto，添加项目 package，保留 message/field/tag。
- Create `crates/kat-rs-datasource/build/fixed_result_domain_codegen.rs`
  - 定义 fixed result plugin spec、proto files、serde derive message paths、生成 records/decoders。
- Create `crates/kat-rs-datasource/build/fixed_result_arrow_codegen.rs`
  - 生成 `FixedResultTableSet`。
- Create `crates/kat-rs-datasource/src/domains/fixed_result/mod.rs`
  - include generated records/decoders。
- Modify `crates/kat-rs-datasource/build.rs`
  - 接入 fixed result proto、serde derive、records/table builders generation。
- Modify `crates/kat-rs-datasource/src/lib.rs`
  - include generated packages and `fixed_result_table_builders`。
- Modify `crates/kat-rs-datasource/src/domains/mod.rs`
  - expose `fixed_result` domain。
- Modify `crates/kat-rs-datasource/src/formats/hitrace/mod.rs`
  - assemble fixed result decoder specs into profiler registry。
- Modify `crates/kat-rs-datasource/src/record.rs`
  - add coarse `TraceRecord::FixedResult`。
- Modify `crates/kat-rs-datasource/src/sinks/arrow/mod.rs`
  - route `FixedResult` records to generated table set。
- Modify `crates/kat-rs-datasource/tests/proto_contract.rs`
  - add proto/domain contract tests。
- Modify `crates/kat-rs-datasource/tests/hitrace_architecture_contract.rs`
  - guard layering and codegen split。
- Modify `crates/kat-rs-datasource/tests/hitrace_datasource_query.rs`
  - add synthetic `.htrace` fixed result query coverage。

### Task 1: Proto Contract RED

**Files:**
- Modify: `crates/kat-rs-datasource/tests/proto_contract.rs`

- [ ] **Step 1: Add failing proto/domain expectations**

Add generated module includes for `kat.cpu_data`, `kat.memory_data`, `kat.process_data`, `kat.diskio_data`, `kat.network_data`, `kat.gpu_data`; add a test `generated_proto_includes_fixed_result_system_plugins` that constructs and decodes:

```rust
proto::kat::cpu_data::CpuData { process_num: 2, user_load: 1.5, ..Default::default() }
proto::kat::memory_data::MemoryData { zram: 64, gpu_used_size: 32, ..Default::default() }
proto::kat::process_data::ProcessData { processesinfo: vec![...] }
proto::kat::diskio_data::DiskioData { rd_sectors_kb: 10, wr_sectors_kb: 20, ..Default::default() }
proto::kat::network_data::NetworkDatas { networkinfo: vec![...] , ..Default::default() }
proto::kat::gpu_data::GpuData { boottime: 100, gpu_utilisation: 80, ..Default::default() }
```

Add `TraceRecord::FixedResult` matching with one config and one result record.

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test -p kat-rs-datasource --test proto_contract -- --nocapture
```

Expected: compile fails because fixed result proto modules, domain module, record variant and generated records do not exist.

### Task 2: Query Contract RED

**Files:**
- Modify: `crates/kat-rs-datasource/tests/hitrace_datasource_query.rs`

- [ ] **Step 1: Add failing synthetic fixed-result query test**

Add lightweight prost test messages for the 6 config/result roots and helper functions producing:

```text
cpu-plugin_config, cpu-plugin
memory-plugin_config, memory-plugin
process-plugin_config, process-plugin
diskio-plugin_config, diskio-plugin
network-plugin_config, network-plugin
gpu-plugin_config, gpu-plugin
```

Add test `query_extracts_fixed_result_system_plugin_direct_tables` that asserts representative SQL results:

```sql
select pid, report_process_info from cpu_config
select process_num, user_load, total_load from cpu_data
select report_sysmem_mem_info from memory_config
select zram, gpu_used_size from memory_data
select report_process_tree, report_cpu from process_config
select processesinfo from process_data
select report_io_stats from diskio_config
select rd_sectors_kb, wr_sectors_kb from diskio_data
select single_pid, startup_process_name from network_config
select networkinfo from network_data
select pid, report_gpu_info from gpu_config
select boottime, gpu_utilisation from gpu_data
```

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_datasource_query -- --nocapture
```

Expected: fails because fixed result direct tables do not exist.

### Task 3: Implement Proto + Build Codegen

**Files:**
- Create: `crates/kat-rs-datasource/proto/cpu_data/cpu_plugin_config.proto`
- Create: `crates/kat-rs-datasource/proto/cpu_data/cpu_plugin_result.proto`
- Create: `crates/kat-rs-datasource/proto/memory_data/memory_plugin_common.proto`
- Create: `crates/kat-rs-datasource/proto/memory_data/memory_plugin_config.proto`
- Create: `crates/kat-rs-datasource/proto/memory_data/memory_plugin_result.proto`
- Create: `crates/kat-rs-datasource/proto/process_data/process_plugin_config.proto`
- Create: `crates/kat-rs-datasource/proto/process_data/process_plugin_result.proto`
- Create: `crates/kat-rs-datasource/proto/diskio_data/diskio_plugin_config.proto`
- Create: `crates/kat-rs-datasource/proto/diskio_data/diskio_plugin_result.proto`
- Create: `crates/kat-rs-datasource/proto/network_data/network_plugin_config.proto`
- Create: `crates/kat-rs-datasource/proto/network_data/network_plugin_result.proto`
- Create: `crates/kat-rs-datasource/proto/gpu_data/gpu_plugin_config.proto`
- Create: `crates/kat-rs-datasource/proto/gpu_data/gpu_plugin_result.proto`
- Create: `crates/kat-rs-datasource/build/fixed_result_domain_codegen.rs`
- Create: `crates/kat-rs-datasource/build/fixed_result_arrow_codegen.rs`
- Modify: `crates/kat-rs-datasource/build.rs`

- [ ] **Step 1: Add upstream proto files**

Copy the upstream message definitions from `developtools_profiler/protos/types/plugins/<plugin>/`, add project packages:

```proto
package kat.cpu_data;
package kat.memory_data;
package kat.process_data;
package kat.diskio_data;
package kat.network_data;
package kat.gpu_data;
```

Change memory imports to:

```proto
import "memory_data/memory_plugin_common.proto";
```

- [ ] **Step 2: Add fixed result domain codegen helper**

Implement static specs:

```rust
pub(crate) const FIXED_RESULT_PLUGIN_SPECS: &[FixedResultPluginSpec] = &[
    FixedResultPluginSpec { plugin_name: "cpu-plugin", package: "cpu_data", table_prefix: "cpu", config_message: "CpuConfig", result_message: "CpuData", ... },
    ...
];
```

Generate `fixed_result_records.rs` with `FixedResultRecord`, decoder specs, and per-plugin `PluginDecoder` impls.

- [ ] **Step 3: Add fixed result Arrow codegen helper**

Generate `fixed_result_table_builders.rs` with `FixedResultTableSet`, one `MessageTableBuilder<T>` per config/result table, and `push_record`, `into_tables`, `flush_tables`.

- [ ] **Step 4: Wire build.rs**

Include the new build modules, add fixed result proto files to `proto_files`, derive serde for all fixed-result messages including nested messages, compile FDS, and generate records/table builders.

### Task 4: Implement Runtime Wiring

**Files:**
- Create: `crates/kat-rs-datasource/src/domains/fixed_result/mod.rs`
- Modify: `crates/kat-rs-datasource/src/domains/mod.rs`
- Modify: `crates/kat-rs-datasource/src/lib.rs`
- Modify: `crates/kat-rs-datasource/src/formats/hitrace/mod.rs`
- Modify: `crates/kat-rs-datasource/src/record.rs`
- Modify: `crates/kat-rs-datasource/src/sinks/arrow/mod.rs`

- [ ] **Step 1: Include generated fixed result modules**

Add generated `fixed_result_table_builders` in `lib.rs`, package includes for 6 generated proto packages, and `domains::fixed_result`.

- [ ] **Step 2: Add coarse TraceRecord variant**

Add:

```rust
FixedResult(Box<FixedResultRecord>),
```

- [ ] **Step 3: Register fixed result decoders**

Extend the hitrace pipeline decoder specs with `FIXED_RESULT_PLUGIN_DECODERS`.

- [ ] **Step 4: Add fixed result Arrow table set**

Add `FixedResultTableSet` to `ArrowSink`, route `TraceRecord::FixedResult`.

- [ ] **Step 5: Run GREEN for proto contract**

Run:

```powershell
cargo test -p kat-rs-datasource --test proto_contract -- --nocapture
```

Expected: proto/domain contract passes.

### Task 5: Finish Query Coverage + Architecture Guards

**Files:**
- Modify: `crates/kat-rs-datasource/tests/hitrace_datasource_query.rs`
- Modify: `crates/kat-rs-datasource/tests/hitrace_architecture_contract.rs`

- [ ] **Step 1: Adjust query expectations to actual Arrow JSON**

If nested/list JSON shape differs from the initial expected value, update only the expected JSON to match Arrow's direct projection, not the production code.

- [ ] **Step 2: Add architecture guards**

Assert:

```rust
src/domains/fixed_result/mod.rs exists
build/fixed_result_domain_codegen.rs exists
build/fixed_result_arrow_codegen.rs exists
formats/hitrace/profiler does not contain fixed result message names
fixed result generated table set contains cpu_data, memory_data, process_data, diskio_data, network_data, gpu_data
```

- [ ] **Step 3: Run focused GREEN**

Run:

```powershell
cargo test -p kat-rs-datasource --test hitrace_architecture_contract -- --nocapture
cargo test -p kat-rs-datasource --test hitrace_datasource_query -- --nocapture
```

Expected: both pass.

### Task 6: Full Verification

**Files:**
- No new files expected.

- [ ] **Step 1: Format**

Run:

```powershell
cargo fmt --all -- --check
```

Expected: exit 0.

- [ ] **Step 2: Full tests**

Run:

```powershell
cargo test --workspace
```

Expected: exit 0.

- [ ] **Step 3: Clippy**

Run:

```powershell
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: exit 0.

- [ ] **Step 4: Diff hygiene**

Run:

```powershell
git diff --check
git status --short --branch
```

Expected: no whitespace errors; branch contains only issue #53 files.

## Self-Review

- Spec coverage: Tasks cover upstream proto paths, prost generation, payload shape/root message, domain decode, Arrow direct tables, and tests.
- Placeholder scan: No TBD/TODO/future implementation placeholders.
- Type consistency: The plan consistently uses `FixedResultRecord`, `FixedResultTableSet`, `FIXED_RESULT_PLUGIN_DECODERS`, and 12 table names from the spec.
