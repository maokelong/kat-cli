# kat-rs-datasource 关系化转换 Prototype SDD

状态：已合并原 SDD 与执行计划，并同步到当前代码实现状态。本文是该 prototype 的单一阅读入口。

更新时间：2026-07-08。

## 1. 背景

`kat-rs-datasource` 的目标是提供底层 datasource 能力：读取 `.htrace`，decode profiler payload，把 `.proto` 中 SQL 不友好的复杂结构转换成 SQL 友好的关系表，再通过 Parquet + DataFusion 查询。

这次 prototype 验证的新路径是：

```text
.htrace
  -> profiler envelope
  -> typed prost decode
  -> DecodedPayload
  -> descriptor + finite rules
  -> relational rows / dynamic Arrow batches
  -> Parquet dataset
  -> DataFusion query
```

这条路径已经替代 prototype 中旧的 `ArrowSink / ArrowTableSet` 默认 hitrace 查询路径。

## 2. 目标

1. 验证新目录结构能承接现有 `.htrace` reader、profiler envelope dispatch、dataset writer 和 query 入口。
2. 验证 decode 层只负责 typed prost decode，不负责表展开或业务派生。
3. 验证 `kat-rs-datasource` 可以把当前 profiler protobuf payload 机械转换成 SQL 友好的关系表。
4. 验证转换结果可以写成简化 catalog + Parquet dataset，并被 DataFusion 重新打开查询。
5. 验证旧 hitrace 默认表不再生成，prototype 输出 canonical table name。

## 3. 非目标

1. 不保留旧 hitrace 查询表作为默认输出。
2. 不保留旧 `sinks/arrow` 作为新架构边界。
3. 不做长期 dataset 持久化语义、迁移、恢复或原子替换。
4. 不做业务短表名、业务 alias、统计派生、字段筛选或业务语义分析。
5. 不引入 YAML/JSON manifest。
6. 不做 runtime reflection decoder 作为主路径。
7. 不做无限递归 proto 展开。

## 4. 已确认决策

| 问题 | 决策 |
| --- | --- |
| decode 策略 | 统一 typed `prost::Message::decode`，不走 `prost-reflect`。 |
| profiler decoder 注册 | 使用 `ProfilerPluginRoute` 全表生成 decoder，不再为每个 plugin 单独维护注册常量和构造函数。 |
| Runtime descriptor 从哪里来 | build-time 生成精简 descriptor 摘要到 `OUT_DIR`，runtime 使用静态 Rust 数据。 |
| oneof 规则名 | 使用通用名 `OneofVariantTable`。 |
| Catalog 结构 | 简化为 `name/path/format`。 |
| `from_hitrace` 查询路径 | 走临时 Parquet workspace，再通过 DataFusion 查询。 |
| 旧 hitrace 表 | 默认不再输出旧表。 |

## 5. 当前实现架构

### 5.1 ingest / format

位置：

```text
crates/kat-rs-datasource/src/formats/hitrace/
```

职责：

1. 读取 `.htrace` 文件。
2. 解析 profiler section 和 envelope framing。
3. 根据 envelope 的 plugin name / kind 调用 profiler registry。

当前 `formats/hitrace` 不负责 protobuf 业务语义，不负责表展开，也不写 Arrow/Parquet 表。

### 5.2 decode

位置：

```text
crates/kat-rs-datasource/src/decode/profiler/
```

decode 层通过 `ProfilerPluginRoute` 描述每个 profiler plugin 的 config/data payload 类型。

核心结构：

```text
ProfilerPayloadRoute
  root_message
  emit

ProfilerPluginRoute
  plugin_name
  config: Option<ProfilerPayloadRoute>
  data: ProfilerPayloadRoute
```

全表入口：

```text
PROFILER_PLUGIN_ROUTES
profiler_plugin_decoders()
```

当前 route 覆盖：

```text
cpu-plugin       CpuConfig / CpuData
memory-plugin    MemoryConfig / MemoryData
process-plugin   ProcessConfig / ProcessData
diskio-plugin    DiskioConfig / DiskioData
network-plugin   NetworkConfig / NetworkDatas
gpu-plugin       GpuConfig / GpuData
ftrace-plugin    TracePluginResult
nativehook       NativeHookConfig / BatchNativeHookData
hookdaemon       NativeHookConfig / BatchNativeHookData
```

保留 `PluginDecoder` trait。原因是 `formats/hitrace/profiler` 仍只面向 decoder trait 调度，不反向依赖 `decode::profiler::ProfilerPluginRoute`，保持 format 层和 decode 层边界清楚。

### 5.3 record boundary

位置：

```text
crates/kat-rs-datasource/src/record.rs
```

当前 `TraceRecord` 已收敛为：

```text
TraceRecord::DecodedPayload(Box<DecodedPayload>)
```

`DecodedPayload` 是通用 decode 结果容器：

```text
plugin_name
root_message
message: PayloadValue
```

decode 层只把 typed prost message decode 后序列化成 datasource 内部的 `PayloadValue` 通用树。它不展开 repeated/oneof，也不决定表结构。

这里用 `PayloadValue` 是关系化层的中间树表示，目的是让关系化层可以按 descriptor path 遍历取值，而不用给每个 proto message 手写一套取字段代码。它通过 serde visitor 从 typed prost message 构造，但不使用 `serde_json::Value` 作为运行时中间结构。

“遍历取值”指按 proto path 一层层走到目标数据。例如 `MemoryData.processesinfo.smapinfo`：

1. 从 `MemoryData` 的 JSON tree 开始。
2. 读取 `processesinfo` 数组。
3. 对数组里的每个 process，再读取它的 `smapinfo` 数组。
4. 对每个 smap object，提取其中的 scalar 字段，生成 `memory_data__processesinfo__smapinfo` 的一行。

| 表示方式 | 含义 | 当前取舍 |
| --- | --- | --- |
| `PayloadValue` | typed prost decode 后转成 datasource 内部通用树，再按 path 取值。 | 当前使用；字段名保留静态引用，数字保持原始类型，避免完整 JSON value tree 的额外开销。 |
| typed Rust struct | 直接在具体 `CpuData`、`MemoryData` 等结构上取字段。 | 类型更强、性能更好，但每个 message 都要写或生成取字段代码。 |
| `prost-reflect::DynamicMessage` | 用 descriptor 做 runtime reflection decode 和遍历取值。 | 更动态，但引入 runtime reflection decoder；当前明确不作为主路径。 |
| 取字段 codegen | build-time 生成更贴近 proto 的取字段代码。 | 维护成本高，暂不作为当前优化方向。 |

### 5.4 relational expansion rules

位置：

```text
crates/kat-rs-datasource/src/relational/
```

runtime 根据当前出现的 `root_message` 和 build-time descriptor 摘要生成展开计划：

```text
ExpansionPlanItem
  rule
  root_message
  source_path
  source_message
  output_table
```

规则按 descriptor 里看到的源结构分类，而不是按某个 plugin 或当前代码里的 enum 名分类。这样新增 payload 时，先判断字段形态，再决定列、表和边界处理。

术语说明：本文统一使用 `Message` 表示 protobuf descriptor 里的 `message`。在规则解释里可以把 `Message` 理解成一个对象，因为它有自己的字段集合；但分类名和 descriptor 匹配条件统一使用 `Message`，避免把 `Object` 和 `Message` 误读成两种类型。

| 分类 | 中文名 | descriptor 形态 | 默认输出 |
| --- | --- | --- | --- |
| `ScalarField` | 标量字段 | scalar 字段，非 repeated、非 bytes、非 enum | 当前表的一列；root scalar 进入 `<root>` 表。 |
| `EnumField` | 枚举字段 | enum 字段 | 当前表两列：原值列和 `<field>_name`。 |
| `BytesField` | 二进制字段 | bytes 字段 | 当前表的一列，列类型是 Arrow / Parquet binary。 |
| `MessageField` | Message 字段 | 非 repeated 的 message 字段 | 生成一张子表。 |
| `RepeatedScalar` | 重复标量/枚举字段 | repeated scalar / enum 字段 | 生成 value child table；如果元素是 enum，额外生成 `value_name`。 |
| `RepeatedMessage` | 重复 Message 字段 | repeated message 字段 | 生成一张 child table；深层 repeated 继续按层级展开。 |
| `OneofVariant` | 互斥分支 | oneof 内的 scalar / enum / message / bytes variant | 主表记录实际 variant 名；每个实际出现的 variant 生成独立表；bytes variant 以 binary value 列保存。 |

#### 5.4.0 展开公共约定

下面几条是所有展开规则共享的约定，先说明表怎么命名、行怎么追溯、什么时候真正建表。

表命名：

| 场景 | 规则 | 示例 |
| --- | --- | --- |
| root message 自身成表 | 使用 `root_message_snake`。 | `MemoryData` -> `memory_data` |
| 子层级成表 | 用 `__` 连接 proto path。 | `MemoryData.processesinfo.smapinfo` -> `memory_data__processesinfo__smapinfo` |
| 为什么用 `__` | 区分 proto path 层级和普通 snake_case。 | `processesinfo__smapinfo` 中的 `__` 表示层级。 |

直接写成例子就是：MemoryData -> `memory_data`。

oneof 命名：

| 场景 | 规则 | 示例 |
| --- | --- | --- |
| oneof group | 不作为表名层级。 | 不生成 `batch_native_hook_data__events__event`。 |
| oneof 所在主表 | 增加一列，列名是 oneof group name，值是实际 variant 名。 | `event = 'alloc_event'` |
| oneof variant 子表 | 使用最近的真实父表 + variant field name。 | `batch_native_hook_data__events__alloc_event` |

来源键：

| 字段 | 含义 |
| --- | --- |
| `source_index` | 当前行来自第几个输入 trace 文件；当前单文件入口恒为 `0`。 |
| `row_index` | 当前表内递增的行号。 |
| `parent_index` | 指向父表里的 `row_index`，用于 join 回父行；root-level 表行没有父行时为空。 |

建表时机：

| 规则 | 含义 |
| --- | --- |
| descriptor 生成计划 | descriptor 只告诉我们哪些结构可以展开、怎么展开。 |
| 实际数据决定落表 | 本次 materialize 只写入实际产生至少一行的表。 |
| 不生成空表 | 未出现的 repeated message、message field 或 oneof variant 不进入 `catalog.json`。 |

#### 5.4.1 ScalarField

为什么提取为规则：scalar 是 SQL 最自然的列形态。大部分查询、过滤、排序、聚合都依赖 scalar 列，不能把它们藏在 JSON 或 nested struct 里。

规则：被选中为表的 message 中，普通 scalar 字段直接成为该表列。root message 上的 scalar 字段没有父实体行时，进入 `<root>` 表。`<root>` 表只表示 root message 自身字段，不表示聚合或业务概要。

descriptor 匹配：`field.label != repeated`，`field.type` 是 int、uint、float、double、bool、string 等 scalar，且不是 bytes、不是 enum、不是 message。

是否生成子表：不生成子表，只生成列。

样例：

```proto
message MemoryData {
  repeated ProcessMemoryInfo processesinfo = 1;
  uint64 zram = 4;
  uint64 gpu_used_size = 10;
}

message ProcessMemoryInfo {
  int32 pid = 1;
  string name = 2;
}
```

输入数据：

```text
MemoryData {
  zram: 64,
  gpu_used_size: 32,
  processesinfo: [
    { pid: 42, name: "render" }
  ]
}
```

输出表 `memory_data`：

| source_index | row_index | zram | gpu_used_size |
| --- | --- | ---: | ---: |
| 0 | 0 | 64 | 32 |

输出表 `memory_data__processesinfo`：

| source_index | row_index | parent_index | pid | name |
| --- | --- | --- | ---: | --- |
| 0 | 0 |  | 42 | render |

#### 5.4.2 EnumField

为什么提取为规则：enum 在 proto 里是独立类型，但在 SQL 查询里通常先作为可过滤、可分组的离散值。只看原值不利于阅读，补充 enum name 可以提高查询结果可读性；name 来自 proto descriptor，不是业务派生。

规则：enum 字段默认进入当前表，并生成两列：`<field>` 保存 proto enum 原值，`<field>_name` 保存 descriptor 中对应的 enum value name。如果运行时值在 descriptor 中找不到，`<field>` 仍保留原值，`<field>_name` 为空。

descriptor 匹配：`field.type == enum`，且不是 repeated。

是否生成子表：不生成子表，只生成列。

样例：

```proto
message FtraceCpuStatsMsg {
  enum Status {
    TRACE_START = 0;
    TRACE_END = 1;
  }
  Status status = 1;
  string trace_clock = 3;
}
```

输入数据：

```text
TracePluginResult {
  ftrace_cpu_stats: [
    { status: TRACE_START, trace_clock: "boot" },
    { status: TRACE_END, trace_clock: "boot" }
  ]
}
```

输出表 `trace_plugin_result__ftrace_cpu_stats`：

| source_index | row_index | parent_index | status | status_name | trace_clock |
| --- | --- | --- | ---: | --- | --- |
| 0 | 0 |  | 0 | TRACE_START | boot |
| 0 | 1 |  | 1 | TRACE_END | boot |

#### 5.4.3 BytesField

为什么提取为规则：bytes 是 protobuf 明确表达的原始二进制字段。native_hook 的 `sym_table` / `str_table` 这类字段按符号表原始 bytes 理解，不应被误当成 UTF-8 文本。为了保证底层 datasource 不丢失 `.proto` 中已有内容，bytes 字段可以进入关系表，但列类型必须是 binary，而不是 string。

规则：bytes 字段进入当前表的一列，按 Arrow / Parquet binary bytes 原样保存。这里的 “bytes 列” 指 SQL 表中的一个 binary 类型列，可以被 DataFusion 注册和查询；它不是 UTF-8 string 列，也不在关系化阶段生成 base64/hex 派生列、hash 或 length 辅助列。查询结果如果要展示 bytes，由最后一公里输出格式决定显示为数组、hex、base64 或其他形式。

descriptor 匹配：`field.type == bytes`。

是否生成子表：不生成子表，只生成 binary 列。

样例：

```proto
message SymbolTable {
  uint32 file_path_id = 1;
  bytes sym_table = 5;
  bytes str_table = 6;
  int32 pid = 7;
}
```

输入数据：

```text
SymbolTable {
  file_path_id: 9,
  sym_table: <bytes>,
  str_table: <bytes>,
  pid: 42
}
```

输出表 `batch_native_hook_data__events__symbol_table`：

| source_index | row_index | parent_index | file_path_id | sym_table | str_table | pid |
| --- | --- | --- | ---: | --- | --- | ---: |
| 0 | 0 | 0 | 9 | `<bytes>` | `<bytes>` | 42 |

`sym_table` 和 `str_table` 是 binary 列，不是 string 列。

#### 5.4.4 MessageField

为什么提取为规则：Message 字段可以理解成一个对象字段，它不是 scalar 列，有自己的字段集合和可能的后续子结构。把它 inline 到父表会让列名膨胀，也会让“这个字段是否存在”与父行混在一起。

规则：非 repeated 的 message 字段生成一张子表。子表一行对应一个存在的嵌套 Message，并在相同输入 trace 文件的 `source_index` 内通过 `parent_index` 回到父行。

descriptor 匹配：`field.label != repeated`，`field.type == message`，且不属于 oneof variant。

是否生成子表：生成子表。

样例：

```proto
message FtraceEvent {
  uint64 timestamp = 1;
  string comm = 3;
  CommonFields common_fields = 50;
}

message CommonFields {
  uint32 type = 1;
  uint32 flags = 2;
  int32 pid = 4;
}
```

输入数据：

```text
FtraceEvent {
  timestamp: 10,
  comm: "switch_source",
  common_fields: {
    type: 123,
    flags: 1,
    pid: 42
  }
}
```

输出表 `trace_plugin_result__ftrace_cpu_detail__event`：

| source_index | row_index | parent_index | timestamp | comm |
| --- | --- | --- | ---: | --- |
| 0 | 0 | 0 | 10 | switch_source |

输出表 `trace_plugin_result__ftrace_cpu_detail__event__common_fields`：

| source_index | row_index | parent_index | type | flags | pid |
| --- | --- | --- | ---: | ---: | ---: |
| 0 | 0 | 0 | 123 | 1 | 42 |

#### 5.4.5 RepeatedScalar（重复标量/枚举字段）

为什么提取为规则：repeated scalar 和 repeated enum 都是“一行父数据下挂多个单值”。它们和 repeated message 不同，元素本身没有字段集合，不能展开成多列 Message 表；但直接放 array 列会降低 SQL 兼容性，完全丢弃又会损失 proto 内容。

规则：repeated scalar / repeated enum 生成 value child table。每行表示父行下的一个值。普通 scalar 生成 `value` 列；enum 生成 `value` 和 `value_name` 两列，其中 `value_name` 来自 proto descriptor。是否保留顺序由具体字段需求决定；默认只承诺值可查，不把顺序作为业务语义。

descriptor 匹配：`field.label == repeated`，`field.type` 是 scalar 或 enum，且不是 message。

是否生成子表：生成 child table，列至少包含 `source_index`、`parent_index` 和 `value`；如果元素是 enum，再增加 `value_name`。

样例：

```proto
message StackMap {
  uint32 id = 1;
  repeated uint64 frame_map_id = 2;
  repeated uint64 ip = 3;
  int32 pid = 4;
}
```

输入数据：

```text
StackMap {
  id: 7,
  frame_map_id: [11, 12],
  ip: [4096, 8192],
  pid: 42
}
```

输出表 `batch_native_hook_data__events__stack_map`：

| source_index | row_index | parent_index | id | pid |
| --- | --- | --- | ---: | ---: |
| 0 | 0 | 0 | 7 | 42 |

输出表 `batch_native_hook_data__events__stack_map__frame_map_id`：

| source_index | row_index | parent_index | value |
| --- | --- | --- | ---: |
| 0 | 0 | 0 | 11 |
| 0 | 1 | 0 | 12 |

输出表 `batch_native_hook_data__events__stack_map__ip`：

| source_index | row_index | parent_index | value |
| --- | --- | --- | ---: |
| 0 | 0 | 0 | 4096 |
| 0 | 1 | 0 | 8192 |

repeated enum 样例：

```proto
message TraceFilter {
  repeated Status enabled_status = 1;
}

enum Status {
  TRACE_START = 0;
  TRACE_END = 1;
}
```

输入数据：

```text
TraceFilter {
  enabled_status: [TRACE_START, TRACE_END]
}
```

输出表 `trace_filter__enabled_status`：

| source_index | row_index | parent_index | value | value_name |
| --- | --- | --- | ---: | --- |
| 0 | 0 | 0 | 0 | TRACE_START |
| 0 | 1 | 0 | 1 | TRACE_END |

#### 5.4.6 RepeatedMessage

为什么提取为规则：repeated message 是一组对象，也就是一对多关系。如果塞进父表，会变成数组列，SQL 过滤、join、聚合都不友好。

规则：repeated message 字段生成 child table。深层 repeated 不单独定义新规则，继续按同一规则递归到下一层，并用表名里的 `__` 表达 proto path。

descriptor 匹配：`field.label == repeated`，`field.type == message`。

是否生成子表：生成 child table；如果 child message 内还有 repeated message，继续生成更深层 child table。

样例：

```proto
message MemoryData {
  repeated ProcessMemoryInfo processesinfo = 1;
}

message ProcessMemoryInfo {
  int32 pid = 1;
  repeated SmapsInfo smapinfo = 12;
}

message SmapsInfo {
  string path = 4;
  uint64 rss = 6;
}
```

输入数据：

```text
MemoryData {
  processesinfo: [
    {
      pid: 42,
      smapinfo: [
        { path: "/system/lib/libark.so", rss: 512 }
      ]
    }
  ]
}
```

输出表 `memory_data__processesinfo`：

| source_index | row_index | parent_index | pid |
| --- | --- | --- | ---: |
| 0 | 0 |  | 42 |

输出表 `memory_data__processesinfo__smapinfo`：

| source_index | row_index | parent_index | path | rss |
| --- | --- | --- | --- | ---: |
| 0 | 0 | 0 | /system/lib/libark.so | 512 |

#### 5.4.7 OneofVariant

为什么提取为规则：oneof 表示同一位置上只会出现一种 variant。variant 可能是 scalar、enum、message 或 bytes。把所有 variant 字段压进同一张宽表会产生大量空列，也会模糊实际出现的是哪种分支。

规则：oneof 所在 message 的主表增加一列，列名使用 oneof group name，值为实际出现的 variant field name。oneof 内每个实际出现的 variant 生成独立表。表名使用“最近的真实父表 + variant field name”，不包含 oneof group name。scalar variant 表生成 `value`；enum variant 表生成 `value` 和 `value_name`；message variant 表生成该 message 自己的 scalar / enum / bytes 列；bytes variant 表生成 binary `value` 列。主表的 oneof group 列始终记录实际 variant 名。

descriptor 匹配：字段带 `oneof_index`。如果 `field.type == message`，按 message variant 展开；如果 `field.type == enum`，按 enum value table 展开；如果 `field.type` 是普通 scalar，按 scalar value table 展开；如果 `field.type == bytes`，按 binary value table 展开。

是否生成子表：生成子表。父表是 oneof 所在 message 对应的实际输出表，而不是 oneof group 名。例如 `BatchNativeHookData.events.event.alloc_event` 的父表是 `batch_native_hook_data__events`，不是不存在的 `batch_native_hook_data__events__event`。父表通过 `event = 'alloc_event'` 记录该行实际命中的 oneof variant。variant message 内部如果还有 repeated message，继续按 `RepeatedMessage` 展开。

样例：

```proto
message NativeHookData {
  uint64 tv_sec = 1;
  uint64 tv_nsec = 2;
  oneof event {
    AllocEvent alloc_event = 3;
    FreeEvent free_event = 4;
    uint64 mmap_addr = 5;
    EventStatus status = 6;
  }
}

message AllocEvent {
  int32 pid = 1;
  uint64 size = 4;
  repeated Frame frame_info = 5;
}

enum EventStatus {
  TRACE_START = 0;
  TRACE_END = 1;
}
```

输入数据：

```text
BatchNativeHookData {
  events: [
    {
      tv_sec: 1,
      tv_nsec: 20,
      event: {
        alloc_event: {
          pid: 42,
          size: 64,
          frame_info: [
            { symbol_name: "malloc", file_path: "/system/lib/libc.so" }
          ]
        }
      }
    },
    {
      tv_sec: 2,
      tv_nsec: 30,
      event: {
        mmap_addr: 4096
      }
    },
    {
      tv_sec: 3,
      tv_nsec: 40,
      event: {
        status: TRACE_START
      }
    }
  ]
}
```

输出表 `batch_native_hook_data__events`：

| source_index | row_index | parent_index | tv_sec | tv_nsec | event |
| --- | --- | --- | ---: | ---: | --- |
| 0 | 0 |  | 1 | 20 | alloc_event |
| 0 | 1 |  | 2 | 30 | mmap_addr |
| 0 | 2 |  | 3 | 40 | status |

输出表 `batch_native_hook_data__events__alloc_event`：

| source_index | row_index | parent_index | pid | size |
| --- | --- | --- | ---: | ---: |
| 0 | 0 | 0 | 42 | 64 |

输出表 `batch_native_hook_data__events__alloc_event__frame_info`：

| source_index | row_index | parent_index | symbol_name | file_path |
| --- | --- | --- | --- | --- |
| 0 | 0 | 0 | malloc | /system/lib/libc.so |

输出表 `batch_native_hook_data__events__mmap_addr`：

| source_index | row_index | parent_index | value |
| --- | --- | --- | ---: |
| 0 | 0 | 1 | 4096 |

输出表 `batch_native_hook_data__events__status`：

| source_index | row_index | parent_index | value | value_name |
| --- | --- | --- | ---: | --- |
| 0 | 0 | 2 | 0 | TRACE_START |

#### 5.4.8 行关系

位置：

```text
crates/kat-rs-datasource/src/relational/plan.rs
crates/kat-rs-datasource/src/relational/plan_exec.rs
crates/kat-rs-datasource/src/relational/table_data.rs
```

关系化层按 proto path 一层一层展开。每一层只保存该 message 自己的 scalar 字段；如果查询需要上一层字段，就在相同 `source_index` 内通过 `parent_index -> row_index` join 回父表。

以普通 parent-child 结构为例，输入 proto 层级是：

```text
MemoryData
  processesinfo[]
    pid
    smapinfo[]
      path
      rss
```

关系化后会按层级生成表：

```text
memory_data__processesinfo
memory_data__processesinfo__smapinfo
```

子表只保存当前 message 自己的字段。如果查询需要父层字段，就 join 回父表：

```sql
select p.pid, s.path, s.rss
from memory_data__processesinfo__smapinfo s
join memory_data__processesinfo p
  on s.source_index = p.source_index
 and s.parent_index = p.row_index;
```

所有 payload 类型都使用同一套父子层级关系，差异只来自 proto 结构本身。

### 5.5 relational sink

位置：

```text
crates/kat-rs-datasource/src/relational/sink.rs
crates/kat-rs-datasource/src/relational/plan_exec.rs
crates/kat-rs-datasource/src/relational/table_data.rs
crates/kat-rs-datasource/src/relational/table_batch.rs
```

职责：

1. 接收 `TraceRecord::DecodedPayload`。
2. 按 `root_message` 动态补充 expansion plan，并执行对应 plan item。
3. 通过 `PayloadValue` 遍历 `source_path`，取出要写成行的数据和列值。
4. 按表追加 Arrow column builders，并按阈值 flush。
5. 维护每张表自己的 `row_index`。
6. 为 nested child row 写入 `parent_index`。
7. 在 finish 时按表生成 Arrow `RecordBatch`，再写入 Parquet dataset。

关系建模上需要说明的公共字段：

```text
source_index
parent_index
row_index
```

`source_index` 是输入 trace 文件序号；当前单文件入口恒为 `0`。`row_index` 是同一张表内递增的行号，不受其他表影响。`parent_index` 指向父表中的全局 `row_index`，用于 nested child row 回到父行。查询父子关系时应同时使用 `source_index` 和 `parent_index`，避免未来多输入文件时跨文件误 join。

关系化输出不再包含 `record_index` 或 payload sequence 字段。父子 join 不应依赖 payload 顺序，而应使用 `source_index` 和 `parent_index`。`row_index` 只承诺同一张表内唯一递增，不承诺时间顺序；需要时间顺序时，应使用 payload 自带时间字段或后续明确的来源/时间字段。

sink 写入结构：

```text
RelationalDatasetSink
  tables: table -> TableBuffer

TableBuffer
  columns
  builders
  buffered_rows
  next_row_index
  estimated_bytes
  writer: Option<DatasetTableWriter>

push row
  -> 追加到 table buffer
  -> 达到 flush 阈值时生成 Arrow RecordBatch
  -> 写入同一个 Parquet table writer
  -> 重置当前 builders、buffered_rows 和 estimated_bytes

finish
  -> flush 每张表剩余 rows
  -> close 每张表 writer
  -> commit catalog
```

这里不把一张表拆成多个 catalog entry，也不把 catalog path 改成目录。原因是当前 `DatasetTableWriter` 已经支持同一个 Parquet writer 多次 `write(batch)`；先复用这一点即可把内存从 `O(全部关系行)` 降到 `O(所有表当前 buffer 行)`，同时避免同步改 reader、catalog 校验和 DataFusion 注册逻辑。

flush 条件使用双阈值：

```text
rows >= 64K
或
estimated_bytes >= 32MiB
```

`estimated_bytes` 是 `TableBuffer` 的内部估算计数器，只用于判断当前表 buffer 是否需要 flush。它不会写入 Parquet，不会进入 `catalog.json`，也不是需要从 proto descriptor 编译出来的字段。

`rows` 阈值用于处理 ftrace 这类大量小行；`estimated_bytes` 阈值用于处理少量超大 `string` / `bytes` 字段。`estimated_bytes` 不要求精确等于 Rust heap 占用，只作为保守的内存压力近似值：

```text
row fixed overhead
+ table/row public fields overhead
+ 每个 CellValue 的固定开销
+ string/binary 的实际字节长度
```

如果单行本身超过 `estimated_bytes` 阈值，不应报错；该行进入 buffer 后立即单独 flush。这个阈值限制的是 buffer 大小，不限制输入数据合法性。

`row_index` 不能再使用当前 buffer 的 `rows.len()` 计算。因为 flush 后 rows 会清空，同一张表会出现重复行号。每张 `TableBuffer` 需要维护 `next_row_index`：

```text
row_index = next_row_index
next_row_index += 1
```

这样无论一张表 flush 多少次，`row_index` 仍然是同一张表内递增的稳定行号，`source_index + parent_index -> row_index` 的父子 join 语义不变。

`parent_indexes` 的生命周期也应收缩到单个 decoded payload。当前父子关系只在同一个 payload 的展开过程中使用，例如 `MemoryData.processesinfo.smapinfo` 只会回到当前 `MemoryData` payload 的 `processesinfo`。因此每处理完一个 `DecodedPayload` 后可以清空 parent index，避免它随整个 trace 全局增长。

### 5.6 dataset / query

位置：

```text
crates/kat-rs-datasource/src/dataset/
crates/kat-rs-datasource/src/query.rs
```

dataset catalog 已简化为：

```json
{
  "tables": [
    {
      "name": "memory_data",
      "path": "tables/memory_data.parquet",
      "format": "parquet"
    }
  ]
}
```

`catalog.json` 只登记实际存在的表、路径、格式；不登记列、不登记表关系、不承担 schema registry 或 migration 职责。

`TraceDatasource::from_hitrace` 当前走临时 Parquet workspace：

```text
from_hitrace(path)
  -> materialize_hitrace_dataset(path, temp_dataset)
  -> register_dataset_tables
  -> DataFusion query
```

## 6. 当前架构的可扩展性

当前可扩展性是代码级扩展，不是运行时插件系统，也不是自动猜 schema。新增能力时，应先判断它属于哪一种扩展面，再落到对应层级。

| 扩展面 | 例子 | 应该修改什么 | 不应该修改什么 |
| --- | --- | --- | --- |
| 新增输入来源类型 | 新增 txt、jsonl、csv 等输入文件。 | 修改 `materializer` 和 `formats/<source>`，把输入文件读成 datasource 内部可处理的结构化记录。 | 不修改 `relational` 和 `dataset` 来理解具体文件格式。 |
| 新增 payload 编码类型 | `.htrace` envelope 里的 payload 不是 protobuf，而是文本、压缩块或特殊二进制。 | 新增 decode/parser，把 payload bytes 解析成明确结构，再交给后续关系化或写入链路。 | 不修改 proto 字段规则来表示 text/binary payload，也不修改 sink 去解析文本。 |
| 新增 typed proto payload | 新增一个 profiler plugin 或新的 root message。 | 新增 decode plugin 注册和 typed prost decode 覆盖；更新 build-time descriptor 摘要；补 contract test。 | 不修改 dataset/query；不在 decoder 里手写业务表。 |
| 新增关系化结构规则 | 遇到现有规则无法表达的新 proto 结构形态。 | 修改 `relational/rules.rs`、`relational/plan.rs`、`relational/plan_exec.rs`、`relational/table_data.rs` 和测试；先在 SDD 中定义 descriptor 匹配方式、输出表形状和父子关系。 | 不修改 plugin decoder 做结构展开；不把 plugin 特例塞进通用转换层。 |
| 新增写入或性能策略 | 调整 flush、压缩、Arrow builder、Parquet writer 参数。 | 修改 `relational/sink.rs`、`relational/table_batch.rs` 或 `dataset/writer`，并用真实样本验证耗时、RSS 和 dataset size。 | 不修改 decode、规则、表名、列、父子关系或 catalog 语义。 |

## 7. 当前目录结构

```text
crates/kat-rs-datasource/
  build/
    relational_descriptor_codegen.rs   # build-time：从 proto descriptor 生成精简 message/field 摘要

  src/
    materializer.rs                    # 数据流入口：把 .htrace materialize 成关系化 Parquet dataset

    formats/hitrace/
      mod.rs                           # 读取 .htrace，驱动 profiler envelope 流
      profiler/
        envelope.rs                    # profiler envelope 元信息
        framing.rs                     # envelope framing / length-prefix 读取
        payload.rs                     # payload bytes 分类
        registry.rs                    # 按 plugin name / kind 调用 decoder

    decode/profiler/
      mod.rs                           # ProfilerPluginRoute 全表与通用 typed prost decode
      fixed_result/mod.rs              # cpu/memory/process/diskio/network/gpu route
      ftrace/mod.rs                    # ftrace payload route
      native_hook/mod.rs               # nativehook/hookdaemon payload route

    record.rs                          # DecodedPayload：decode 后、关系化前的统一边界
    payload_value.rs                   # typed prost message 经 serde visitor 转成的通用中间树

    relational/
      descriptor.rs                    # runtime 读取 build-time 生成的 descriptor 摘要
      rules.rs                         # 有限转换规则
      plan.rs                          # descriptor + rules + root_message -> ExpansionPlanItem，并生成 canonical table name
      plan_exec.rs                     # 编译并执行 expansion plan，分发到具体表写入
      table_data.rs                    # 按 source_path 找行数据，并把 message 字段追加为列值
      table_batch.rs                   # 维护每张表的 Arrow builders、flush 和 Parquet writer
      sink.rs                          # 消费 DecodedPayload，管理 payload chunk、表集合和生命周期
      row.rs                           # 动态单元格值和列类型

    dataset/                           # 写入/读取 catalog.json 和 Parquet 表文件
    query.rs                           # 注册 dataset 给 DataFusion，并提供查询入口
```

`relational/` 当前保留的几个文件对应数据流里的不同职责：`descriptor.rs` 提供 proto 事实，`rules.rs` 定义匹配规则，`plan.rs` 生成展开计划，`plan_exec.rs` 执行计划分发，`table_data.rs` 负责从 decoded message 取行和列值，`table_batch.rs` 负责 Arrow builders、flush 和 Parquet writer，`sink.rs` 只保留入口与生命周期管理。`row.rs` 只保留动态单元格值和列类型，避免再拆一个过抽象的 value 文件。

与原计划不同的地方：

1. 没有单独保留 `relational/decoded_payload.rs`；`DecodedPayload` 放在 `record.rs`，作为 decode 后、sink 前的统一边界。
2. 没有继续使用 `domains/` 作为新 decode 路径。
3. 没有保留旧 `sinks/arrow` 作为兼容路径。
4. `MessageFieldTable` 规则按普通 message 字段生成子表。
5. profiler decoder 注册已收敛为 `PROFILER_PLUGIN_ROUTES` 全表。

## 8. 验证证据

截至本文更新，已执行：

```text
cargo test -p kat-rs-datasource --test hitrace_architecture_contract
cargo test -p kat-rs-datasource --test proto_contract
cargo test -p kat-rs-datasource --test hitrace_datasource_query
cargo test -p kat-rs-datasource --test plugin_flow_contract
cargo test -p kat-rs-datasource --all-targets
cargo fmt --all --check
```

结果：全部通过。

额外做过源码文本检查，确认旧 profiler decoder 注册入口没有继续出现在 `crates/kat-rs-datasource/src` 中。

## 9. 当前限制和后续问题

1. 当前 prototype 不提供长期 dataset 生命周期承诺；本地 Parquet dataset 仍按可重建查询产物理解。
2. 当前遍历取值基于 `PayloadValue`，依赖 generated serde 字段名和 proto field name 的映射；代码中已有 snake_case / UpperCamel fallback。
3. 当前没有实现 raw/debug payload 表、source_files 表或 profiler envelope provenance 表；`source_index` 先作为关系行里的输入 trace 文件序号。
4. 当前没有实现业务字段筛选、业务 alias、统计派生或查询 API 产品层。
5. 当前 route 表仍是 typed prost 静态表；新增未知 payload 不能只靠字符串配置自动 decode。
6. 当前 catalog 不登记列和表关系；关系仍由表名、公共列、`source_index + parent_index` 和查询约定承接。
7. 当前真实样本已经进入 10 秒内；后续如果新增更大 trace 或更多 payload 后重新超出预算，应重新按第 5.5 节的闭环流程定位瓶颈，不提前引入新的遍历或写入框架。

## 10. 后续小步交付建议

1. 用更多真实 `.htrace` 样本跑当前关系化路径，并把查询结果、峰值 RSS、耗时和表 row count 放入 PR body。
2. 根据真实样本补充 memory/process/ftrace/native_hook 的代表查询。
3. 如果需要支持更多 profiler plugin，先新增 `ProfilerPluginRoute` 和 descriptor 覆盖，再观察关系化规则是否足够。
4. 如果基于 `PayloadValue` 的遍历取值成为真实性能瓶颈，再基于真实 profile 评估更局部的字段索引缓存或 Arrow builder 写入优化，而不是提前重建大框架。
