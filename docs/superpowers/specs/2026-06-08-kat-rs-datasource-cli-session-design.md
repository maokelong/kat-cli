# kat-rs datasource / session / cli 基础架构设计

## 背景

kat-rs 需要重新从目标库 `main` 的干净状态开始建立最小可用架构。当前目标不是一次性覆盖所有日志格式，而是先把核心边界搭好：`datasource` 负责解析和查询，`session` 负责持有运行期状态，`cli` 负责提供二进制命令入口。

## 目标

1. 新增 `kat-rs-datasource` crate。
2. 新增 `kat-rs-session` crate。
3. 新增 `kat-rs-cli` crate，并作为 `kat-rs` 二进制入口。
4. 当前仅支持 `hitrace` 数据源。
5. `datasource` 接收数据源类型和文件路径，`build` 完成后只暴露基于 SQL 字符串的查询能力。
6. SQL 查询结果返回 JSON。
7. hitrace 文件是 protobuf 序列化后的二进制文件。
8. 解析期间使用 `prost` 生成的 Rust struct，并在构建期基于 struct AST 生成 Arrow 表构建代码，运行时将 protobuf 数据转换为 Arrow `RecordBatch`，再注册到 DataFusion。
9. 文件读取使用内存映射，解析完成后文件句柄和 mmap 都释放。
10. 维测日志使用 `log`，不使用 `print` / `println` 输出日志。

## 非目标

1. 不接入真实华为 hitrace 全量 schema。
2. 不支持除 `hitrace` 之外的日志格式。
3. 不引入 Web UI、MCP、Skill 或服务化入口。
4. 不提交大体积真实 trace fixture。
5. 不做缓存、多数据源管理、并发 session 池或跨进程状态。

## crate 职责

| crate | 职责 |
| --- | --- |
| `kat-rs-datasource` | 解析数据源，构建 Arrow 表，注册 DataFusion，并提供 `query_json(sql)`。 |
| `kat-rs-session` | 创建和持有运行期状态，当前主要持有一个已 build 的 datasource。 |
| `kat-rs-cli` | 使用 `clap` 解析命令行参数，创建 session，build datasource，执行 SQL，并把 JSON 写到 stdout；参数结构支持 `serde`。 |

## 依赖方向

```text
kat-rs-cli -> kat-rs-session -> kat-rs-datasource
```

`kat-rs-datasource` 不依赖 `session` 或 `cli`。`session` 不解析 CLI 参数，也不理解 stdout/stderr。

## datasource 生命周期

```rust
let datasource = TraceDatasource::build(DataSourceConfig {
    source_type: DataSourceType::Hitrace,
    path,
})?;

let json = datasource.query_json("select count(*) as count from hitrace_event").await?;
```

`build` 阶段负责：

1. 打开文件。
2. mmap 文件。
3. 使用 prost struct 解码 protobuf。
4. 调用构建期从 prost struct AST 生成的 Arrow builder。
5. 生成 `RecordBatch`。
6. 注册 DataFusion 表。
7. 释放 mmap 和文件句柄。

`query_json` 阶段只执行 SQL，不再读取或映射原始文件。

## 当前 hitrace 契约

当前 PR 只提供一个最小 hitrace protobuf 契约，用于打通 protobuf -> Arrow -> DataFusion -> JSON 的主链路。

```proto
message HitraceTrace {
  repeated HitraceEvent events = 1;
}

message HitraceEvent {
  uint64 timestamp_ns = 1;
  int32 pid = 2;
  int32 tid = 3;
  string tag = 4;
  string message = 5;
  uint32 cpu = 6;
}
```

对应 SQL 表：

| 表名 | 字段 |
| --- | --- |
| `hitrace_event` | 来自 `HitraceEvent` prost struct 字段，当前为 `timestamp_ns`, `pid`, `tid`, `tag`, `message`, `cpu` |

## session 边界

`kat-rs-session` 提供 `Session::create()`，并存储重要运行状态。当前最小状态为一个可选 datasource：

```rust
let mut session = Session::create();
session.build_datasource(config)?;
let json = session.query_json(sql).await?;
```

如果未 build datasource 就查询，返回明确错误。

## CLI 行为

命令：

```text
kat-rs query --source hitrace --file <path> --sql <sql>
```

行为：

1. `--source` 当前只接受 `hitrace`。
2. `--file` 是 protobuf 二进制 trace 文件路径。
3. `--sql` 是交给 DataFusion 的 SQL 字符串。
4. stdout 只输出 JSON 查询结果。
5. 诊断信息走 `log`，由 `RUST_LOG` 控制。
6. 参数解析使用 `clap`，参数结构支持 `serde` 序列化。

## JSON 输出

查询结果返回 JSON 数组。每一行是一个对象，字段名来自 SQL result schema。

示例：

```json
[
  {
    "count": 2
  }
]
```

## 验证标准

1. 单元测试可以构造 protobuf hitrace 文件并查询 `hitrace_event`。
2. `datasource` build 后删除或关闭原文件句柄不影响后续 SQL 查询。
3. `session` 可以持有 datasource 并执行 SQL。
4. CLI 可以用 SQL 查询给定 hitrace 文件，并输出合法 JSON。
5. 代码中维测日志使用 `log`，不使用 `println!` 输出诊断信息。
6. `cargo test`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check --locked`、`cargo test --locked` 通过。
