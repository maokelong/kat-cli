# Plugin Integration Convergence Architecture

## 状态

Review draft。

本文补充 `2026-06-16-payload-schema-primary-architecture.md` 中关于“新增 plugin 维护成本”的部分，专门讨论如何分阶段收敛新增 plugin 的接入点。

新的核心发现是：**payload shape 的大部分信息应该从 `.proto` 或 protobuf descriptor 推导。阶段三默认不引入 manifest；只有当 descriptor 和项目约定都推不出来，或必须保持历史查询兼容时，才补极薄 override。**

本文的第一约束：任何 manifest/override 都不能决定领域模型。领域模型先由 payload schema/descriptor 推导；override 只做接线补充或兼容补丁。

因此，本文不再把阶段三描述为“手写 payload shape manifest”或“新增 policy manifest”。更准确的模型是：

```text
.proto / descriptor
  -> schema truth
  -> message、field、oneof、repeated、nested、enum、bytes 等结构事实

项目约定
  -> 默认命名和表生成约定
  -> package 到 domain/table prefix 的命名约定、常见 timestamp 约定

optional overrides
  -> 只处理 descriptor/约定推不出来的兼容和歧义
  -> 例如必须保留的旧表名、非约定 timestamp 语义、无法自动判断的拆表选择
```

## 1. 当前问题

当前项目已经把 `.htrace` 容器解析、profiler envelope、domain decoder、Arrow sink 拆开，但 ftrace 和 native hook 仍然在若干主流程入口处显式接线。

新增一个 profiler plugin，通常会触碰这些位置：

| 位置 | 当前职责 | 新增 plugin 是否会碰 |
| --- | --- | --- |
| `domains/<plugin>` | payload schema decode 和 domain policy | 会 |
| `formats/hitrace/mod.rs` | 组装 hitrace profiler decoder specs | 会 |
| `record.rs` | 顶层 `TraceRecord` domain variant | 会 |
| `sinks/arrow/mod.rs` | 持有各 domain table set 并分发 record | 会 |
| `build.rs` / `build/*` | proto 编译、serde derive、生成 table/record helper | 会 |
| tests | 架构、proto、query 契约 | 会 |

这里要区分两类变化：

1. **已有 plugin 的字段级 schema 变化。**
   这类变化应该尽量接近“改 proto + regenerate + 测试确认”。
2. **新增一个全新 plugin。**
   这类变化天然需要 domain policy、运行时输入绑定策略、表选择策略和测试，不应假装完全自动化。

当前真正需要收敛的是第二类变化中的机械接线：主流程入口、顶层 record enum、Arrow table set 聚合和 decoder specs 装配。

## 2. Schema Truth 与 Policy Truth

### 2.1 `.proto` 能提供什么

`.proto` 或 protobuf descriptor 应该作为 schema truth。它可以提供：

| 信息 | 是否应从 proto 推导 | 示例 |
| --- | --- | --- |
| package | 是 | `kat.native_hook` |
| root message 候选 | 是 | `BatchNativeHookData`、`TracePluginResult` |
| repeated field | 是 | `BatchNativeHookData.events` |
| oneof field | 是 | `NativeHookData.event` |
| oneof variant 列表 | 是 | `alloc_event -> AllocEvent` |
| message 字段列表 | 是 | `AllocEvent.pid`、`AllocEvent.size` |
| nested/repeated/bytes/enum 字段类型 | 是 | `repeated Frame frame_info`、`bytes sym_table` |
| serde/Arrow schema 候选 | 是 | prost struct + serde_arrow |

这些信息不应该在 manifest 里再手写一遍。否则会出现两份真相源：proto 里一份，manifest 里一份。

### 2.2 `.proto` 不能提供什么

`.proto` 不知道运行时接入和业务策略。下面这些不能简单当作 payload schema 事实，但也不意味着一定要新增 manifest；优先按项目约定推导，推不出来再手写 override 或 domain code：

| 信息 | 推荐处理方式 |
| --- | --- |
| profiler envelope plugin name | `ProfilerPluginData.name` 是运行时分发信息，不属于阶段三 payload schema 推导 |
| config envelope 名称 | 例如 `nativehook_config` 是 envelope 层命名约定，不属于 payload shape |
| 一个 proto 是否接入 `.htrace` | proto 只定义 schema，不定义输入格式 |
| table prefix | 可以默认从 proto package/domain 推导；只有历史查询兼容需要 override |
| timestamp 语义 | `tv_sec + tv_nsec` 可作为项目约定推导；非约定字段才需要 override |
| ftrace cpu meta | parent message 的 scalar 字段可按结构约定进入 event meta |
| repeated child 是否拆表 | 查询价值和表设计决策，不是 schema 事实 |
| raw payload 是否保留 | source/sink 的全局溯源策略，不属于 payload generator |

### 2.3 正确分工

推荐分工：

```text
proto descriptor inference
  负责发现 payload shape
  负责枚举 message / oneof / repeated / field type
  负责生成候选 record/table/schema

conventions / optional overrides
  优先按 package、message、field 命名约定推导
  只在多 root、历史 alias、旧表名、非约定 meta 语义时补充
```

阶段三默认不需要 manifest。若出现 override，它不应该重复列出所有字段和 oneof variant，只能补充 descriptor 和项目约定都无法表达的东西。

## 3. 收敛原则

1. **`.proto` 是 schema 真相源。**
   阶段三必须优先使用 protobuf descriptor 或结构化 proto parser，避免维护第二份 schema manifest。

2. **默认不引入 manifest。**
   阶段三不依赖 plugin name/envelope name 决定领域模型；这些属于 profiler envelope 分发。root message、表名前缀、timestamp policy 先尝试从 proto descriptor 和项目约定推导。只有推导失败、存在多义性、或需要历史查询兼容时，才允许极薄 override；message 字段和 oneof variant 不应手写。

3. **接线自动化不等于 payload 自动化。**
   阶段二首选生成 decoder specs、domain table set 这类接线代码；`TraceRecord` 只有在 coarse domain record 形状稳定后才作为可选生成项。阶段三才讨论按 payload shape 生成 domain/table 样板。

4. **不把所有 plugin 强行压成 oneof。**
   native hook 是 protobuf `oneof` event stream；ftrace 不是 protobuf `oneof`，而是 message field event stream。两者可以共享轻量 variant 映射策略，但不能被描述成同一种 payload 模型。CPU、memory、process 这类 fixed result plugin 应按 root result message 和 repeated child fields 建模。

5. **只保留真正的 domain policy。**
   阶段三不把 plugin name、unknown payload、raw 保留、repeated child 默认投影当作 per-plugin policy。descriptor 和项目约定能推出默认行为时就直接生成；只有查询语义、历史兼容或多义性无法自动判断时，才保留少量人工决策。

6. **每阶段都必须能独立落地和验证。**
   阶段一不依赖阶段二；阶段二不依赖阶段三。

## 4. 阶段一：收敛主流程装配点

### 目标

让 `.htrace` 主流程和 `ArrowSink` 主入口不再直接知道 ftrace/native hook 这两个具体 domain。

阶段一只移动接线位置，不改变运行时模型，不改变生成代码语义，不改变 `TraceRecord` 形状。

### 当前形态

```text
formats/hitrace/mod.rs
  -> 直接引用 FTRACE_PLUGIN_DECODER
  -> 直接引用 NATIVE_HOOK_PLUGIN_DECODER
  -> 直接引用 HOOK_DAEMON_PLUGIN_DECODER

sinks/arrow/mod.rs
  -> 直接持有 FtraceTableSet
  -> 直接持有 NativeHookTableSet
  -> 直接 match TraceRecord::Ftrace / TraceRecord::NativeHook
```

### 目标形态

```text
formats/hitrace/mod.rs
  -> domains::hitrace_plugin_decoder_specs()

sinks/arrow/mod.rs
  -> DomainTableSets::new()
  -> domain_tables.push_record(record)
  -> domain_tables.into_tables()
```

新增文件：

```text
src/domains/registry.rs
  -> hitrace_plugin_decoder_specs()

src/sinks/arrow/domain_tables.rs
  -> DomainTableSets
```

### 阶段一后的新增 plugin 改动面

新增 plugin 仍然需要改：

```text
domains/<plugin>/*
domains/registry.rs
record.rs
sinks/arrow/domain_tables.rs
build.rs / build helper
tests
```

但不再需要改：

```text
formats/hitrace/mod.rs
sinks/arrow/mod.rs
```

### 阶段一不做

1. 不生成 `TraceRecord`。
2. 不生成 `DomainTableSets`。
3. 不引入 integration manifest。
4. 不引入任何 payload shape manifest。
5. 不合并 ftrace/native hook 的 payload 模型。

## 5. 阶段二：收敛机械接线，不定义领域模型

### 目标

新增 plugin 时，优先减少 hitrace decoder specs 装配和 `DomainTableSets` 分发代码的手写成本。

`TraceRecord` 可以继续手写，直到 coarse domain record 形状足够稳定。只有当它和 `DomainTableSets` 的重复模式都被真实新增 plugin 验证过，才考虑一起生成。

阶段二关注“哪个 domain 接到哪个 decoder/table set”，仍不自动理解 payload 内部结构。

### Integration Manifest 概念

阶段二 manifest 是接线 manifest，不是 schema manifest，也不是领域模型 manifest。

下面示例包含 `record_variant` / `record_type`，表示“若启用 `TraceRecord` 生成时需要的接线信息”。它们不描述 payload 字段，也不决定 payload schema。

```rust
PluginIntegration {
    domain: "ftrace",
    record_variant: "Ftrace",
    record_type: "domains::ftrace::FtraceRecord",
    table_set_type: "ftrace_event_table_builders::FtraceTableSet",
    decoder_specs: &[
        "domains::ftrace::FTRACE_PLUGIN_DECODER",
    ],
}

PluginIntegration {
    domain: "native_hook",
    record_variant: "NativeHook",
    record_type: "domains::native_hook::NativeHookRecord",
    table_set_type: "native_hook_table_builders::NativeHookTableSet",
    decoder_specs: &[
        "domains::native_hook::NATIVE_HOOK_PLUGIN_DECODER",
        "domains::native_hook::HOOK_DAEMON_PLUGIN_DECODER",
    ],
}
```

这份 manifest 可以先是 Rust build helper 中的静态数据，不需要一开始引入 TOML/YAML。

### 生成内容

阶段二首选生成：

```text
generated_hitrace_plugin_registry.rs
  -> hitrace_plugin_decoder_specs()

generated_domain_table_sets.rs
  -> DomainTableSets
  -> new()
  -> push_record()
  -> into_tables()
```

可选生成：

```text
generated_trace_record.rs
  -> enum TraceRecord
  -> trait TraceRecordSink 或等价 pre-sink 接口
```

这一步不是阶段二的前置要求。若 `TraceRecord` 仍处在试探期，保留手写更符合“小步交付”原则。

阶段二后，新增 plugin 主要改：

```text
integration manifest
domains/<plugin>/*
build helper，如需新 proto 或新 table generator
tests
```

不再手写：

```text
domains/registry.rs
sinks/arrow/domain_tables.rs
formats/hitrace/mod.rs
sinks/arrow/mod.rs
```

如果启用了 `TraceRecord` 生成，`record.rs` 也可以退出新增 plugin 的手写路径；否则它仍是阶段二内可接受的手写边界。

### 阶段二边界

阶段二 manifest 不应该包含：

```text
oneof variant 列表
message 字段列表
repeated child 字段列表
Arrow 字段 schema
```

这些属于 proto descriptor inference。

阶段二 manifest 可以包含：

```text
domain 名称
record variant 名称（仅在启用 TraceRecord 生成时）
record type（仅在启用 TraceRecord 生成时）
table set type
decoder specs
```

这些是系统接线信息。

### 阶段二进入条件

满足以下条件后再做阶段二：

1. 阶段一完成并稳定。
2. 至少准备接入第三个 plugin，或者已经能明确新增 plugin 的重复接线成本。
3. `DomainTableSets` 的形状已经稳定；若准备启用 `TraceRecord` 生成，则 `TraceRecord` 形状也应被真实新增 domain 验证过。

## 6. 阶段三：Proto Descriptor Inference + Convention Overrides

### 目标

对标准 payload shape，进一步减少 domain decoder 和 Arrow table 样板代码。

阶段三的核心不是手写 payload shape manifest，也不是新增一份 policy manifest，而是：

```text
从 proto descriptor 推导 payload shape
+ 用项目约定推导默认接线和命名
+ 仅在必要时补极薄 override
```

### 输入模型

```text
protobuf descriptor
  -> schema facts

project conventions
  -> default domain/table/timestamp/envelope decisions

optional overrides
  -> ambiguity and compatibility decisions
```

以 native hook 为例，阶段三不需要先定义一份 `PluginPolicy`。多数信息可以直接推导或按约定得到：

| 信息 | 推荐来源 | native hook 情况 |
| --- | --- | --- |
| domain | proto package 命名约定 | `kat.native_hook` -> `native_hook` |
| root message | descriptor shape 推导 | `BatchNativeHookData.events` 指向 event item |
| table prefix | domain 命名约定 | 默认 `native_hook` |
| timestamp policy | 字段命名约定 | `tv_sec + tv_nsec -> ns` |

阶段三不使用 `ProfilerPluginData.name` 判断 payload shape。如果当前实现仍依赖 name 做分发，那是 plugin registry 的实现边界，应在阶段一/二的分发收敛里处理，不进入阶段三 payload schema 推导。

然后这些应从 proto 推导：

```text
BatchNativeHookData.events 是 repeated field
NativeHookData 是 event item message
NativeHookData.event 是 oneof
AllocEvent / FreeEvent / ... 是 oneof payload messages
alloc_event / free_event / ... 是 oneof field names
payload message 字段进入 Arrow schema
```

### Payload shape 分类

阶段三至少应区分四类。前两类都属于更宽泛的 `VariantEventStream`，但它们的 proto 形态不同：

| Shape | 发现方式 | 适用 plugin | 生成潜力 |
| --- | --- | --- | --- |
| `ProtoOneofEventStream` | root repeated item 中包含 protobuf `oneof` | native hook | 高 |
| `MessageFieldEventStream` | event item 中多个 message 字段作为互斥 payload 候选 | ftrace | 中高 |
| `FixedResultMessage` | root message 中有 repeated child fields | CPU、memory、process、network 等 | 中 |
| `SpecialPayload` | descriptor 无法表达完整语义或非 proto | bytrace、hiperf、文本或特殊二进制 | 低，保留手写 |

硬规则：不得把 `MessageFieldEventStream` 描述成 protobuf `oneof`。它只能说是复用轻量 variant 映射策略，而不是复用同一种 proto payload shape。

### ProtoOneofEventStream 推导：native hook

对于 native hook：

```proto
message BatchNativeHookData {
    repeated NativeHookData events = 1;
}

message NativeHookData {
    uint64 tv_sec = 1;
    uint64 tv_nsec = 2;
    oneof event {
        AllocEvent alloc_event = 3;
        FreeEvent free_event = 4;
    }
}
```

可自动发现：

```text
root message: BatchNativeHookData
event repeated field: events
event item message: NativeHookData
oneof field: event
variant: alloc_event -> AllocEvent
variant: free_event -> FreeEvent
```

默认可以按项目约定得到：

```text
table prefix: native_hook
timestamp policy: tv_sec + tv_nsec -> ns
```

阶段三不定义 unknown event policy：

```text
descriptor 中存在的 oneof variant / message field 都生成候选表
当前 descriptor 不认识的未来字段不会进入本轮生成表
是否全局保留 raw payload 由 source/sink 溯源策略决定
```

### MessageFieldEventStream 推导：ftrace

对于 ftrace：

```proto
message TracePluginResult {
  repeated FtraceCpuDetailMsg ftrace_cpu_detail = 2;
}

message FtraceCpuDetailMsg {
  uint32 cpu = 1;
  repeated FtraceEvent event = 2;
}

message FtraceEvent {
  uint64 timestamp = 1;
  int32 tgid = 2;
  string comm = 3;
  SchedSwitchFormat sched_switch_format = 2417;
}
```

可自动发现：

```text
TracePluginResult.ftrace_cpu_detail 是 repeated field
FtraceCpuDetailMsg.event 是 repeated event field
FtraceEvent 没有 oneof；除公共 meta 字段外的 message fields 可作为 event payload 候选
SchedSwitchFormat 可映射为 sched_switch direct table
```

默认可以按结构和命名约定得到：

```text
FtraceCpuDetailMsg.cpu 是 parent scalar meta，进入 EventMeta
FtraceEvent.timestamp/tgid/comm 是 event scalar meta，进入 EventMeta
message field name 决定 event family/table name，例如 sched_switch_format -> sched_switch
所有 message payload fields 都生成 direct table 候选
```

阶段三不决定：

```text
profiler plugin name: ftrace-plugin
运行时 envelope 如何路由到 ftrace decoder
是否人为限制只导出部分 direct table
```

### FixedResultMessage 推导

对于 fixed result plugin，descriptor 可以发现：

```text
root message
scalar fields
repeated child fields
child message type
nested/list/bytes 字段
```

默认可以按 descriptor 和结构约定得到：

```text
root message: 优先选择插件 payload 解码目标；若 descriptor 中唯一 top-level result message 可自动确定
scalar fields: 进入 root table
repeated message child: 生成 child table 候选
repeated scalar/list/bytes: 默认保留为 list/binary column
parent/sample id: 由 traversal path 生成稳定 parent row id、child index
root table 名称: package/domain + root message 命名约定
child table 名称: root table + repeated field name 命名约定
```

只有默认推导失败或需要兼容旧查询时，才需要 override：

```text
多个 root message 候选
某个 repeated message child 必须保留 nested/list
某个 repeated scalar/list 必须拆成独立 child table
旧表名/旧列名兼容
特殊 parent/sample id 语义
```

### SpecialPayload

特殊 payload 不应强行套 generator。它们可以只复用：

```text
PluginEnvelope
decode_payload 或专用 parser
TraceRecordSink
MessageTableBuilder / EventTableBuilder
```

### 阶段三边界

阶段三不应该让 optional override 变成第二份 proto：

不写：

```text
AllocEvent fields
FreeEvent fields
oneof variant 完整列表
Frame 字段列表
```

只有默认推导失败、存在多义性、或需要历史兼容时才写 override：

```text
root message override
legacy table name override
non-convention meta/timestamp override
split-table override
```

### 阶段三进入条件

满足以下条件后再做阶段三：

1. 已经确认至少两种 payload shape 都有生成价值；不要只为 native hook 的 protobuf `oneof` 做通用 generator。
2. 如果阶段三覆盖 `FixedResultMessage`，能从真实需求中确认 root/child table 的查询价值。
3. 至少两类 payload shape 的手写代码出现稳定重复。
4. 对非默认表投影、兼容查询表的边界已有单独决策。
5. build 阶段已经能读取 protobuf descriptor 或可靠结构化 proto AST，而不是继续依赖临时文本匹配。

## 7. 三阶段后的新增成本变化

| 阶段 | 新增 plugin 需要改 | 不再需要改 |
| --- | --- | --- |
| 当前 | format 装配、domain、record、sink、build、tests | 无 |
| 阶段一 | domain、domain registry、record、domain table sets、build、tests | `formats/hitrace/mod.rs`、`sinks/arrow/mod.rs` |
| 阶段二 | integration manifest、domain、record（若未启用生成）、build 策略、tests | domain registry、domain table sets、format/sink 主入口；启用 TraceRecord 生成后也不再手写 record |
| 阶段三 | proto、少量 domain policy、tests | 标准 shape 的大部分 domain/table 样板和字段级 schema 手写 |

关键变化：

```text
阶段二降低接线成本
阶段三降低 schema 展开成本
```

两者不能混成一个新的配置真相源。

## 8. 推荐路线

推荐顺序：

1. **先做阶段一。**
   小步收敛主流程入口，行为不变。

2. **阶段一稳定后评估阶段二。**
   如果第三个 plugin 接入前已经明显感到 `TraceRecord` / `DomainTableSets` 重复，再引入 integration manifest/codegen。

3. **阶段三等待真实 plugin 形态。**
   尤其是 fixed result plugin 出现前，不设计 child-table generator。

4. **阶段三开始前先建立 descriptor inference。**
   优先使用 protobuf descriptor set 或结构化 proto parser，不把 schema 写进 manifest。

## 9. 与既有文档的关系

本文不替代 `2026-06-16-payload-schema-primary-architecture.md`。那份文档定义 payload schema 是主模型，以及 variant/fixed result/special payload 的分层原则。

本文补充的是新增 plugin 的接入点收敛路线，并修正一个重要表述：

```text
旧表述：
  payload shape manifest

新表述：
  proto descriptor inference + project conventions + optional overrides
```

后续应回到 payload schema 文档中修正 5.4：

1. 把“新增字段”和“新增 plugin”拆成两个维护成本模型。
2. 明确字段级 schema 变化应从 proto 自动传导。
3. 明确阶段三默认不需要 manifest；override 只处理 descriptor/约定推不出来的兼容和歧义。
4. 把 ftrace 从 protobuf `oneof` 示例中移出，改为 `MessageFieldEventStream` 示例。

## 10. Review 问题

1. 阶段二是否先只生成 decoder specs / `DomainTableSets` 接线，把 `TraceRecord` 生成作为可选后续？
2. 是否同意 ftrace 归类为 `MessageFieldEventStream`，而不是 `ProtoOneofEventStream`？
3. 阶段三是否应明确禁止手写 oneof variant 和字段列表？
4. 阶段二的 integration manifest 是否应先放在 Rust build helper 中，而不是引入外部配置文件？
5. 阶段三开始前是否必须先改用 protobuf descriptor，而不是当前的轻量文本解析？
6. 新增 plugin 的目标维护成本是否应定义为“proto + 少量 domain policy + tests”，而不是“只改 proto”？
