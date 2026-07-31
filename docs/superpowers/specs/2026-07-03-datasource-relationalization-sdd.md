# kat-datasource Profiler Payload 关系化架构 SDD

状态：PR 候选设计；本文随当前 PR 的 one-pass 实现接受评议，验证证据见第 11.5 节。

## 1. 背景与问题

`kat-datasource` 需要把 `.htrace` 中的 profiler payload 转成可由 DataFusion 查询的 Parquet dataset。

payload 已经有 `.proto` 描述，但 protobuf 的 nested message、repeated、oneof 和 bytes 等结构不能直接作为稳定、易用的 SQL 表面。逐个 plugin 手写表 builder 会重复实现 decode、字段遍历和 Arrow 写入；无条件把每个 message 都拆成表，又会产生大量只用于恢复对象结构的表和 join。

本架构采用一个统一原则：

> 保留 `.proto` 的字段信息和层级；单行内部能够由 Arrow 表达的结构保留为列，只有产生多行或表达 oneof variant 时才生成子表。

外层主要通过 Python DataFusion 查询，因此 singular message 可以使用 Arrow `Struct`，bytes 使用 Arrow `Binary`；产生多个值或多行的 repeated 字段仍生成 child table。

## 2. 目标

1. 使用 typed prost 解码 profiler protobuf payload。
2. 根据 proto descriptor 和有限、通用的结构规则生成关系化计划。
3. 完整保留 scalar、enum、bytes、message、repeated scalar、repeated message 和 oneof 数据。
4. 生成 SQL 友好的 Parquet 表，并由 DataFusion 直接查询。
5. 用稳定的表名和公共关系列表达表之间的关系。
6. 对大 trace 采用有界表缓冲和增量 Parquet 写入，不在内存中保留全量关系行。
7. 新增已知 protobuf payload 时，只增加 decode 注册和 descriptor 覆盖；不为每个 message 手写表转换代码。
8. 通用关系化表作为现有 Hitrace source facts 的附加输出，不改变 `clock_domain`、`clock_snapshot`、`sched_switch` 及其校验语义。
9. 保留未知 plugin/section 的 observer 时序、排序后的 `unsupported_*` 返回数组，以及失败时不发布 `.kat-dataset` marker 的契约。

## 3. 非目标

1. 不设计业务查询模型、业务别名、统计派生或分析 API。
2. 不按业务判断字段价值，也不丢弃 `.proto` 中已支持结构的数据。
3. 本切片不删除或替换现有 source-fact projection、旧 Arrow 查询路径和公开 import 接口。
4. 不做 runtime reflection decoder；decode 主路径仍使用 generated Rust type 和 `prost::Message::decode`。
5. 不承诺 dataset 跨进程长期持久化、迁移、恢复或原子替换。
6. 不新增 `catalog.json` 或其他 Dataset 元数据文件；遵守 ADR 0020 的 Dataset Storage 契约。
7. 不修改现有 source facts 的表名、schema、内容和时钟语义；覆盖目标的失败生命周期遵守 ADR 0020。
8. 不修改 ADR 0025 定义的未知内容报告和失败传播行为。

## 4. 数据流与分层

### 4.1 端到端数据流

```text
kat import / import_hitrace
  -> materializer.rs
  -> ManagedDatasetWriter::begin(target)
  -> formats/hitrace + decode/profiler（读取一次）
       -> LongTermHitraceSink
            -> existing TraceRecord
                 -> collect source facts
                 -> validate clock / loss / ordering / thread continuity
            -> record.rs: DecodedPayload
                 -> payload_value.rs: PayloadValue
                 -> relational/plan.rs: ExpansionPlan
                 -> relational/plan_exec.rs + table_data.rs
                 -> relational/table_batch.rs: Arrow builders / RecordBatch
       -> observe unsupported plugin / section
  -> LongTermHitraceSink::finish
       -> finish relational tables
       -> generic relational tables
       -> existing clock_domain / clock_snapshot / sched_switch facts
  -> ManagedDatasetWriter::finish
       -> .kat-dataset + tables/*.parquet
  -> Dataset Storage resolution
  -> Python DataFusion
```

CLI 继续调用原有 `import_hitrace`。`materializer.rs` 先按 ADR 0020 打开统一 Dataset writer，再读取一次输入。`LongTermHitraceSink` 同时接收现有 source records 和 `DecodedPayload`：前者继续生成并校验 source facts，后者进入通用关系化展开。两类输出写入同一个 Dataset。

这相对 base 明确改变了目标打开时机：base 在 decode 和 source-fact 校验成功后才打开 writer；当前实现为了单遍流式写入，在 decode 前调用 `ManagedDatasetWriter::begin`。因此，用户通过 `--overwrite-dataset` 授权覆盖已有目标后，旧内容可能在输入解码或校验失败前被删除。失败不会调用 `ManagedDatasetWriter::finish`，因而不会发布 `.kat-dataset` marker，但不恢复旧内容。该行为遵守 ADR 0020 的破坏式 fail-fast 语义，不承诺备份、回滚或失败恢复。

`TraceRecordSink` 显式声明是否接收 `DecodedPayload` 和现有 source records。当前组合 sink 同时接收两类 record；同一个 typed decoder 不重复解析输入，也不让关系化层感知时钟业务。

### 4.2 各层输入和输出

| 顺序 | 模块 | 输入 | 输出 | 职责 |
| --- | --- | --- | --- | --- |
| 1 | `materializer.rs` | 一个输入 source | 统一 Dataset 写入流程 | 保持公开 import 入口；先打开 ADR 0020 writer，再组装组合 sink 并单遍 decode。 |
| 2 | `formats/hitrace/` | `.htrace` bytes | profiler envelope | 读取文件、解析 framing，并按 plugin name / payload kind 分发。 |
| 3 | `decode/profiler/` | envelope payload bytes | typed prost message、现有 records 与 `DecodedPayload` | 根据 `ProfilerPluginRoute` 选择 generated Rust type 并执行 decode；同一 decoder 同时服务现有 facts 和新增关系化输出。 |
| 4 | `record.rs` | typed prost message | `TraceRecord` / `DecodedPayload` | 保留现有 record 变体，并增加 decode 后、关系化前的通用 payload 边界。 |
| 5 | `payload_value.rs` | typed prost message | `PayloadValue` | 把不同 Rust message 转成统一、只读的结构树，供通用规则访问。 |
| 6 | `relational/plan.rs` | root message + descriptor 摘要 | `ExpansionPlanItem` | 判断会生成哪些表、表名、源路径和运行时父表。 |
| 7 | `relational/plan_exec.rs`、`table_data.rs` | payload value + plan | 各表的列追加操作 | 找到计划对应的数据，维护父子索引，并把字段追加到表缓冲。 |
| 8 | `relational/table_batch.rs` | 列追加操作 | Arrow `RecordBatch` | 创建或复用 Arrow builders，达到阈值后生成 batch。 |
| 9 | `dataset_writer.rs` | 关系表 batch 与现有 source facts | `.kat-dataset` + Parquet files | 通过逻辑表名增量写表；Datasource 不指定物理文件名，不写 catalog。 |
| 10 | Dataset Storage / Python DataFusion | resolved dataset | SQL 查询结果 | 由 Dataset Storage 解析合法 Parquet 表并交给外层查询，不重新解释 protobuf。 |

### 4.3 Build-time descriptor 摘要

`build/relational_descriptor_codegen.rs` 从项目使用的 proto descriptor 生成精简 Rust 数据。摘要只保留关系化需要的信息：

```text
message package / name
field name
field label
field scalar、enum、bytes 或 message 类型
message type name
enum value / name
oneof group
```

runtime 不读取 `.proto` 文件，也不解析完整 `FileDescriptorSet`。descriptor 摘要负责说明“数据结构是什么”，转换规则负责说明“这种结构如何成为列或表”。

当前 plan 使用 proto message 短名索引 descriptor。codegen 会拒绝短名冲突，不能在两个 package 或 nested path 中静默绑定到错误 message。

### 4.4 Decode 边界

`decode/profiler/` 通过一张 `ProfilerPluginRoute` 表登记 plugin 的 config/data typed root。每条 route 只包含：

```text
plugin_name
config payload type（可选）
data payload type
```

decode 层执行：

```text
payload bytes
  -> <GeneratedType as prost::Message>::decode
  -> PayloadValue
  -> DecodedPayload {
       root_message,
       message
     }
```

decode 层不决定表名、不展开 repeated/oneof、不注入业务字段，也不为 ftrace 或 native_hook 建立特殊表模型。

`plugin_name` 只用于 envelope registry 选择 typed decoder，不进入 `DecodedPayload`，也不参与关系表身份。关系化层按 protobuf `root_message` 选择 plan 和输出表；多个 plugin alias 使用同一个 typed root 时，共享同一组关系表。

### 4.5 `DecodedPayload` 与 `PayloadValue`

`DecodedPayload` 表示“一份已经完成 protobuf 解码的 profiler payload”，包含路由信息和数据：

```text
DecodedPayload
  root_message
  message: PayloadValue
```

`PayloadValue` 是 typed prost message 的通用只读结构表示。它保留：

```text
null
bool / signed / unsigned / float
string / bytes
sequence
message fields
oneof variant
```

例如：

```proto
message StackMap {
  uint32 id = 1;
  repeated uint64 frame_map_id = 2;
  repeated uint64 ip = 3;
}
```

typed decode 后对应的 `PayloadValue` 概念形状是：

```text
Message {
  id: U64(7),
  frame_map_id: Sequence [U64(1), U64(2)],
  ip: Sequence [U64(4096), U64(8192)]
}
```

它不是查询结果，也不决定表结构。关系化层按 descriptor 和 plan 读取它。

`PayloadValueSerializer` 只服务于 typed prost message 到内部 `PayloadValue` 的转换，不是对外序列化格式。这里没有直接使用现成通用转换方案，原因如下：

| 方案 | 不直接使用的原因 |
| --- | --- |
| `serde_json::Value` | bytes 会变成数字数组，无法直接保留 Arrow `Binary` 语义；同时会为字段名和容器创建额外的 owned JSON 中间对象。 |
| `serde_arrow` | 适合把同构记录写入一张已知 Arrow 表，但本架构需要根据 descriptor 把一份 payload 分发到多张父子表，并维护 `row_index` / `parent_index`，不能直接替代关系化计划和遍历。 |
| `prost-reflect::DynamicMessage` | 可以运行时反射字段，但会把 decode 主路径改成 runtime reflection，与已确认的 typed prost decode 边界不一致。 |

因此当前实现使用受限的内部 value tree：保留 binary、数值符号和 message/sequence 结构，只实现关系化遍历需要的 Serde 输入能力。它不承担 JSON、Arrow 或 protobuf 的通用序列化职责。

## 5. 关系化转换

### 5.1 转换原则

1. 每个 decoded root payload 都在 root table 生成一行。
2. scalar、enum、bytes 是当前 row 的列。
3. singular message 是当前 row 的 nullable `Struct` 列。
4. repeated scalar / enum / bytes 产生多行，因此生成 value child table。
5. repeated message 产生多行，因此生成 child table。
6. oneof 的父 message 保留命中 variant 名；实际 variant 生成 child table。
7. message 中继续出现 message、repeated 或 oneof 时，递归应用相同规则。
8. 表名保留完整 proto field path；物理父表单独记录。

descriptor 字段按以下顺序分类：

```text
oneof variant
  -> repeated
  -> message
  -> enum
  -> bytes
  -> scalar
```

### 5.2 规则总表

| 规则 | 中文名 | descriptor 形态 | 当前输出 | 是否生成子表 |
| --- | --- | --- | --- | --- |
| `RootRecord` | 根记录 | decoded root message | `<root_message_snake>` 一行 | 否 |
| `ScalarField` | 标量字段 | 非 repeated、非 oneof 的数值、布尔、字符串字段 | 当前 row 的 primitive 列 | 否 |
| `EnumField` | 枚举字段 | 非 repeated、非 oneof 的 enum 字段 | `<field>` 原值列 + `<field>_name` 名称列 | 否 |
| `BytesField` | 字节字段 | 非 repeated、非 oneof 的 bytes 字段 | 当前 row 的 Arrow `Binary` 列 | 否 |
| `MessageField` | Message 字段 | 非 repeated、非 oneof 的 message 字段 | 当前 row 的 nullable `Struct` 列 | 否 |
| `RepeatedScalar` | 重复标量字段 | repeated scalar / enum / bytes | 每个值一行的 value child table | 是 |
| `RepeatedMessage` | 重复 Message 字段 | repeated message | 每个元素一行的 child table | 是 |
| `OneofVariant` | Oneof 分支 | 带 `oneof_index` 的 scalar / enum / bytes / message | 父 row 记录 variant；variant child table 保存值 | 是 |

### 5.3 `RootRecord`

为什么需要：root message 是一份 decoded payload 的关系锚点。即使 root 没有直接 scalar，仍需要 root row 承接 Struct 列以及所有一级 child table。

规则：

```text
RootMessage -> root_message_snake
```

原数据：

```proto
message BatchNativeHookData {
  repeated NativeHookData events = 1;
}
```

```text
BatchNativeHookData {
  events: [event-0, event-1]
}
```

展开后：

`batch_native_hook_data`

| source_index | row_index | parent_index |
| ---: | ---: | --- |
| 0 | 0 | null |

`batch_native_hook_data_events`

| source_index | row_index | parent_index | ... |
| ---: | ---: | ---: | --- |
| 0 | 0 | 0 | event-0 fields |
| 0 | 1 | 0 | event-1 fields |

### 5.4 `ScalarField`

为什么提取为规则：protobuf scalar 可以一对一映射到 Arrow primitive，不会改变行数。

规则：字段留在当前 row；字段名使用 proto field name；数值类型按有符号、无符号和位宽映射。

原数据：

```proto
message MemoryData {
  uint64 zram = 1;
  bool compressed = 2;
  string source = 3;
}
```

```text
MemoryData { zram: 1024, compressed: true, source: "system" }
```

展开后：

`memory_data`

| source_index | row_index | parent_index | zram | compressed | source |
| ---: | ---: | --- | ---: | --- | --- |
| 0 | 0 | null | 1024 | true | system |

### 5.5 `EnumField`

为什么提取为规则：只保存枚举数值适合计算，但查询者难以读懂；只保存名称又会丢失原始值。二者都属于 proto 信息，不是业务派生。

规则：生成原字段名数值列和 `<field>_name` 名称列。未知枚举值保留原值，名称为 null。

原数据：

```proto
enum EventStatus {
  TRACE_START = 0;
  TRACE_END = 1;
}

message Event {
  EventStatus status = 1;
}
```

```text
Event { status: TRACE_START }
```

展开后：

`event`

| source_index | row_index | status | status_name |
| ---: | ---: | ---: | --- |
| 0 | 0 | 0 | TRACE_START |

### 5.6 `BytesField`

为什么提取为规则：protobuf bytes 是原始字节序列，不等价于 UTF-8 string。强制转字符串会改变内容或在无效 UTF-8 时失败。

规则：直接写入 Arrow `Binary` / Parquet `BYTE_ARRAY`，不做文本解释。

build-time 从 proto descriptor 自动识别 `bytes` 字段并配置 binary 序列化，不维护字段名 allowlist。

原数据：

```proto
message Blob {
  bytes payload = 1;
}
```

```text
Blob { payload: [0x00, 0xff, 0x41] }
```

展开后：

`blob`

| source_index | row_index | payload |
| ---: | ---: | --- |
| 0 | 0 | `00 ff 41` |

### 5.7 `MessageField`

为什么提取为规则：singular message 与父 message 是一对零或一关系。拆成独立表不会减少重复数据，却会增加表数量和 join。

规则：作为当前 row 的 nullable Arrow `Struct` 列；message 内部继续递归生成 primitive 和 Struct 子字段。message 缺失时整个 Struct 为 null。message 内的 repeated 字段不进入 Struct，继续按 repeated 规则生成 child table。

如果 message 自身没有任何可内联列、只包含会生成子表的 repeated message 或 oneof variant，则不生成空 Struct 列。Parquet 不支持写空 Struct；其后代数据仍按完整 proto path 生成子表。

原数据：

```proto
message Root {
  Meta meta = 1;
}

message Meta {
  string name = 1;
  Position position = 2;
}

message Position {
  uint64 line = 1;
}
```

```text
Root {
  meta: {
    name: "main",
    position: { line: 42 }
  }
}
```

展开后：

`root`

| source_index | row_index | meta |
| ---: | ---: | --- |
| 0 | 0 | `{name: "main", position: {line: 42}}` |

不会生成 `root_meta` 或 `root_meta_position`。

DataFusion 查询示例：

```sql
select meta['name'], meta['position']['line']
from root;
```

### 5.8 `RepeatedScalar`

为什么提取为规则：repeated scalar/enum/bytes 会为同一父 row 产生多个值，不能放进单个 primitive 列。继续使用 child table 可以保持现有 SQL 查询习惯，也避免为这一条规则引入通用 List builder。

规则：

```text
repeated scalar -> child table(value: primitive)
repeated bytes  -> child table(value: Binary)
repeated enum   -> child table(value: Int32, value_name: Utf8)
```

原数据：

```proto
message StackMap {
  uint32 id = 1;
  repeated uint64 frame_map_id = 2;
  repeated uint64 ip = 3;
}
```

```text
StackMap {
  id: 7,
  frame_map_id: [1, 2],
  ip: [4096, 8192]
}
```

展开后：

`batch_native_hook_data_events_stack_map`

| source_index | row_index | parent_index | id |
| ---: | ---: | ---: | ---: |
| 0 | 0 | 12 | 7 |

`batch_native_hook_data_events_stack_map_frame_map_id`

| source_index | row_index | parent_index | value |
| ---: | ---: | ---: | ---: |
| 0 | 0 | 0 | 1 |
| 0 | 1 | 0 | 2 |

`batch_native_hook_data_events_stack_map_ip`

| source_index | row_index | parent_index | value |
| ---: | ---: | ---: | ---: |
| 0 | 0 | 0 | 4096 |
| 0 | 1 | 0 | 8192 |

原 repeated enum：

```text
states: [TRACE_START, TRACE_END]
```

展开后的 `states` child table：

| source_index | row_index | parent_index | value | value_name |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 0 | 0 | `TRACE_START` |
| 0 | 1 | 0 | 1 | `TRACE_END` |

### 5.9 `RepeatedMessage`

为什么提取为规则：每个 repeated message 元素都有自己的字段和行身份，一个 `List<Struct>` 会把大规模事件集合压在单个 Parquet row 内，不利于筛选、聚合和流式写入。

规则：每个 repeated message field 生成 child table，每个元素生成一行。深层 repeated message 继续生成下一层 child table。

原数据：

```proto
message MemoryData {
  repeated ProcessMemoryInfo processesinfo = 1;
}

message ProcessMemoryInfo {
  int32 pid = 1;
  repeated SmapsInfo smapinfo = 2;
}

message SmapsInfo {
  string path = 1;
  uint64 rss = 2;
}
```

```text
MemoryData {
  processesinfo: [
    {
      pid: 100,
      smapinfo: [
        { path: "/a.so", rss: 64 },
        { path: "/b.so", rss: 32 }
      ]
    }
  ]
}
```

展开后：

`memory_data`

| source_index | row_index | parent_index |
| ---: | ---: | --- |
| 0 | 0 | null |

`memory_data_processesinfo`

| source_index | row_index | parent_index | pid |
| ---: | ---: | ---: | ---: |
| 0 | 0 | 0 | 100 |

`memory_data_processesinfo_smapinfo`

| source_index | row_index | parent_index | path | rss |
| ---: | ---: | ---: | --- | ---: |
| 0 | 0 | 0 | /a.so | 64 |
| 0 | 1 | 0 | /b.so | 32 |

### 5.10 `OneofVariant`

为什么提取为规则：oneof 的字段集合互斥，不同 variant 的 schema 可能完全不同。把所有 variant 合成一张宽表会产生大量无意义 null，也难以表达 message variant 内部的 repeated message。

规则：

1. oneof 所在的父表增加一列，列名使用 oneof group name，值为实际命中的 variant field name。
2. 每个实际出现的 variant 生成独立 child table。
3. 表名跳过 oneof group name，使用“父表完整 proto path + variant field name”。
4. scalar variant 生成 `value`；enum variant生成 `value` 和 `value_name`；bytes variant 生成 Binary `value`；message variant 按 message 字段规则生成列。
5. 不生成 oneof group 中间表。

原数据：

```proto
message NativeHookData {
  uint64 tv_sec = 1;
  oneof event {
    AllocEvent alloc_event = 2;
    uint64 mmap_addr = 3;
    EventStatus status = 4;
  }
}

message AllocEvent {
  int32 pid = 1;
  uint64 size = 2;
  repeated Frame frame_info = 3;
}
```

```text
NativeHookData {
  tv_sec: 1,
  event: {
    alloc_event: {
      pid: 42,
      size: 64,
      frame_info: [{ ip: 4096 }]
    }
  }
}
```

展开后：

`batch_native_hook_data_events`

| source_index | row_index | parent_index | tv_sec | event |
| ---: | ---: | ---: | ---: | --- |
| 0 | 8 | 0 | 1 | alloc_event |

`batch_native_hook_data_events_alloc_event`

| source_index | row_index | parent_index | pid | size |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 3 | 8 | 42 | 64 |

`batch_native_hook_data_events_alloc_event_frame_info`

| source_index | row_index | parent_index | ip |
| ---: | ---: | ---: | ---: |
| 0 | 0 | 3 | 4096 |

不会生成 `batch_native_hook_data_events_event`。

如果命中 scalar variant `mmap_addr`：

`batch_native_hook_data_events_mmap_addr`

| source_index | row_index | parent_index | value |
| ---: | ---: | ---: | ---: |
| 0 | 0 | 9 | 4096 |

### 5.11 Struct 内的 repeated message

表名必须保留完整 proto path，即使 path 中某个 message 只作为 Struct 列存在。

原数据：

```proto
message Root {
  Meta meta = 1;
}

message Meta {
  string name = 1;
  repeated Item items = 2;
}
```

展开后：

`root`

| source_index | row_index | meta |
| ---: | ---: | --- |
| 0 | 5 | `{name: "sample"}` |

`root_meta_items`

| source_index | row_index | parent_index | value |
| ---: | ---: | ---: | --- |
| 0 | 0 | 5 | item-0 |

这里不存在 `root_meta` 表：

- `root_meta_items` 的名字保存完整 proto path，避免与 `root.config.items` 等字段冲突。
- 它的运行时父表是 `root`；`plan.rs` 在生成计划时明确这一关系，执行时据此写入 `parent_index`。

## 6. 表和行关系契约

### 6.1 公共列

每张关系表包含：

| 列 | 类型 | 含义 |
| --- | --- | --- |
| `source_index` | `UInt64` | 当前 import 输入中的 source 序号。当前公开的 `import_hitrace` 每次只接收一个 source，因此该值为 `0`。 |
| `row_index` | `UInt64` | 同一张表内递增的行号。 |
| `parent_index` | nullable `UInt64` | 指向该表转换契约所定义父表的 `row_index`；root row 为 null。 |

本切片不提供多 source import，也不增加 payload 级公共键。一个 source 中可以有多份 decoded payload，父子关系由实际父表的 table-local `row_index` 承接。

### 6.2 固定 join 方式

子表回到物理父表时使用：

```sql
select *
from child c
join parent p
  on c.source_index = p.source_index
 and c.parent_index = p.row_index;
```

`parent_index` 只能结合转换契约中的父表解释，不能仅根据表名截断猜测父表。`parent_table` 保存在内存中的 expansion plan 里，用于保证父表先写和生成索引；本切片不把它持久化成 Dataset metadata。

### 6.3 表名

表名使用：

```text
root_message_snake + "_" + full proto field path
```

规则：

1. root 和 path 段以 `_` 连接，满足 ADR 0020 的 Dataset table name 规则。
2. 普通字段名内部也使用 `_`，因此表名用于稳定命名，不作为父表关系解析格式。
3. oneof group name 不进入表名。
4. root table 直接使用 root message 的 snake_case，不增加 `overview`。
5. 表名不承诺每个 path 前缀都有对应实体表。
6. root message 和每个 proto field path 段都规范化为 snake_case；`source_path` 仍保留 descriptor 原名用于读取数据。
7. 如果两条不同 proto path 规范化后得到同一个表名，plan 构造必须报错，不能覆盖或合并。

## 7. 计划生成与执行

### 7.1 `plan.rs`：决定生成哪些表

`plan.rs` 从 root message 出发读取 descriptor，生成表级 `ExpansionPlanItem`。计划项只为真实输出表存在：

```text
ExpansionPlanItem
  rule: RootRecord | RepeatedScalar | RepeatedMessage | OneofVariant
  root_message
  source_path
  source_message
  output_table
  parent_table
```

`ScalarField`、`EnumField`、`BytesField` 和 `MessageField` 决定列 schema，不单独生成表计划项。`RepeatedScalar`、`RepeatedMessage` 和 `OneofVariant` 都生成表计划项。

例如：

```text
BatchNativeHookData
  events: repeated NativeHookData
    event.alloc_event: oneof AllocEvent
      frame_info: repeated Frame
```

生成：

```text
RootRecord
  output_table: batch_native_hook_data
  parent_table: null

RepeatedMessage
  source_path: events
  output_table: batch_native_hook_data_events
  parent_table: batch_native_hook_data

OneofVariant
  source_path: events.event.alloc_event
  output_table: batch_native_hook_data_events_alloc_event
  parent_table: batch_native_hook_data_events

RepeatedMessage
  source_path: events.event.alloc_event.frame_info
  output_table: batch_native_hook_data_events_alloc_event_frame_info
  parent_table: batch_native_hook_data_events_alloc_event
```

### 7.2 `plan_exec.rs`：复用计划执行结构

同一种 root message 会出现很多次。`plan_exec.rs` 在第一次遇到 root 时，把表计划整理成可重复执行的 root plan：

1. 确定父表必须先于子表执行。
2. 把共享 source path 的步骤放在同一遍访问中。
3. 把 oneof group 的 variants 放入同一个 dispatch。
4. 缓存每张表的列 schema。

这一步只整理 descriptor path 和执行顺序，不写入 plugin 名、event 名或业务判断。

每个 payload 调用一次 `emit_payload`：

```text
DecodedPayload.message
  -> 取 root_message 对应的已编译计划
  -> 按计划访问 PayloadValue
  -> 依次追加 root、父表、子表数据
```

`RelationalDatasetSink::push_payload` 收到一份 `DecodedPayload` 后立即执行这条数据流，并在执行完成后清理该 payload 的父子索引。sink 不跨 payload 保留解码结果；常驻缓冲只包含各表的 Arrow builders 和有界 Parquet 写入队列。

### 7.3 `table_data.rs`：找到行数据并追加字段

`table_data.rs` 做两件事：

1. 沿 `source_path` 找到应成为当前表行的 `PayloadValue`。
2. 根据当前 message descriptor，把 scalar、enum、bytes 和 Struct 递归追加到该表的 Arrow builders。

它不创建 Parquet 文件，也不决定哪些 message 应生成表。

例如计划要求写：

```text
source_path: events.event.alloc_event.frame_info
```

执行时：

```text
root value
  -> 遍历 events sequence
  -> 选择 event.alloc_event variant
  -> 遍历 frame_info sequence
  -> 每个 Frame 追加一行
```

父表写入后产生的 `row_index` 会按当前 payload 的 path ordinal 暂存；子表写入时取得该索引并写入 `parent_index`。

### 7.4 `table_batch.rs`：直接追加 Arrow builders

每张实际输出表有一个 `TableBuffer`。第一次写入该表时，根据 `ColumnSpec` 创建：

```text
公共列 builders
  source_index
  parent_index
  row_index

数据列 builders
  primitive builder
  Binary builder
  Struct children builders + validity
```

字段值直接进入对应 builder，不生成 `RelationalRow` 或逐行 `Vec<CellValue>` 中间对象。

当一张表达到行数阈值或 estimated bytes 阈值时：

```text
builders
  -> RecordBatch
  -> bounded FIFO
  -> 单个 Parquet writer worker
```

约束：

1. 每张表的 `row_index` 在 `TableBuffer` 中单调递增，flush 后不重置。
2. 同一张表的 batch 按产生顺序写入。
3. 只使用一个后台 Parquet writer，不并行写同一张表。
4. 队列有界，writer 背压会传回关系化线程。
5. `estimated_bytes` 是内存占用估算计数，只用于决定何时 flush，不写入 Parquet，也不出现在 schema。

## 8. Dataset Storage 契约

关系化输出遵守 ADR 0020，不建立自己的 catalog、文件命名或目录管理能力。

关系化 sink 只向 `dataset_writer::DatasetWriter` 提交逻辑表名、Arrow schema 和 `RecordBatch`：

```text
DatasetWriter::begin(target)
  -> begin_table(logical_table_name, schema)
  -> write(batch)*
  -> finish table
  -> finish dataset
```

Dataset Storage 统一负责：

1. 校验逻辑表名和顶层列名。
2. 建立确定性 `tables/<table_name>.parquet` 物理布局。
3. 写入和验证 Parquet。
4. 发布空普通文件 `.kat-dataset`。
5. 解析受管理表并把 canonical Parquet path 交给外层查询。

Dataset 中没有 `catalog.json`、独立 Dataset ID 或关系元数据文件。Parquet schema 是列结构的权威来源，包括 nested `Struct` 和 `Binary`。表关系由第 6 节的稳定表契约与公共关系列表达。

现有 `clock_domain`、`clock_snapshot` 和 `sched_switch` 使用同一个 `DatasetWriter` 写入同一 Dataset。关系化层不感知这些表的时钟业务，Dataset Storage 也不区分 source facts 与通用关系表。

## 9. 当前目录结构

目录按运行时数据流排列：

```text
kat/platform/datasource/
  src/
    materializer.rs                 # 公开 import 入口并组装完整流程

    formats/hitrace/                # 读取 .htrace、解析 envelope、分发 plugin

    decode/profiler/                # typed prost decode 和 ProfilerPluginRoute 注册

    record.rs                       # DecodedPayload / TraceRecord 统一边界
    payload_value.rs                # typed message 到通用 PayloadValue

    relational/
      descriptor.rs                 # 暴露 build-time descriptor 摘要
      rules.rs                      # 关系化规则类型
      plan.rs                       # 生成表计划、合法表名和运行时 parent_table
      plan_exec.rs                  # 缓存并执行 root plan
      table_data.rs                 # 访问 source_path，递归追加列数据
      row.rs                        # ColumnSpec / ColumnType
      table_batch.rs                # Arrow builders、flush、后台 Parquet writer
      sink.rs                       # 接收 DecodedPayload，协调计划和表缓冲

    dataset_writer.rs               # ADR 0020 Dataset Storage 唯一写入口

    domains/                        # 现有 ftrace/native_hook source-fact records
    sinks/arrow/                    # 现有直接 Rust 查询表生成路径，本切片保留

    query.rs                        # 现有 Rust DataFusion 查询入口，本切片不替换

  build/
    relational_descriptor_codegen.rs  # 生成精简 descriptor 摘要
```

`materializer.rs` 同时协调新增关系化输出和现有 source facts，但两者的职责仍然分离：`relational/` 只处理 protobuf 结构，现有 `LongTermHitraceSink` 继续拥有时钟和 ftrace 业务校验。

## 10. 扩展方式

| 新增内容 | 应修改什么 | 不需要修改什么 |
| --- | --- | --- |
| 新 profiler protobuf plugin | 添加 generated proto 覆盖和 `ProfilerPluginRoute` decode 注册。 | 不修改关系化规则、table batch、Dataset Storage。 |
| 已支持 plugin 新增 root message | 注册 typed root，并让 descriptor 摘要包含该 root 可达的 message。 | 不手写表 builder。 |
| proto 新增现有形态字段 | 更新 proto 后重新 build；现有规则自动生成列或表。 | 不修改 decode 路由和 dataset。 |
| proto 出现新的结构形态 | 先在 SDD 中定义结构语义、SQL 输出和验证，再增加一条有限规则。 | 不在 plugin decoder 中临时展开。 |
| 新 text / binary profiler payload | 在 format/decode 边界增加明确 parser 和 root 数据结构，再进入同一关系化边界。 | 不把 parser 塞进 relational 或 dataset。 |
| 新查询方式 | 使用 Dataset Storage 已解析的逻辑表和 canonical Parquet path，交给 Rust 或 Python DataFusion。 | 不扫描物理目录、不重新 decode `.htrace`。 |

## 11. 验证要求

### 11.1 规则契约

必须覆盖：

1. 每个 decoded root 都生成 root table。
2. root table 名没有 `_overview`。
3. scalar、enum、bytes 的列类型和值正确。
4. MessageField 生成 nullable Struct，不生成 message child table。
5. RepeatedScalar 生成 value child table。
6. repeated enum 同时保留 value 和 name。
7. RepeatedMessage 生成 child table，并正确写入父表索引。
8. OneofVariant 跳过 group 中间表，父表保留 variant 名。
9. Struct 内 repeated message 的表名保留完整 proto path，`parent_index` 指向 plan 定义的最近实际父表。
10. 不同 proto path 规范化为同一合法表名时拒绝生成计划。
11. cpu、memory、process、diskio、network、gpu 六类 fixed-result plugin 的 config/data route 均通过公开 Import 入口验证代表字段值；message/repeated 数据同时验证 Struct 或 child table。

### 11.2 Dataset 契约

必须验证：

1. 产物包含合法空普通文件 `.kat-dataset`。
2. Datasource 只向 Dataset Storage 提交逻辑表名，不指定物理文件名。
3. 只为实际写出的逻辑表生成 `tables/<name>.parquet`。
4. Parquet schema 可通过 Dataset Storage resolution 交给 DataFusion。
5. 多次 flush 不改变 row_index、parent_index 或 batch 顺序。
6. 关系化表与现有 `clock_domain`、`clock_snapshot`、`sched_switch` 同时存在。
7. 现有时钟、loss、排序和 thread-continuity 校验继续执行；它们在 writer 打开后、marker 发布前完成。
8. 未知 plugin/section 继续按 ADR 0025 报告；observer、decode、关系化或 source-fact 校验失败均不发布 `.kat-dataset` marker。
9. 覆盖已有目标时，失败不恢复 writer 打开前删除的旧内容，符合 ADR 0020 的破坏式 fail-fast 语义。

### 11.3 查询契约

至少覆盖：

```sql
-- Struct
select meta['name']
from root;

-- RepeatedScalar child table
select value
from batch_native_hook_data_events_stack_map_ip
order by row_index;

-- RepeatedMessage 父子 join
select p.pid, s.path
from memory_data_processesinfo_smapinfo s
join memory_data_processesinfo p
  on s.source_index = p.source_index
 and s.parent_index = p.row_index;

-- OneofVariant 父子 join
select e.tv_sec, a.pid, a.size
from batch_native_hook_data_events_alloc_event a
join batch_native_hook_data_events e
  on a.source_index = e.source_index
 and a.parent_index = e.row_index
where e.event = 'alloc_event';
```

### 11.4 真实数据

至少使用包含 ftrace、memory 和 native_hook 代表结构的真实 `.htrace` 验证：

1. import 成功。
2. 生成表数量和表名可解释。
3. 代表查询返回正确结果。
4. nested Struct 可由 Python DataFusion 读取。
5. 大 trace 内存受表缓冲阈值约束。

### 11.5 本次实现验证证据

最终可审计的代码对象由 PR body 记录 HEAD SHA。本节不嵌入包含自身文档提交的 commit SHA，避免证据在更新本文时自然失效。

代码验证：

```text
cargo test --workspace --all-targets -- --test-threads=1
  结果：231 passed，0 failed，3 ignored。

cargo test -p kat-datasource --test hitrace_import_contract \
  import_decodes_ -- --nocapture
  结果：6 passed，0 failed。

KAT_REAL_HITRACE=<sample> cargo test -p kat-datasource \
  --test hitrace_import_contract real_openharmony_capture_smoke \
  -- --ignored --nocapture
  结果：1 passed，0 failed。
```

真实数据通过公开 `kat import hitrace`、Linux release 二进制和 WSL2 ext4 输入/输出导入：

| 样本 | 结果 | 现有功能共存证据 |
| --- | --- | --- |
| `hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace` | SHA-256 `da6877da3f24db1e4754b9f06bcfb35830fb1fffc2ae827ee306548f2cf9f4b9`；导入成功；7.31s；峰值 RSS 240,700 KiB；9 张表；64,063,947 Parquet bytes。 | `clock_domain`、`clock_snapshot`、`sched_switch`、`trace_plugin_result_clocks_detail` 均存在且非空；共 6 张 `trace_plugin_result*` 关系表，真实 smoke 校验 event 行可通过 `source_index + parent_index` 回到 CPU detail 行。 |

移除 8 条 `DecodedPayload` 延迟缓冲后，在同一 WSL2 ext4 环境进行了五组 baseline/candidate 交叉 A/B。baseline 二进制 SHA-256 为 `0a3b4739c883a21fd5d609ea1de342bccca6ca3db3280e977857a659f1c29868`，candidate 二进制 SHA-256 为 `4c5fab714b3fb13d65ed630582d1bc85887678a0ef10319a0841d8d626246fd3`。baseline 中位数为 6.98s，candidate 中位数为 6.86s，改善 1.72%；五组 candidate 均更快。峰值 RSS 中位数分别为 240,892 KiB 和 240,716 KiB，没有观察到稳定的内存变化。

主样本两侧均输出 9 张表和 64,063,947 Parquet bytes，全部 dataset 文件 SHA-256 一致。`native_heap_full.htrace` 和 `all_memory_full.htrace` 也分别得到相同表数、Parquet bytes 和逐文件 SHA-256。该改动因此保留，收益主要是删除无执行收益的 payload 保留层，同时未改变输出、顺序和父子索引。

使用 Python `datafusion==54.0.0`、`pyarrow==24.0.0` 查询真实样本导入产物：`hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace` 的 nested Struct 可读取 `sched_switch_format.prev_comm/next_comm`；`native_heap_full.htrace` 的 `sym_table` / `str_table` 可读取为 Binary，样本行长度分别为 2,848 / 4,504 bytes；`batch_native_hook_data_events_alloc_event` 的 239,493 行全部可通过 `source_index + parent_index` join 回 `batch_native_hook_data_events` 父表。

## 12. 当前限制

1. decode 仍需要为新 typed root 增加静态 route；不能只靠字符串动态 decode 未编译的 message。
2. 当前 import 单遍读取并在 decode 前打开 writer；覆盖已有目标时，后续失败不发布 marker，也不恢复旧内容。
3. Dataset 不持久化父表元数据；查询者依据稳定转换契约选择父表，并用公共关系列 join。
4. dataset 是可重建查询产物，不承诺跨版本 schema migration。
5. 对新的 proto 结构形态，需要先明确 SQL 表达后再新增规则。
