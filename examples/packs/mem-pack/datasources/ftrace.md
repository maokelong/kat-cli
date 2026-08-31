# FtraceProvider 数据合同

`FtraceProvider` 把一份 tracefs 文本提供为可通过 SQL 查询的类型化关系。Provider
构造成功后即可调用 `query()`；返回值是 eager `dp.Table`。转换器、Catalog 路径和物理
存储格式不属于使用者合同。

## 内部物化

默认模式以来源文件内容的 SHA-256 作为内部目录名。相同内容跨 Workflow 复用已经通过
准入检查的 Parquet；目录位置不属于公共 API。旧目录无法打开、缺少固定关系或内容损坏
时，Provider 删除并重建。相同内容已经保存的 `clock_domain` 与本次请求不一致时明确
失败，避免把同一时间值解释成不同的时钟域。

调用方需要在 Provider 对象释放时删除内部物化结果，可以传入 `auto_cleanup=True`。该
模式使用实例独占临时目录，不命中也不删除默认的 SHA-256 目录。未传入时默认为
`False`，保留可供后续 Workflow 复用的内部结果。

## 运行时发现

不同 Trace 可能产生不同的 payload 表。调用方应通过只读 SQL 查看当前 Catalog：

```python
tables = provider.query("SHOW TABLES")
columns = provider.query("DESCRIBE text_ftrace_event")
```

`text_ftrace_header`、`text_ftrace_event_occurrence` 和 `text_ftrace_event` 固定存在。
四张 payload 表只在来源中出现对应事件时生成。

## 关系拓扑

```text
text_ftrace_header

text_ftrace_event_occurrence
  _kat_row_id
      │
      └── text_ftrace_event._kat_parent_row_id
              ├── text_ftrace_event_sched_switch._kat_parent_row_id
              ├── text_ftrace_event_sched_wakeup._kat_parent_row_id
              ├── text_ftrace_event_sched_wakeup_new._kat_parent_row_id
              └── text_ftrace_event_tracing_mark_write._kat_parent_row_id
```

`_kat_row_id` 只在当前 Catalog 内标识一行；它不是跨转换稳定的业务 ID。
每个已支持的来源事件恰好有一行 occurrence、一行事件根和一行对应 payload。
合法但暂未支持的事件不会产生这三类行，因此 `source_event_sequence` 可以出现间隙。

## 固定关系

### `text_ftrace_header`

每个 Catalog 恰好一行，描述输入文件头。

| 字段 | Arrow 类型 | 含义 |
| --- | --- | --- |
| `tracer` | `Utf8` | tracefs tracer 名称 |
| `entries_in_buffer` | `UInt64` | 文件头声明的 buffer 内事件数 |
| `entries_written` | `UInt64` | 文件头声明的累计写入事件数 |
| `cpu_count` | `UInt32` | 文件头声明的 CPU 数量 |
| `has_tgid_column` | `Boolean` | 事件列标题是否包含 TGID |

### `text_ftrace_event_occurrence`

保留已支持事件在来源事件流中的位置。

| 字段 | Arrow 类型 | 含义 |
| --- | --- | --- |
| `_kat_row_id` | `UInt64` | 当前 occurrence 行 ID |
| `source_event_sequence` | `UInt64` | 从 0 开始的来源事件序号；未知事件也占用序号 |

### `text_ftrace_event`

保存所有已支持事件共享的事件头。

| 字段 | Arrow 类型 | 含义 |
| --- | --- | --- |
| `_kat_row_id` | `UInt64` | 当前事件根行 ID |
| `_kat_parent_row_id` | `UInt64` | 对应 occurrence 的 `_kat_row_id` |
| `clock_domain` | `Utf8` | 调用方明确提供的采集时钟域 |
| `clock_value` | `UInt64` | tracefs 十进制时间值换算得到的纳秒刻度 |
| `cpu` | `UInt32` | 事件发生的 CPU |
| `emitter_thread_name` | `Utf8` | 事件发出线程名 |
| `emitter_thread_id` | `Int32` | 事件发出线程 ID |
| `emitter_process_id` | `Int32?` | TGID；来源没有 TGID 列时为 null |
| `context_flags` | `Utf8` | tracefs 上下文 flags 原文 |

`clock_value` 只有与同一 `clock_domain` 配对时才有意义。Provider 不承诺不同 clock
domain 的数值可直接比较。

## 按需生成的 payload 关系

每张 payload 表都包含：

| 字段 | Arrow 类型 | 含义 |
| --- | --- | --- |
| `_kat_row_id` | `UInt64` | 当前 payload 表内的行 ID |
| `_kat_parent_row_id` | `UInt64` | 对应 `text_ftrace_event._kat_row_id` |

### `text_ftrace_event_sched_switch`

| 字段 | Arrow 类型 |
| --- | --- |
| `previous_thread_name` | `Utf8` |
| `previous_thread_id` | `Int32` |
| `previous_priority` | `Int32` |
| `previous_state` | `Utf8` |
| `next_thread_name` | `Utf8` |
| `next_thread_id` | `Int32` |
| `next_priority` | `Int32` |

### `text_ftrace_event_sched_wakeup`

| 字段 | Arrow 类型 |
| --- | --- |
| `thread_name` | `Utf8` |
| `thread_id` | `Int32` |
| `priority` | `Int32` |
| `target_cpu` | `UInt32` |

### `text_ftrace_event_sched_wakeup_new`

字段与 `text_ftrace_event_sched_wakeup` 相同，但只记录 `sched_wakeup_new` 事件。

### `text_ftrace_event_tracing_mark_write`

| 字段 | Arrow 类型 |
| --- | --- |
| `content` | `Utf8` |

## 查询示例

查询发生过线程切换的来源位置和公共事件头：

```sql
SELECT
    o.source_event_sequence,
    e.clock_domain,
    e.clock_value,
    e.cpu,
    s.previous_thread_id,
    s.next_thread_id
FROM text_ftrace_event_occurrence AS o
JOIN text_ftrace_event AS e
  ON e._kat_parent_row_id = o._kat_row_id
JOIN text_ftrace_event_sched_switch AS s
  ON s._kat_parent_row_id = e._kat_row_id
ORDER BY o.source_event_sequence
```

## 文档来源

`text_ftrace_event` 的业务字段以及四类 payload 字段来源于
`crates/ftrace2parquet/proto/text_ftrace_event.proto`。

以下内容当前不在 Proto 中，不能只根据该文件完整生成：

- `text_ftrace_header` 和 `text_ftrace_event_occurrence`；
- `_kat_row_id`、`_kat_parent_row_id` 以及父子关系；
- Proto 类型到实际 Arrow/Parquet 类型的映射；
- 表的生成条件、时钟语义和稳定性承诺。

本切片不引入第二套 relational plan 或文档生成器；上述物理关系和查询语义由本文件与转换
合同测试共同维护。
