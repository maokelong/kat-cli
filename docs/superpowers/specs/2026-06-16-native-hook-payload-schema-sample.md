# native_hook payload schema 架构样板

## 1. 背景

当前 datasource 已经把输入格式、domain payload、Arrow sink、catalog/query 拆成独立层级。native_hook 是第一个需要验证这套分层是否真的可扩展的插件：它既有 `NativeHookConfig` 这类配置消息，也有 `BatchNativeHookData -> NativeHookData -> oneof event` 这类事件负载，还包含 `Frame`、`SymbolTable` 等嵌套或二进制字段。

本次改造的目标不是把所有可能的 native_hook 查询模型一次设计完，而是让 native_hook 成为一个明确样板：

- proto payload schema 是主模型。
- oneof 是 payload schema 的一种形态，不是单独的架构层。
- domain 层表达 native_hook 领域记录，但记录枚举和 oneof 分发不再靠人工同步。
- Arrow sink 层负责物理表构建，不承担 native_hook 事件语义选择。
- build 阶段根据 proto schema 生成稳定、可检查的 glue code。

## 2. 目标与非目标

### 2.1 目标

- 从 `native_hook_result.proto` 中读取 `NativeHookData.oneof event`，生成 native_hook 事件映射。
- 根据 oneof 事件生成 `NativeHookRecord` 事件变体和 oneof 到 domain record 的转换逻辑。
- 根据 oneof 事件生成 native_hook direct table builders，避免手写 `NativeHookTableBuilders` 的事件清单。
- 保留 domain-owned 的语义边界：是否落表、表名规则、公共上下文字段仍由 datasource 侧定义。
- 覆盖 `MapsInfo` 和 `SymbolTable`，避免 oneof 中有事件但解析链路静默丢弃。
- 只实现 direct/raw 查询表，不在本次实现 derived table。

### 2.2 非目标

- 不引入运行时 descriptor reflection。
- 不实现通用跨插件的一键 schema 镜像。
- 不把 native_hook 的查询表完全绑定为 proto message 的 1:1 自动镜像。
- 不设计 derived table 的最终形态。
- 不改变 ftrace 的现有解析主路径。

## 3. 目标分层

### 3.1 formats/hitrace/profiler

职责：

- 解析 `.htrace` profiler envelope。
- 识别 plugin payload 的基本外壳信息。
- 把 payload bytes 交给 domain decoder。

边界：

- 不知道 native_hook 事件类型。
- 不决定 native_hook 如何落 Arrow 表。

### 3.2 domains/native_hook

职责：

- 解析 native_hook payload message。
- 表达 native_hook domain record。
- 管理 native_hook 事件公共上下文，例如插件时间戳。

边界：

- 可以知道 native_hook proto schema。
- 不直接构造 Arrow array。
- 不维护手写 oneof 分发表。

### 3.3 sinks/arrow

职责：

- 把 domain record 转换成 Arrow `RecordBatch`。
- 维护通用 Arrow table builder 能力。
- 维护 native_hook 的物理表集合。

边界：

- 可以知道有哪些 native_hook direct tables。
- 不决定 oneof 事件如何从 proto 映射到 domain record。
- 不承担 native_hook payload decode。

### 3.4 catalog/query

职责：

- 注册 Arrow 表。
- 暴露 DataFusion 查询入口。

边界：

- 不关心 native_hook proto。
- 不关心 oneof 分发。

### 3.5 build.rs / generated code

职责：

- 编译 proto。
- 解析 proto payload schema 中可稳定推导的结构。
- 生成重复性 glue code。

边界：

- 只做编译期生成。
- 不引入运行时 schema discovery。
- 不替代 datasource 对查询模型的命名和稳定性承诺。

## 4. 数据流

```text
.htrace bytes
  -> formats/hitrace/profiler
  -> domains/native_hook payload decode
  -> generated oneof dispatch
  -> NativeHookRecord
  -> sinks/arrow native_hook table builders
  -> catalog/query
```

关键点：

- `.htrace` envelope 只负责把 native_hook payload 送到 native_hook domain。
- native_hook domain 负责 decode `NativeHookConfig` 和 `BatchNativeHookData`。
- `BatchNativeHookData.events[].event` 的 oneof 分发由生成代码处理。
- Arrow sink 接收 `NativeHookRecord`，根据 record 变体写入对应 direct table。

## 5. Proto 解析实现

### 5.1 主模型

native_hook 的主模型是 proto payload schema：

- `NativeHookConfig` 表达配置 payload。
- `BatchNativeHookData` 表达批量事件 payload。
- `NativeHookData` 表达单条事件和公共字段。
- `NativeHookData.oneof event` 表达事件种类集合。

oneof 只负责描述“这条事件是哪一种 payload message”，不是独立的数据流层。

### 5.2 编译期推导

build 阶段从 `native_hook_result.proto` 推导：

- oneof 字段名。
- oneof 对应 message 类型。
- prost 生成的 oneof enum variant 名。
- datasource 使用的 record variant 名。
- datasource 使用的 direct table 名。

这些内容以前容易分散在 domain match、sink builder、测试常量中。改造后由同一个 proto oneof 事件清单推导，减少新增事件时的同步成本。

### 5.3 手写保留项

以下内容仍然手写，因为它们是 datasource 的架构决策，不应完全由 proto 自动决定：

- 哪个 payload message 是 native_hook 批量入口。
- `NativeHookConfig` 是否作为独立 record/table。
- `NativeHookData.timestamp` 如何进入公共上下文。
- 表名稳定规则。
- 是否展开嵌套 repeated 字段。
- 是否构造 derived table。

## 6. Arrow 转换实现

### 6.1 direct/raw 表

本次只实现 direct/raw 表：

- 每个 native_hook event message 对应一张查询表。
- 每行包含公共上下文字段和事件 payload 字段。
- 公共上下文字段目前至少包含 plugin event timestamp。

示例表名规则：

- `AllocEvent` -> `native_hook_alloc`
- `FreeEvent` -> `native_hook_free`
- `MemTagEvent` -> `native_hook_mem_tag`
- `RecordStatisticsEvent` -> `native_hook_statistics`
- `SymbolTable` -> `native_hook_symbol_table`

### 6.2 不做 proto 自动镜像

direct/raw 表不是 proto schema 的无条件镜像。原因：

- 查询表需要稳定命名，不能完全受 proto 字段名变化影响。
- 查询表需要追加上下文字段，而这些字段不一定属于事件 message。
- 某些 proto 字段可能适合后续展开为 child table 或 derived table。
- datasource 需要维护查询兼容性，而 proto schema 主要服务序列化兼容性。

## 7. 需要维护的映射代码

| 映射项 | 改造后来源 | 是否生成 | 维护成本 |
| --- | --- | --- | --- |
| proto message Rust 类型 | `.proto` | 自动生成 | 新增字段通常只改 proto |
| serde derives 注入 | build.rs 选择规则 | 半自动生成 | 需要维护哪些 message 可进入 Arrow；字段本身不手写 |
| oneof event 清单 | `NativeHookData.oneof event` | 自动推导 | 新增 oneof 事件后不再同步多处清单 |
| prost oneof variant 名 | oneof 字段名 | 自动推导 | 由 prost 命名规则决定 |
| `NativeHookRecord` 事件变体 | oneof message 类型 + datasource 命名规则 | 自动生成 | 特殊命名规则少量维护 |
| oneof 到 record 分发 | oneof event 清单 | 自动生成 | 新增事件不再手写 match |
| native_hook table builder 清单 | oneof event 清单 + 表名规则 | 自动生成 | 新增事件不再手写 builder 列表 |
| 公共上下文字段 | domain 代码 | 手写 | 低；属于 native_hook domain 语义 |
| 表名稳定规则 | build.rs helper | 手写规则 | 中；需要兼顾可读性和兼容性 |
| nested/child table 展开 | 暂不实现 | 不生成 | 后续按查询需求设计 |
| derived table | 暂不实现 | 不生成 | 后续可在 domain 后处理或 query 层实现 |

## 8. 新增字段后的修改点

### 8.1 事件 message 新增普通字段

预期修改：

- 修改 proto。
- 重新构建项目。

通常不需要修改：

- domain oneof match。
- `NativeHookRecord` enum。
- `NativeHookTableBuilders` 清单。

可能需要修改：

- 如果字段类型不适合 serde_arrow 默认映射，需要补充 serde field attribute。
- 如果字段应展开为 child table，需要新增手写查询模型。

### 8.2 新增 oneof 事件类型

预期修改：

- 修改 proto，在 `NativeHookData.oneof event` 中添加事件。
- 重新构建项目。

通常不需要修改：

- oneof 分发代码。
- record enum。
- direct table builder 清单。

可能需要修改：

- 如果表名需要特殊稳定命名，补充表名规则。
- 如果该事件不应落 direct table，需要显式加入跳过规则。

### 8.3 新增 payload 入口类型

预期修改：

- 修改 domain decoder。
- 明确 payload code 到 decoder 的注册关系。

原因：

- payload 入口类型不是 oneof 内部结构，不能只靠 `NativeHookData.oneof event` 推导。

## 9. 性能分析

### 9.1 Decode 开销

运行时 decode 仍使用 prost 静态代码，不引入 descriptor reflection。oneof 分发是普通 Rust `match`，与手写 match 性能等价。

### 9.2 Arrow 构建开销

Arrow 构建仍按表批量追加 row，再生成 `RecordBatch`。改造减少的是人工映射代码，不改变主要内存拷贝路径。

### 9.3 Query 性能

direct/raw 表保持面向查询的宽表形态，查询性能与当前模型一致。新增 `MapsInfo` 和 `SymbolTable` 后会增加可查询表数量，但不会影响未查询表的 DataFusion 扫描成本。

### 9.4 内存占用

domain record 仍会持有 decode 后的 prost message。新增落表类型会增加对应 builder 的 row buffer。若后续发现 `SymbolTable` 二进制字段过大，可以再设计专门的 binary table 或延迟 materialization 策略。

## 10. 优缺点

### 10.1 优点

- 新增 native_hook oneof 事件时，主要修改点回到 proto。
- domain/sink 不再各自维护一份事件清单。
- 生成代码仍是静态 Rust，性能接近手写实现。
- 架构边界更清晰：decode 在 domain，Arrow 在 sink，重复映射在 build。
- 可以作为后续其他 oneof payload 插件的参考样板。

### 10.2 缺点

- build.rs 会承担更多 schema 推导逻辑。
- 表名规则仍需要 datasource 维护，不能完全交给 proto。
- 对 prost 命名规则有依赖，需要测试保护。
- 复杂字段是否展开仍需要后续查询模型设计。

## 11. 验收标准

- native_hook oneof 事件清单不再由手写静态数组维护。
- `NativeHookRecord` 事件变体由 generated code 提供。
- native_hook oneof 到 record 的分发由 generated code 提供。
- native_hook direct table builders 由 generated code 根据 oneof 事件生成。
- `MapsInfo` 和 `SymbolTable` 不再被 native_hook decode 链路静默跳过。
- 现有 ftrace 查询测试不受影响。
- cargo fmt、cargo test 通过。
