# Memory Analysis PACK

这个 External PACK 展示如何把 Rust 来源解码能力接入 PACK 自有 Provider：

```text
UTF-8 text Ftrace
  -> FtraceProvider(...)
  -> typed Ftrace relations
  -> Provider.query(SQL) 返回 eager dp.Table
  -> Workflow 发布 Run Output
```

Provider 构造成功后即可查询；调用方不需要知道转换器、Catalog 路径或物理存储格式。
实现内部由 `kat-datasource` 原生扩展拥有文本语法、Proto 类型合同、有限批次写入和原子
发布，Python Provider 直接调用 `kat_datasource.text_ftrace.decode()`，并负责内部物化复用、
必需关系校验和本地查询。Workflow 不感知 Rust、Parquet 或物化位置。

`text_ftrace_header` 固定存在。只要来源中至少有一个已支持事件，
`text_ftrace_event_occurrence` 和 `text_ftrace_event` 就同时存在。四种首批类型化载荷关系
只在来源实际出现时生成：

- `text_ftrace_event_sched_switch`
- `text_ftrace_event_sched_wakeup`
- `text_ftrace_event_sched_wakeup_new`
- `text_ftrace_event_tracing_mark_write`

合法但未支持的事件只进入 `text_ftrace_unsupported_event` 报告，不产生无类型 raw
payload；一份全部由未支持事件构成的合法 Trace 仍可查询 header。事件根显式保存
`clock_domain + clock_value`；调用方必须根据采集配置提供 domain，Provider 不从文件名
或数值猜测时间语义。

解析合同以 `TASK-PID / TGID / CPU# / TIMESTAMP / FUNCTION` 事件列头为准。
`entries-in-buffer/entries-written` 与 `#P` 只是展示性统计，不进入关系模型，也不参与事件
数量或 CPU 范围校验；数字形式、OpenHarmony 转换器保留的格式占位符以及缺失形式均可接受。

完整的表、字段、关联关系和查询示例见 Provider 的同名文档
[knowledge/providers/ftrace.md](./knowledge/providers/ftrace.md)。运行时也可以使用
`SHOW TABLES` 和 `DESCRIBE <table>` 查看当前 Trace 实际产生的关系及其物理字段。

## 运行

```bash
kat run \
  --pack mem-pack \
  --workflow summarize-ftrace \
  --pack-dir ./examples/packs/mem-pack \
  -- \
  --trace-path /absolute/path/to/trace.ftrace \
  --clock-domain monotonic
```

Provider 默认以 ftrace 文件内容的 SHA-256 为内部目录名，在 `ctx.datasource_root` 下跨
Workflow 复用已经通过校验的 Parquet；调用方不感知目录位置。相同内容使用不同
`clock_domain` 时明确失败，不静默复用错误的时间语义。旧目录打不开、缺少必需关系或
损坏时直接重建。

`redecode` 与 `auto_cleanup` 独立控制本次使用的内容目录：

| `redecode` | `auto_cleanup` | 行为 |
| --- | --- | --- |
| `False` | `False` | 复用或创建，结束后保留 |
| `False` | `True` | 复用或创建，结束后删除 |
| `True` | `False` | 删除旧结果并重新解析，结束后保留 |
| `True` | `True` | 删除旧结果并重新解析，结束后删除 |

Workflow 在 eager `dp.Table` 产生后调用 `finish()`，因此用户无需使用 `with`。同一 Trace
被多个 Workflow 并发查询、重解析或清理时，由调用方协调；本 Provider 不提供并发保障。

## 验证

Rust 合同测试负责解析、类型化 oneof 关系、来源序号间隙、坏输入、有限批次和原子发布：

```bash
cargo test --locked -p kat-datasource
```

PACK pytest 负责 Provider 原生调用边界、Catalog、查询拓扑、失败清理和 Workflow Output：

```bash
kat test --pack-dir ./examples/packs/mem-pack
```

真实 OpenHarmony 设备纵向用例还会执行 HDC 采集、拉取、转换、查询和第二次内容复用。
它要求显式设置 `KAT_HDC_TARGET`；普通 CI 没有设备时跳过，不会猜测或默认选择连接目标。
