# Memory Analysis PACK

这个 External PACK 展示如何把独立 Rust 转换器接入 PACK 自有 Provider：

```text
UTF-8 text Ftrace
  -> FtraceProvider(...)
  -> typed Ftrace relations
  -> Provider.query(SQL) 返回 eager dp.Table
  -> Workflow 发布 Run Output
```

Provider 构造成功后即可查询；调用方不需要知道转换器、Catalog 路径或物理存储格式。
实现内部由 Rust `ftrace2parquet` 拥有文本语法、Proto 类型合同、有限批次写入和原子发布，
Python Provider 负责进程调用、临时 Catalog 生命周期、必需关系校验和本地查询。部署环境
通过 `KAT_FTRACE2PARQUET_EXECUTABLE` 配置批准的转换器，Workflow 不感知其位置。

固定关系为 `text_ftrace_header`、`text_ftrace_event_occurrence` 和
`text_ftrace_event`。四种首批类型化载荷关系只在来源实际出现时生成：

- `text_ftrace_event_sched_switch`
- `text_ftrace_event_sched_wakeup`
- `text_ftrace_event_sched_wakeup_new`
- `text_ftrace_event_tracing_mark_write`

合法但未支持的事件只占用 `source_event_sequence`，不产生无类型 raw payload。事件根
显式保存 `clock_domain + clock_value`；调用方必须根据采集配置提供 domain，Provider
不从文件名或数值猜测时间语义。

完整的表、字段、关联关系和查询示例见 Provider 的同名文档
[datasources/ftrace.md](./datasources/ftrace.md)。运行时也可以使用
`SHOW TABLES` 和 `DESCRIBE <table>` 查看当前 Trace 实际产生的关系及其物理字段。

## 构建和运行

```bash
cargo build --locked -p ftrace2parquet

export KAT_FTRACE2PARQUET_EXECUTABLE="$PWD/target/debug/ftrace2parquet"

kat run \
  --pack mem-pack \
  --workflow summarize-ftrace \
  --pack-dir ./examples/packs/mem-pack \
  -- \
  --trace-path /absolute/path/to/trace.ftrace \
  --clock-domain monotonic
```

Provider 在 `ctx.datasource_root` 下创建并管理本次执行唯一的临时 workspace。查询结果是
eager `dp.Table`；内部物理存储清理后不影响 Run Output。

## 验证

Rust 合同测试负责解析、类型化 oneof 关系、来源序号间隙、坏输入、有限批次和原子发布：

```bash
cargo test --locked -p ftrace2parquet
```

PACK pytest 负责 Provider 进程边界、Catalog、查询拓扑、失败清理和 Workflow Output：

```bash
kat test --pack-dir ./examples/packs/mem-pack
```
