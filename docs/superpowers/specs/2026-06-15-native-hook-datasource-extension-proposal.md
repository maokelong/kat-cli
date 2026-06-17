# native hook datasource 扩展与统一架构边界提案

## 背景

本提案基于 `maokelong/kat-rs` main 当前提交 `cf08a23`。datasource 当前已经从单链路实现收敛为分层结构：`.htrace` 容器读取在 `formats/hitrace`，profiler envelope 机制在 `formats/hitrace/profiler`，具体 payload 解码在 `domains/*`，pre-sink record stream 由 `record` 承载，Arrow 落表在 `sinks/arrow`，SQL 注册只消费 `catalog::TraceDataset`。

native hook 是该架构收敛后的第一个非 ftrace profiler plugin 扩展。它不只是新增 proto 和表，还会检验当前架构是否能在不污染 ftrace、不扩大全局中心枚举、不让 sink 承担 domain 语义的前提下承载第二个 profiler plugin。

待接入的 native hook proto 以 OpenHarmony developtools_profiler 为事实源：

| 文件 | 作用 |
| --- | --- |
| `D:\新建文件夹\developtools_profiler\protos\types\plugins\native_hook\native_hook_config.proto` | native hook 采集配置 |
| `D:\新建文件夹\developtools_profiler\protos\types\plugins\native_hook\native_hook_result.proto` | native hook 批量事件 payload |

Profiler 当前识别 `nativehook` 和 `hookdaemon` 作为 native hook data envelope，识别 `nativehook_config` 作为 config envelope。`native_hook_result.proto` 的 payload 顶层是 `BatchNativeHookData`，内部包含多条 `NativeHookData`，每条事件通过 oneof 表示 alloc、free、mmap、munmap、mem tag、trace alloc/free、symbol/file/thread/map 辅助映射、symbol table、frame/stack map 和 statistics event。

本提案重新梳理 native hook 接入后的文档逻辑：先给出当前切片结论、目标、非目标、当前架构评估和目标架构，再对比 ftrace 与 native hook 两条流程的优缺点，最后合并两者优点，给出更推荐的统一演进方案。

## 当前切片结论

当前切片的交付目标是：完成 native hook raw/direct 查询面接入，并完成接入所必需的架构边界修正。关键结论如下：

| 议题 | 决策 |
| --- | --- |
| profiler envelope 定位 | `plugin_flow` 不再作为顶层通用 plugin framework，重定位为 `formats/hitrace/profiler` |
| decoder specs 装配 | 由 `.htrace` pipeline 装配 ftrace/native hook decoder specs，registry 机制层不内置具体 decoder |
| native hook domain | 新增 `domains/native_hook`，与 `domains/ftrace` 同级 |
| native hook 语义 | native hook oneof 分支由 `domains/native_hook` 转成 `NativeHookRecord`，sink 不直接理解 protobuf oneof |
| record 扩展 | `TraceRecord` 顶层使用粗粒度 `Ftrace` 和 `NativeHook` domain records，不展开 domain 内部 schema |
| Arrow sink 分层 | `sinks/arrow/table` 放通用 Arrow table 构建，`sinks/arrow/ftrace` 放 ftrace 专用 direct event helper，`sinks/arrow/native_hook` 放 native hook 表物化 |
| build script | native hook proto 进入 prost 编译和 native hook oneof builder 生成，不进入 ftrace event family 生成器 |
| query | 继续只消费 `TraceDataset`，不感知 ftrace/native hook 语义 |

本切片不追求一次性解决所有未来扩展问题。完整 catalog 元信息、TraceStreamer 派生表对齐、跨 domain 的通用生成框架、多 sink 插件系统都不进入当前交付。它们作为推荐演进方向记录在本文后半部分。

## 目标

1. 新增 native hook config/result proto，保留 OpenHarmony developtools_profiler 中 native hook proto 的消息名、字段名和 tag number。
2. 将原顶层 `plugin_flow` 内容重定位为 `formats/hitrace/profiler`，并拆清 envelope、segment、registry 三类机制职责。
3. 明确 `.htrace` pipeline 装配职责，由它决定本次 decode 启用哪些 profiler plugin decoder。
4. 移除 profiler registry 机制层对 ftrace decoder 的内置依赖；registry 只接收外部传入的 decoder specs。
5. 新增 `domains/native_hook`，负责 `nativehook_config`、`nativehook` 和必要时的 `hookdaemon` payload 语义。
6. 用粗粒度 `TraceRecord::Ftrace` 和 `TraceRecord::NativeHook` 连接 domain decoder 与 Arrow sink，native hook 内部用 `NativeHookRecord` 表达 config/direct event 语义，避免全局 `record` 展开 domain 内部 schema。
7. 新增 native hook raw/direct tables，让 config、alloc、free、mmap、munmap、mem tag、statistics 和辅助映射数据可被 DataFusion 查询。
8. 拆分 Arrow sink 内的通用表构建和 ftrace 专用 direct event helper，避免 `table_builder` 继续混合职责。
9. 保持现有 ftrace sched direct tables、`profiler_plugin_data` raw table 和 query 入口行为不变。

## 非目标

1. 不实现 TraceStreamer 的 `native_hook`、`native_hook_frame`、`native_hook_statistic` 派生表语义。
2. 不实现 alloc/free 或 mmap/munmap 生命周期匹配、`current_size_dur`、`all_heap_size`、last caller 修正和 split file 处理。
3. 不实现离线符号化、ELF symbol table reload、callframe 压缩展开、JS/native 混合栈重建或跨事件栈归一化。
4. 不把 native hook 接入 `FTRACE_EVENT_FAMILIES`。
5. 不把 ftrace event family 生成器直接改造成通用 plugin table generator。
6. 不引入 `prost-reflect` 或运行时 protobuf descriptor 反射。
7. 不设计多 sink 插件系统。
8. 不让 `formats/hitrace/file` 或 `query` 感知 native hook 业务语义。

## 当前架构评估

当前架构已经具备接入第二个 profiler plugin 的基础，但直接追加 native hook 会暴露若干层级边界问题。下表按接入 native hook 时需要评估的层来描述：哪些位置是合理的，哪些位置会在扩展时放大成本，以及本切片如何处理。

| 层 | 当前状态 | 直接扩展风险 | 本切片处理 |
| --- | --- | --- | --- |
| `formats/hitrace` | 作为 `.htrace` 容器层方向正确，但文件读取和 decode 编排职责需要显式区分 | 继续追加 plugin 判断会让 format file 层耦合 domain | 保持 `file` 只读容器事实，pipeline 负责 decoder specs 装配 |
| 原 `plugin_flow` | envelope、segment、registry 机制有价值，但顶层位置和名称容易误导 | 容易被理解成全项目 plugin framework，registry 也容易内置具体 domain decoder | 重定位为 `formats/hitrace/profiler`，registry 不内置具体 decoder |
| `domains/ftrace` | ftrace payload 解码边界合理 | native hook 若挂到 ftrace family 会污染 ftrace 抽象 | 保持 ftrace domain 独立，native hook 新增为同级 domain |
| `domains/native_hook` | native hook 需要独立承载 oneof 分支语义、ignored policy 和后续派生状态 | 如果缺少 domain 层，oneof 判断会下沉到 sink | 新增 native hook domain，并把 oneof 到 `NativeHookRecord` 的转换放在 domain |
| `record` | 连接 decoder 与 sink 的方向合理，但全局 enum 有膨胀风险 | 每个 plugin 都把内部事件分支放到顶层会形成中心枚举 | 顶层只加 `NativeHook` 粗粒度 record，domain 内部细分 |
| `sinks/arrow` | sink 位置合理，但原 `table_builder` 混合了通用能力和 ftrace 专用能力 | ftrace `EventMeta` 会被误当成通用 direct event 模型，native hook 表构建也可能继续堆入 `mod` | 拆出 `table`、`ftrace`、`native_hook`，`mod` 只做编排 |
| `build.rs` | prost 编译和 ftrace family 生成在同一文件中 | 误把 native hook 接到 `FTRACE_EVENT_FAMILIES`，或把 ftrace generator 过早通用化 | native hook 只进入 prost 编译；ftrace generator 继续只服务 ftrace |
| `catalog` | 当前足够注册表，但分类较粗 | config、map、statistics、derived 等语义未来会挤压 `TableCategory` | 本切片不升级模型，只在文档和测试中明确 raw/direct 语义 |
| `query` | 只消费 `TraceDataset`，方向正确 | 隐含每张表至少有一个 batch 承载 schema | native hook 空表也生成 schema batch，由测试覆盖 |

这个评估的重点不是否定现有结构，而是明确“新增第二个 profiler plugin 时，哪些边界必须先收住”。其中最关键的是三点：profiler registry 不拥有 domain 列表，native hook oneof 语义不下沉到 sink，全局 `TraceRecord` 不展开每个 plugin 的内部 schema。

## 目标架构

| 层级 | 职责 | 边界要求 |
| --- | --- | --- |
| `formats/hitrace/file` | 读取 `.htrace` header、section 和 body | 不识别任何 plugin name，不引用 domain、proto payload 或 Arrow |
| `formats/hitrace/profiler/envelope` | 建模 `ProfilerPluginData` envelope，识别 data/config | 不依赖 `domains/*` |
| `formats/hitrace/profiler/segment` | 解 profiler section 里的 length-prefixed protobuf message 流 | 不依赖 `domains/*` |
| `formats/hitrace/profiler/registry` | 提供 decoder trait、spec 和 registry 调度机制 | 不内置 ftrace/native hook decoder |
| `formats/hitrace` pipeline | 编排一次 `.htrace` decode，选择本次启用的 decoder specs | 可以依赖 profiler 机制层、domain decoder specs 和 record sink |
| `domains/ftrace` | 解 `ftrace-plugin` payload，产出 ftrace record | 不写 Arrow，不注册 SQL |
| `domains/native_hook` | 解 native hook config/data payload，产出 `NativeHookRecord` | 拥有 native hook oneof 语义，不写 Arrow，不注册 SQL |
| `record` | 承载 pre-sink 粗粒度 records | 不展开 domain 内部 oneof schema |
| `sinks/arrow/table` | 通用 Arrow table builder 和 `TraceTable` 物化 | 不含 ftrace/native hook domain 语义 |
| `sinks/arrow/ftrace` | ftrace direct event metadata 与 direct event table helper | 只处理 ftrace event record 的 Arrow 物化辅助 |
| `sinks/arrow/native_hook` | native hook raw/direct tables 的 Arrow 物化 | 消费 `NativeHookRecord`，不直接 match protobuf oneof |
| `sinks/arrow/mod` | 编排 profiler raw table、ftrace tables 和 native hook tables，输出 `TraceDataset` | 不堆具体表字段逻辑 |
| `catalog` | 描述可注册 table dataset | 不承载 decode stream 协议 |
| `query` | 注册 `TraceDataset` 并执行 SQL | 不依赖 format/domain/sink 内部细节 |

目标架构按职责分为四段：format pipeline、profiler envelope 机制、domain decoder、sink/query。依赖不会被消除，而是移动到正确位置：pipeline 是装配层，可以知道具体 domain decoder；profiler envelope 是机制层，不能知道任何具体 domain；domain 层拥有业务语义；sink 层拥有物理表和 Arrow 兼容细节。

## ftrace 流程对比

| 步骤 | 当前流程 | 优点 | 缺点 |
| --- | --- | --- | --- |
| 输入格式 | `.htrace` section 进入 profiler envelope，按 `ftrace-plugin` 识别 payload | 没把 ftrace 当成独立 input format，位置准确 | 无明显问题 |
| decoder 装配 | `.htrace` pipeline 装配 `FTRACE_PLUGIN_DECODER` | registry 不反向依赖 ftrace，机制层干净 | pipeline 会知道所有 domain decoder；这是装配层可接受依赖 |
| payload decode | `TracePluginResult` 展开 CPU detail 和 ftrace event | 解码路径简单，能保留 ftrace 原始 event 上下文 | domain 语义较薄，仍把大的 ftrace event proto 交给 sink 继续拆 |
| record stream | ftrace event 被包装为 `FtraceRecord` 后进入 `TraceRecord::Ftrace` | 与 native hook 的粗粒度 domain record 形状一致，仍能复用现有 sched direct tables | ftrace domain 语义仍偏薄，但当前不需要为了统一而引入复杂 spec |
| Arrow 落表 | build.rs 生成 ftrace event table builders | ftrace event family 很多，自动生成能减少重复并降低扩展成本 | 生成器强绑定 ftrace event family 假设，不能直接复用给 native hook |
| 表模型 | 每个 ftrace event message 基本对应一张 direct table | 规则稳定，适合自动生成和回归验证 | 更接近 proto event family 镜像，domain 查询表语义没有显式声明 |
| query | DataFusion 注册 `TraceDataset` 后查询 | query 层干净，和 format/domain 解耦 | 无明显问题 |

ftrace 的核心优势是规则性强：`TracePluginResult -> FtraceEvent -> optional event family messages -> direct tables` 这一形状非常适合生成器。它的主要不足是 domain 语义偏薄，但这不是当前 native hook 接入必须解决的问题；不应为了统一而强行引入重型 table spec。

## native hook 流程对比

| 步骤 | 当前流程 | 优点 | 缺点 |
| --- | --- | --- | --- |
| 输入格式 | `.htrace` section 进入 profiler envelope，按 `nativehook`、`hookdaemon`、`nativehook_config` 识别 payload | 没误建 `formats/native_hook`，因为 native hook 当前不是独立输入格式 | 无明显问题 |
| decoder 装配 | `.htrace` pipeline 装配 native hook decoder specs | 与 ftrace 一致，registry 保持机制层 | plugin 增多后 pipeline specs 列表会变长，后续可抽装配函数 |
| payload decode | `BatchNativeHookData` 展开批内 `NativeHookData`，oneof 分支转为 `NativeHookRecord` | native hook 语义归 domain，sink 不直接理解 protobuf oneof | `MapsInfo`、`SymbolTab` 当前跳过，后续应显式记录支持或忽略策略 |
| record stream | `TraceRecord::NativeHook` 承载 native hook domain record | 全局 `TraceRecord` 不随 native hook oneof 膨胀，边界清楚 | native hook 的内部 record 比 ftrace 更语义化，但不要求 ftrace 立即补齐同等语义层 |
| Arrow 落表 | `sinks/arrow/native_hook` 手写 tables | 表语义清楚，能避免 proto 镜像误导 | 手写重复多，新增表或字段成本高 |
| 表模型 | 面向查询的 raw/direct tables，如 alloc、free、mmap、statistics 和辅助映射表 | SQL 好查，事件表边界清晰 | 不是从 proto 裸结构直接推导，需要维护映射规则 |
| query | DataFusion 注册 `TraceDataset` 后查询 | query 层干净，和 native hook 语义解耦 | 无明显问题 |

native hook 的核心优势是语义归位：oneof 分支在 domain 层转换成 `NativeHookRecord`，sink 只负责物化。它的主要不足是表构建目前手写，扩展成本比 ftrace 高。

## 两条流程暴露出的边界问题

| 问题 | 影响 | 当前处理 | 后续推荐 |
| --- | --- | --- | --- |
| 顶层 `plugin_flow` 命名和位置不准确 | 容易被误解为全项目 plugin framework | 重定位为 `formats/hitrace/profiler` | 保持 profiler envelope 机制层，不承载 domain 列表 |
| registry 内置 ftrace decoder | 机制层反向依赖 domain | 移除内置 decoder，由 pipeline 装配 specs | 新增 plugin 时只改装配层或装配函数 |
| `table_builder.rs` 同时含通用和 ftrace 专用能力 | `EventMeta` 这类 ftrace 语义被伪装成通用 table builder | 拆为 `sinks/arrow/table` 和 `sinks/arrow/ftrace` | 后续 domain 的 sink helper 都放独立文件 |
| native hook sink 直接 match protobuf oneof | sink 承担 domain 语义，后续派生逻辑容易下沉到 Arrow 层 | oneof 到 `NativeHookRecord` 的转换上移到 `domains/native_hook` | 复杂派生、归一化和 ignored policy 都归 domain |
| `TraceRecord` 容易随 domain 增多而膨胀 | 全局中心 enum 会越来越大 | ftrace 和 native hook 都只占一个顶层 domain record | 后续新增 domain 也遵循一个 domain 一个顶层 record |
| ftrace generator 和 ftrace event family 强绑定 | 自动生成能力难复用于 native hook | 当前保持 ftrace 专属 generator，native hook 使用独立 oneof-driven 生成 | 后续只有出现真实重复时才抽共享生成 helper |

这些问题的共性是：语义、装配、物理表和机械生成需要分开。domain 层决定“是什么”，sink 层决定“如何落 Arrow”，build.rs 只负责“如何重复生成”。

## 推荐的统一架构边界

更推荐的长期边界如下：

| 层级 | 推荐职责 |
| --- | --- |
| `formats/hitrace/file` | 只读 `.htrace` 容器，不识别 plugin |
| `formats/hitrace/profiler` | 只表达 `ProfilerPluginData` envelope、segment、registry 机制 |
| `formats/hitrace/pipeline` | 装配 profiler decoder specs，驱动 decode，保留 raw profiler records |
| `domains/ftrace` | 解 ftrace payload，产出 `FtraceRecord` |
| `domains/native_hook` | 解 native hook payload，产出 `NativeHookRecord`，明确 oneof 分支支持和忽略策略 |
| `record` | 只放粗粒度 `ProfilerPluginData`、`Ftrace`、`NativeHook` 等跨层 record |
| `sinks/arrow/table` | 通用 Arrow table builder |
| `sinks/arrow/ftrace` | ftrace records 到 Arrow tables 的物化 |
| `sinks/arrow/native_hook` | native hook records 到 Arrow tables 的物化 |
| `build.rs` | 负责 prost 编译和机械生成；native hook 后续可按 oneof 分支生成重复 builder 代码 |
| `catalog/query` | 只消费 `TraceDataset`，不感知 format/domain 细节 |

统一后的关键原则：

1. domain 层拥有语义。ftrace domain 知道哪些 ftrace event 形成哪些 domain records；native hook domain 知道 alloc、free、mmap、statistics、map、frame、stack 的语义和支持策略。
2. sink 层拥有物理表。Arrow 字段、row struct、空 batch、serde rename、DataFusion 兼容属于 sink。
3. build.rs 只做机械生成。它不从 proto 裸结构直接决定查询语义，也不把 native hook 塞进 ftrace event family。
4. record 层只做粗粒度跨层传输。domain 内部 oneof 或 event family 不应被摊平到全局 `TraceRecord`。

## 更好的统一流程

统一流程的目标不是让 ftrace 和 native hook 在每一层都长得完全一样，而是让两者在同一条职责边界上扩展。两边不同时，优先选择能承载更复杂语义、同时不牺牲已有自动化能力的边界。

| 步骤 | 推荐统一流程 | 为什么这样做 | 两边不同时的选择 |
| --- | --- | --- | --- |
| 1. 读取输入 | `formats/hitrace/file` 只产出 `.htrace` section/body | `.htrace` section 是文件格式事实，plugin payload 语义不是文件层职责 | native hook 当前不是独立输入格式，所以不新建 `formats/native_hook`；如果未来出现独立 native hook 文件，再新增对应 format |
| 2. 解析 envelope | `formats/hitrace/profiler` 解析 `ProfilerPluginData` 和 data/config envelope | profiler envelope 是 `.htrace` 内部承载机制，属于 format 下的 profiler 子格式，不属于 ftrace 或 native hook domain | ftrace 过去让 registry 间接依赖 domain decoder，native hook 接入后会放大反向依赖；因此 registry 只保留机制能力 |
| 3. 装配 decoder | `.htrace` pipeline 或外层装配处组合所有 profiler plugin decoder specs | 装配处知道输入格式、plugin name 和启用顺序，registry 不应该内置 domain 列表 | ftrace 当前装配经验可保留；native hook 也按同一装配入口加入，避免每个 domain 自己注册到机制层 |
| 4. domain decode | domain decoder 把 payload 转成 domain-owned record | protobuf 只描述传输结构，domain record 描述当前 datasource 能承诺的语义 | ftrace 规则简单，可先包装为 `FtraceRecord::Event`；native hook oneof、config、map、statistics 更复杂，必须在 domain 层解释 |
| 5. record stream | `TraceRecord` 只承载 coarse domain records | 全局 record 是跨 domain 流水线边界，不应展开每个 domain 的内部事件种类 | ftrace 不再暴露 `FtraceEventRecord` 顶层变体；native hook 不把 oneof 分支膨胀到全局 enum |
| 6. direct 表映射 | native hook 按 oneof 分支到 direct/map/statistics 表的固定映射生成 builders | ftrace 保留 event family 生成能力，不被 native hook 复杂度拖累 | hi profile proto 已经把 alloc/free/mmap/trace alloc 等分支分清，不需要额外重型 spec |
| 7. Arrow 物化 | sink 保留 native hook generic event row/table helper，重复 builders 由 build.rs 生成 | Arrow builder、serde rename、DataFusion table 注册是物化细节，应由 sink 或 generated code 承担 | 保留 ftrace 的生成优势；减少 native hook 手写重复，但不先抽跨 domain 框架 |
| 8. SQL 查询 | query 只注册 `TraceDataset` | query 层只面对 dataset/catalog，不知道 ftrace/native hook 的 decode 细节 | 两边都不在 query 层引入 plugin 分支，避免新增 domain 时继续污染查询入口 |

这个流程结合了两边优点：ftrace 的规则性和生成能力、native hook 的 domain 语义归位。它避免两种极端：一是全部手写导致扩展成本高；二是把 `BatchNativeHookData` 整体 proto mirror 成难查的宽表。

根据 hi profile 的 `native_hook_result.proto`，`NativeHookData` 的 oneof 已经区分了 `alloc_event`、`free_event`、`mmap_event`、`statistics_event`、`trace_alloc_event`、`trace_free_event` 和各类 map 分支。因此当前采用薄映射：domain 决定 oneof 分支是否支持、落到哪个表、是否跳过 `MapsInfo`/`SymbolTable`，字段投影沿用 prost message 的 serde/serde_arrow schema 和少量上下文字段。复杂的 domain-owned table spec 不作为近期目标。

## 表模型与生成策略

需要区分两种表模型：

| 模型 | 含义 | 优点 | 缺点 | 适用性 |
| --- | --- | --- | --- | --- |
| proto schema 自动镜像 | protobuf message 长什么样，表尽量长什么样 | 扩展快，和 upstream proto 对齐直观 | oneof、batch、repeated nested 字段会让 SQL 难查，容易误导为完整语义支持 | 适合调试原始结构，不适合作为稳定查询 API |
| 面向查询的 raw/direct 表 | 按事件语义和查询需求拆表，补充必要上下文列 | SQL 可查、可排序、可过滤，表语义清楚 | 需要维护映射规则，不能完全从 proto 裸结构推导 | 适合作为当前 datasource 查询面 |

native hook 当前选择面向查询的 raw/direct 表，因为 `BatchNativeHookData -> NativeHookData oneof` 是传输结构，不适合整体镜像成一张 SQL 表。但 oneof 内部的单个事件消息已经很接近 direct 表，可以按分支生成或维护独立表。

`NativeHookTableBuilders` 当前从 native hook oneof 分支和一份很薄的策略表生成，而不是从完整的 domain-owned table spec 生成。策略表只表达：

| 内容 | 归属 |
| --- | --- |
| oneof 分支到表名的映射 | `domains/native_hook` 或 native hook sink 生成配置 |
| 哪些 proto 分支当前忽略或保留 raw | `domains/native_hook` |
| 上下文字段、repeated 字段处理、serde rename | `sinks/arrow` 或 sink-oriented generated code |
| 重复 builder、push、into_tables 样板 | `build.rs` 机械生成 |

这样保留 native hook 当前的语义清晰度，同时避免把“生成 builder”升级成不必要的跨 domain 架构工程。

## Native Hook 当前接入设计

### Proto 组织

新增 `crates/kat-rs-datasource/proto/native_hook/native_hook_config.proto` 和 `crates/kat-rs-datasource/proto/native_hook/native_hook_result.proto`。文件内容从 OpenHarmony developtools_profiler 对应 proto 迁移，保留消息名、字段名和 tag number。

迁移时只做必要格式调整：package 使用 `kat.native_hook`；保留 `optimize_for = LITE_RUNTIME`；删除或不依赖 Java package；不修改 upstream 字段编号；不合并 `NativeHookConfig` 和 `BatchNativeHookData`。

### Decoder 语义

native hook decoder 支持 `nativehook_config` config envelope 和 `nativehook` data envelope。`hookdaemon` 使用同一 native hook payload 语义时，由 pipeline 为同一 decoder 逻辑装配第二个 plugin name。

config envelope 解码为 `NativeHookRecord::Config`。data envelope 解码为 `BatchNativeHookData`，遍历批内 `NativeHookData`，并在 `domains/native_hook` 内将 oneof 分支转换为 `NativeHookRecord` 的 direct event 或辅助映射 record。`MapsInfo` 和 `SymbolTable` 当前不进入 direct/raw 查询表，必须作为显式忽略策略保留在 domain 层，而不是由 sink 隐式跳过。

decoder finish 生命周期在当前切片不输出派生数据。后续如果实现 TraceStreamer 派生表，finish 可用于 flush 缓冲事件或完成归一化，但状态仍归 `domains/native_hook` 管。

### Arrow 查询面

当前切片交付 raw/direct tables，而不是 TraceStreamer 派生表：

| 表 | 来源 | 定位 |
| --- | --- | --- |
| `native_hook_config` | `NativeHookRecord::Config` | config envelope 的可读配置快照 |
| `native_hook_alloc` | native hook alloc record | malloc direct event |
| `native_hook_free` | native hook free record | free direct event |
| `native_hook_mmap` | native hook mmap record | mmap direct event |
| `native_hook_munmap` | native hook munmap record | munmap direct event |
| `native_hook_mem_tag` | native hook mem tag record | memory tag direct event |
| `native_hook_statistics` | native hook statistics record | statistics direct event |
| `native_hook_file_path_map` | native hook file path map record | id 到 path 的辅助映射 |
| `native_hook_symbol_map` | native hook symbol map record | id 到 symbol 的辅助映射 |
| `native_hook_thread_name_map` | native hook thread name map record | id 到 thread name 的辅助映射 |
| `native_hook_frame_map` | native hook frame map record | compressed frame 辅助映射 |
| `native_hook_stack_map` | native hook stack map record | stack id 到 frame/ip 的辅助映射 |
| `native_hook_trace_alloc` | native hook trace alloc record | fd/thread/gpu 等资源申请 direct event |
| `native_hook_trace_free` | native hook trace free record | fd/thread/gpu 等资源释放 direct event |

alloc、free、mmap、munmap 表保留事件直接字段和统一事件元信息，例如事件时间、批内顺序、plugin name、envelope name、section offset、pid、tid、addr、size、thread name id 和 stack id。重复 `frame_info` 当前不承诺完整 call stack 查询语义；后续如需支持，应新增 frame detail 表或 derived stack 表设计。

TraceStreamer 的 `native_hook`、`native_hook_frame`、`native_hook_statistic` 表是经过状态机、字典、线程进程映射和符号化处理后的查询面。当前切片不承诺这些表名和语义。后续如需对齐，应新增单独设计，明确 direct tables 到 derived tables 的转换规则。

### 错误处理与时间

native hook decode 错误应带上 profiler section offset、envelope name、version 和 sample interval，保持与 ftrace decoder 相同的错误上下文风格。

unknown oneof 分支在 proto3/prost 中通常表现为 event 为空或 unknown fields 被忽略。当前切片跳过这类事件，同时保留 raw `profiler_plugin_data`。不能 decode 的 `BatchNativeHookData` payload 应返回错误，因为它表示已知 plugin 的 payload 损坏。

timestamp 先按 `tv_sec` 和 `tv_nsec` 生成原始纳秒值。TraceStreamer 会把 native hook realtime 转成 primary trace time；kat-rs 当前没有 clock filter 层，本切片不引入跨 clock 对齐。trace time normalization 另行设计。

## 演进路线

推荐按以下顺序演进，而不是一次性引入过大抽象：

1. 当前切片：完成 profiler envelope 重定位、registry 解耦、native hook domain record、native hook raw/direct tables、Arrow sink 文件级拆分，并对齐 hi profile 当前 native hook proto。
2. 当前统一流程切片：把 ftrace 顶层 record 收敛为 `TraceRecord::Ftrace(FtraceRecord)`，让 ftrace 与 native hook 在 record 层形状一致。
3. 当前生成切片：为 native hook 建立 oneof-driven 的轻量 builder 生成，移除手写 `NativeHookTableBuilders` 和各类手写 Row。
4. 后续：如果 ftrace 和 native hook 的生成逻辑出现真实重复，再抽共享生成 helper；不要先设计跨 domain 的重型 spec-driven generator。
5. 后续：在 catalog 中增加更细的 table category 和元信息，以区分 raw、direct、map、derived、statistics 等查询面。

该路线避免两类风险：直接把 native hook 当 ftrace event family 会混淆事实层次；直接从 proto mirror 生成 SQL 表会让查询语义不清。

## 测试与验证

需要新增或更新以下测试：

1. proto contract 验证 `NativeHookConfig`、`BatchNativeHookData`、`AllocEvent` 和 `RecordStatisticsEvent` 能通过 prost encode/decode。
2. profiler envelope contract 验证 config envelope、data envelope、unknown plugin 和 raw `profiler_plugin_data` 保留行为。
3. architecture contract 验证 `formats/hitrace/profiler` 不依赖 `domains/*`，registry 机制层不内置 ftrace/native hook decoder 列表。
4. architecture contract 验证 `formats/hitrace/file` 不包含 native hook plugin name、native hook proto 类型、Arrow builder 或 table builder。
5. architecture contract 验证 `sinks/arrow/table` 不包含 ftrace/native hook domain 语义，`sinks/arrow/ftrace` 承载 ftrace direct event helper。
6. architecture contract 验证 native hook oneof 分支判断位于 `domains/native_hook`，而不是 `sinks/arrow/native_hook`。
7. native hook decoder 行为测试验证构造的 `ProfilerPluginData` payload 能产出 native hook domain records。
8. Arrow sink 行为测试验证 native hook 空表可注册，包含 alloc/free/statistics 样例时对应 direct table 可查询。
9. query 行为测试覆盖空表 schema 契约，避免新增 table builder 因没有数据 batch 而无法注册。
10. 回归测试验证现有 `profiler_plugin_data` 和 sched direct tables 查询结果不变。

完整实现 PR 的验证命令仍应包含 `cargo fmt --all -- --check`、`cargo test --workspace` 和 `cargo clippy --workspace --all-targets -- -D warnings`。

本提案文件自身的检查项是：无未完成标记、无互相矛盾的阶段描述、无代码片段、无把 native hook 塞进 ftrace family 的描述。
