# kat-rs datasource / cli 轻量设计

## 要解决的问题

当前切片只需要打通一条可验证链路：读取最小 `.htrace` profiler 文件，将外层 `ProfilerPluginData` protobuf segment 转成 Arrow batch，并在识别到 `sched_switch` payload 时解出 `SchedSwitchFormat`，注册到 DataFusion，通过 CLI 用 SQL 查询 JSON 结果。

## 不做什么

1. 不解析除 `sched_switch` 之外的 profiler plugin payload 内层业务数据。
2. 不支持 hitrace 之外的数据源。
3. 不增加 session、缓存、多 datasource 管理或交互式状态。
4. 不使用运行时 protobuf descriptor 或 dynamic message。
5. 不提交大体积真实 trace fixture。

## 最小交付

1. `kat-rs-datasource` 提供 `TraceDatasource::from_hitrace(path)` 和 `query_json(sql)`。
2. `kat-rs-cli` 提供 `kat-rs query --source hitrace --file <path> --sql <sql>`。
3. `kat-rs-arrow-derive` 给 prost 生成的 `ProfilerPluginData` 直接生成 `record_batch_from(rows)`，调用点保持：

```rust
let batch = ProfilerPluginData::record_batch_from(messages)?;
```

## 数据流

```text
kat-rs-cli
  -> TraceDatasource::from_hitrace(path)
  -> mmap .htrace
  -> decode length-prefixed ProfilerPluginData
  -> ProfilerPluginData::record_batch_from(messages)
  -> decode sched_switch payload as SchedSwitchFormat
  -> DataFusion MemTable
  -> query_json(sql)
```

## hitrace 契约

当前暴露 `profiler_plugin_data` 和 `sched_switch` 两张表。

`profiler_plugin_data` 字段来自最小 `ProfilerPluginData` proto：

| 字段 | 类型 |
| --- | --- |
| `name` | `string` |
| `status` | `uint32` |
| `data` | `bytes` |
| `clock_id` | `int32` |
| `tv_sec` | `uint64` |
| `tv_nsec` | `uint64` |
| `version` | `string` |
| `sample_interval` | `uint32` |

`data` 查询结果用十六进制字符串输出。

`sched_switch` 字段来自 `SchedSwitchFormat` proto：

| 字段 | 类型 |
| --- | --- |
| `prev_comm` | `string` |
| `prev_pid` | `int32` |
| `prev_prio` | `int32` |
| `prev_state` | `uint64` |
| `next_comm` | `string` |
| `next_pid` | `int32` |
| `next_prio` | `int32` |

## 验证

1. datasource 测试构造最小 `.htrace`，验证 SQL 查询 JSON、二进制列输出和 build 后文件句柄释放。
2. datasource 测试拒绝缺少 `OHOSPROF` header 的 length-prefixed protobuf 输入。
3. CLI 测试验证 query 命令输出 JSON、缺少参数会失败、未知 source 会失败、help 包含必要参数。
4. `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings` 通过。
