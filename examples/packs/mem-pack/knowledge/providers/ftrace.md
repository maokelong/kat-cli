# FtraceProvider 数据合同

`FtraceProvider` 把一份 tracefs 文本提供为可通过 SQL 查询的类型化关系。Provider
构造成功后即可调用 `query()`；返回值是 eager `dp.Table`。转换器、Catalog 路径和物理
存储格式不属于使用者合同。

## 内部物化

物化目录固定为 `workspace_root / source.name`。目录已经包含顶层 Parquet 文件时，Provider
直接打开并执行关系与 `clock_domain` 准入检查，不再次解析。目录不存在或为空时才调用
转换器；非空但没有 Parquet 的目录会被拒绝，避免覆盖不明文件。已有 Parquet 无法打开或
未通过准入检查时明确失败，不隐式删除或重建。

目录身份只由文件名决定。同一 `workspace_root` 下的同名来源会复用同一目录，即使来源路径
或内容不同；调用方需要用不同文件名或不同 `workspace_root` 区分它们。Provider 不提供
`redecode`、自动清理或 `finish()` 接口。

## 运行时发现

不同 Trace 可能产生不同的 payload 表。调用方应通过只读 SQL 查看当前 Catalog：

```python
tables = provider.query("SHOW TABLES")
columns = provider.query("DESCRIBE text_ftrace_event")
```

`provider.tables` 返回当前 Catalog 的稳定排序关系名；`provider.decode_report` 返回排序、
去重后的未支持事件名。`text_ftrace_header` 固定存在；只有至少一个已支持事件时，
`text_ftrace_event_occurrence` 和 `text_ftrace_event` 才同时存在。payload 表只在
来源中出现对应事件时生成，`text_ftrace_unsupported_event` 只在存在未支持事件时生成。

## 关系拓扑

```text
text_ftrace_header

text_ftrace_unsupported_event (存在未支持事件时)

text_ftrace_event_occurrence
  _kat_row_id
      │
      └── text_ftrace_event._kat_parent_row_id
              ├── text_ftrace_event_sched_switch._kat_parent_row_id
              ├── text_ftrace_event_sched_wakeup._kat_parent_row_id
              ├── text_ftrace_event_sched_wakeup_new._kat_parent_row_id
              ├── text_ftrace_event_tracing_mark_write._kat_parent_row_id
              ├── text_ftrace_event_sched_blocked_reason._kat_parent_row_id
              ├── text_ftrace_event_mm_filemap_add_to_page_cache._kat_parent_row_id
              ├── text_ftrace_event_mm_filemap_delete_from_page_cache._kat_parent_row_id
              ├── text_ftrace_event_block_rq_issue._kat_parent_row_id
              ├── text_ftrace_event_block_rq_complete._kat_parent_row_id
              ├── text_ftrace_event_binder_transaction._kat_parent_row_id
              └── text_ftrace_event_print._kat_parent_row_id
```

`_kat_row_id` 只在当前 Catalog 内标识一行；它不是跨转换稳定的业务 ID。
每个已支持的来源事件恰好有一行 occurrence、一行事件根和一行对应 payload。
合法但暂未支持的事件不会产生这三类行，因此 `source_event_sequence` 可以出现间隙。

### `text_ftrace_unsupported_event`

按名称汇总本次转换遇到的合法但未支持事件；名称排序且去重。

| 字段 | Arrow 类型 | 含义 |
| --- | --- | --- |
| `event_name` | `Utf8` | 未支持的 ftrace 事件名 |

## 固定关系

### `text_ftrace_header`

每个 Catalog 恰好一行，只保留解析事件结构需要的输入合同。展示性的
`entries-in-buffer/entries-written` 与 `#P` 行会被忽略，无论它包含数字、格式占位符或
完全缺失都不影响事件解析；Provider 不据此校验事件数量或 CPU 范围。

| 字段 | Arrow 类型 | 含义 |
| --- | --- | --- |
| `tracer` | `Utf8` | tracefs tracer 名称 |
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

### `text_ftrace_event_sched_blocked_reason`

| 字段 | Arrow 类型 |
| --- | --- |
| `pid` | `Int32` |
| `io_wait` | `UInt32` |
| `caller` | `Utf8` |

### `text_ftrace_event_mm_filemap_add_to_page_cache`

| 字段 | Arrow 类型 |
| --- | --- |
| `device_major` | `UInt32` |
| `device_minor` | `UInt32` |
| `inode` | `UInt64` |
| `page_frame_number` | `UInt64` |
| `offset_bytes` | `UInt64` |
| `order` | `UInt32?` |
| `page_address` | `Utf8?` |

`order` 来自较新的 folio 输出，`page_address` 来自较旧的 page 输出；不存在的字段为 null。

### `text_ftrace_event_mm_filemap_delete_from_page_cache`

字段与 `text_ftrace_event_mm_filemap_add_to_page_cache` 相同，但只记录删除事件。

### `text_ftrace_event_block_rq_issue`

| 字段 | Arrow 类型 |
| --- | --- |
| `device_major` | `UInt32` |
| `device_minor` | `UInt32` |
| `rwbs` | `Utf8` |
| `bytes` | `UInt32` |
| `command` | `Utf8` |
| `sector` | `UInt64` |
| `sector_count` | `UInt32` |
| `process_name` | `Utf8` |

### `text_ftrace_event_block_rq_complete`

| 字段 | Arrow 类型 |
| --- | --- |
| `device_major` | `UInt32` |
| `device_minor` | `UInt32` |
| `rwbs` | `Utf8` |
| `command` | `Utf8` |
| `sector` | `UInt64` |
| `sector_count` | `UInt32` |
| `error` | `Int32` |

### `text_ftrace_event_binder_transaction`

| 字段 | Arrow 类型 |
| --- | --- |
| `transaction_id` | `Int32` |
| `destination_node_id` | `Int32` |
| `destination_process_id` | `Int32` |
| `destination_thread_id` | `Int32` |
| `reply` | `Int32` |
| `flags` | `UInt32` |
| `code` | `UInt32` |

### `text_ftrace_event_print`

| 字段 | Arrow 类型 |
| --- | --- |
| `instruction_pointer` | `Utf8` |
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

`text_ftrace_event` 的业务字段以及各类 payload 字段来源于
`kat/platform/datasource/proto/text_ftrace/text_ftrace_event.proto`。

以下内容当前不在 Proto 中，不能只根据该文件完整生成：

- `text_ftrace_header`、`text_ftrace_event_occurrence` 和
  `text_ftrace_unsupported_event`；
- `_kat_row_id`、`_kat_parent_row_id` 以及父子关系；
- Proto 类型到实际 Arrow/Parquet 类型的映射；
- 表的生成条件、时钟语义和稳定性承诺。

本切片不引入第二套 relational plan 或文档生成器；上述物理关系和查询语义由本文件与转换
合同测试共同维护。
