# Task 1 Report: SQLite Five-Table Dataset Materializer

## 概述

完成 `kat-rs-datasource` 内的 SQLite pack demo 数据集物化能力，新增
`materialize_sqlite_pack_demo_dataset(sqlite_path, dataset_path)`，将 SQLite 五张源表读取为
Arrow `RecordBatch`，写入 Parquet，并通过现有 dataset catalog / DataFusion 注册链路做落盘后校验。

## 变更范围

- `Cargo.toml`
- `Cargo.lock`
- `crates/kat-rs-datasource/Cargo.toml`
- `crates/kat-rs-datasource/src/formats/mod.rs`
- `crates/kat-rs-datasource/src/formats/sqlite.rs`
- `crates/kat-rs-datasource/src/materializer.rs`
- `crates/kat-rs-datasource/src/lib.rs`
- `crates/kat-rs-datasource/tests/dataset_contract.rs`

## TDD 证据

### RED

先追加 brief 指定的 3 个 datasource 契约测试，再运行：

```powershell
cargo test -p kat-rs-datasource --test dataset_contract sqlite_pack_demo_materializer -- --nocapture
```

失败结果符合预期：

- `cannot find function materialize_sqlite_pack_demo_dataset in crate kat_rs_datasource`
- 共 3 处调用点报同一缺失入口错误

这证明新增测试确实覆盖了新接口，而不是误测到已有行为。

### GREEN

补齐最小生产代码后重新运行：

```powershell
cargo test -p kat-rs-datasource --test dataset_contract sqlite_pack_demo_materializer -- --nocapture
cargo test -p kat-rs-datasource --test dataset_contract materialized_catalog_records_source_table_kind -- --nocapture
```

结果：

- `sqlite_pack_demo_materializer_*`: 3 passed
- `materialized_catalog_records_source_table_kind`: 1 passed

## 实现说明

1. 在 workspace 与 `kat-rs-datasource` 中引入 `rusqlite`，并按 brief 补入 `serde_yaml_ng`、`sha2`。
2. 新增 `formats/sqlite.rs`：
   - 校验 5 张必需表是否存在；
   - 读取 SQLite schema，映射到 Arrow `Int64` / `Float64` / `Utf8` / `Binary`；
   - 分批构建 `RecordBatch`；
   - 为 `instant` 额外暴露 `rowid: Int64`。
3. 在 `materializer.rs` 新增 SQLite dataset materializer，沿用现有 `DatasetWriter` 写 Parquet 与 catalog。
4. 在 `lib.rs` 暴露新入口。
5. 在 `dataset_contract.rs` 追加 brief 指定的 3 个测试与 SQLite fixture。

## 自审

- 只改了任务要求文件与 `Cargo.lock`，未碰用户点名禁止的未跟踪目录和文档。
- 复核了 catalog 断言，确认新物化结果仍写入 `kind = "source"`。
- 复核了 `instant.rowid` 为 `Int64` 且可直接查询。

## 备注

`instant.rowid` 仍按 `Int64` 显式物化，供后续 pack 执行直接查询。

## Fix: preserve sqlite thread rows

review 前修正了一个 MVP 偏差：SQLite materializer 不应过滤 `thread.is_main_thread = 1`，而应完整复制五张源表，保留非主线程行供后续 wakeup source 等 pack 使用。

### 本次 RED

先修改 `crates/kat-rs-datasource/tests/dataset_contract.rs`，新增：

- `select itid, name, is_main_thread from thread order by itid` 必须返回 fixture 的两条 `thread` 行；
- 原有 `process` / `thread` 联接断言收紧为 `and t.is_main_thread = 1`，只表达主线程联接仍可命中，不再暗含源表被过滤。

随后运行：

```powershell
cargo test -p kat-rs-datasource --test dataset_contract sqlite_pack_demo_materializer -- --nocapture
```

失败证据：`sqlite_pack_demo_materializer_writes_only_five_source_tables` 只查到一条 `thread` 行，缺少 `itid = 440` 的非主线程 fixture 数据。

### 本次 GREEN

最小修复位于 `crates/kat-rs-datasource/src/formats/sqlite.rs`：删除 `thread where is_main_thread = 1` 特判，恢复为与其余源表一致的全表读取。

验证命令：

```powershell
cargo test -p kat-rs-datasource --test dataset_contract sqlite_pack_demo_materializer -- --nocapture
cargo test -p kat-rs-datasource --test dataset_contract materialized_catalog_records_source_table_kind -- --nocapture
```

验证结果：

- `sqlite_pack_demo_materializer_*`: 3 passed
- `materialized_catalog_records_source_table_kind`: 1 passed
