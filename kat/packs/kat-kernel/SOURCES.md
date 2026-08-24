# `kat-kernel` 数据来源

本 PACK 只解释调用方明确提供的已采集数据；它既不从现场设备采集，也不访问本机的 `/proc`。`hitrace` 解释一份已采集的 `.htrace`，`raw_smaps` 则把 SMAPS 文本作为普通日志案例。

## `hitrace`

执行 `kat bind` 或 `kat materialize` 时，通过 `--trace <path>` 提供一份已经采集的 `.htrace`。Binding 只保存路径；查询第一次实际引用 `"kat-kernel".hitrace.<table>` 时，Source 才解析整份 capture，并在本次操作持有的私有临时目录中建立 Parquet TableProvider。同一次操作重复访问表不会重新解析或导出。显式 Materialization 仍走 KAT 的通用 Source 流程，不由本 PACK 直接写 Dataset。

该来源提供两张固定事实表，以及一张仅在 capture 含有受支持且完整的调度事件时出现的表：

### `clock_domain`

| 字段 | Arrow 类型 | 可空 | 含义 |
| --- | --- | --- | --- |
| `clock_domain` | `Utf8` | 否 | capture 中可引用的时钟域。 |
| `clock_type` | `Utf8` | 否 | 时钟域对应的时钟类型。 |
| `ticks_per_second` | `UInt64` | 否 | `clock_value` 的每秒 tick 数；当前为 1,000,000,000。 |

### `clock_snapshot`

| 字段 | Arrow 类型 | 可空 | 含义 |
| --- | --- | --- | --- |
| `snapshot_id` | `UInt64` | 否 | 同一次时钟快照的编号。 |
| `clock_domain` | `Utf8` | 否 | 该观测值所属的时钟域。 |
| `clock_value` | `UInt64` | 否 | 该时钟域中的观测值。 |

### `sched_switch`（可选）

| 字段 | Arrow 类型 | 可空 | 含义 |
| --- | --- | --- | --- |
| `clock_domain` | `Utf8` | 否 | 调度时间戳所属的时钟域。 |
| `clock_value` | `UInt64` | 否 | 调度切换发生时的时钟值。 |
| `cpu` | `UInt32` | 否 | 发生切换的 CPU。 |
| `cpu_switch_sequence` | `UInt64` | 否 | 该 CPU 内按 capture 顺序从 0 开始的切换序号。 |
| `previous_thread_id` | `Int32` | 否 | 被切出的线程 ID。 |
| `previous_thread_name` | `Utf8` | 否 | 被切出的线程名。 |
| `next_thread_id` | `Int32` | 否 | 被切入的线程 ID。 |
| `next_thread_name` | `Utf8` | 否 | 被切入的线程名。 |

损坏的 capture 会让整个 Source Resolution 失败，不发布部分表。合法但暂不支持的 section 或 plugin 继续按既有 Hitrace parser 规则处理，不形成额外事实表。首个切片的 `hitrace` 只接受一份 capture，不支持多份 capture 对比。出现真实的对比 Workflow 后，再扩展同一个 Source 接受多份 capture，并把设备或样本身份作为显式事实字段交给上层分析。

## `raw_smaps`

每出现一次 `--files`，就提供一份已经采集的完整快照，参数出现的顺序决定从 0 开始的 `snapshot_id`。同一路径可以重复提供，每次出现都表示一份独立快照，不会被去重。

执行 `kat bind` 或 `kat materialize` 时，可以在 `--` 后重复传入 `--files <path>`。重放 Binding 时，相对路径以绑定时保存的工作目录为基准；本次显式提供 materialize 参数时，则以命令的工作目录为基准。PACK 不自行搜索文件，也不根据文件内容猜测来源。Workflow 只查询所选 Dataset 提供的 Source tables，不接收这些文件路径。

该来源提供两张只读事实表：

### `snapshots`

| 字段 | Arrow 类型 | 可空 | 含义 |
| --- | --- | --- | --- |
| `snapshot_id` | `UInt64` | 否 | 按输入顺序分配的快照编号。 |
| `source_file` | `Utf8` | 否 | Source Entry 实际收到的文件路径。 |

### `mappings`

| 字段 | Arrow 类型 | 可空 | 含义 |
| --- | --- | --- | --- |
| `snapshot_id` | `UInt64` | 否 | 关联 `snapshots.snapshot_id`。 |
| `start_address` | `UInt64` | 否 | 映射起始虚拟地址。 |
| `end_address` | `UInt64` | 否 | 映射结束虚拟地址。 |
| `permissions` | `Utf8` | 否 | SMAPS 映射头中的四字符权限。 |
| `offset` | `UInt64` | 否 | 文件映射偏移。 |
| `device` | `Utf8` | 否 | SMAPS 映射头中的设备号。 |
| `inode` | `UInt64` | 否 | inode 编号。 |
| `pathname` | `Utf8` | 否 | 映射路径或内核标签；来源省略时为空字符串。 |
| `size_kib` | `UInt64` | 否 | `Size` 指标，单位 KiB。 |
| `rss_kib` | `UInt64` | 否 | `Rss` 指标，单位 KiB。 |
| `pss_kib` | `UInt64` | 否 | `Pss` 指标，单位 KiB。 |

Decoder 机械解析标准映射头，以及每个映射必需的 `Size`、`Rss`、`Pss` 指标。空文件仍会在 `snapshots` 中保留对应记录，`mappings` 则返回具有稳定 Schema 的零行结果。映射头、必需指标或数值损坏时，整张表读取失败，不发布部分结果。其他 SMAPS 属性只在确认是合法属性行后跳过，不形成事实字段，也不做来源一致性或业务完整性校验。

`raw_smaps` 通过 KAT 的 Arrow reader helper 延迟读取文件。查询 `snapshots` 不会解析 SMAPS 内容；首次查询 `mappings` 时才逐个文件解析，并分批产生 RecordBatch。显式 Materialization 可以把两张表或选定子集保存到 Dataset，之后查询已物化事实不再依赖原始文件。

这里用 SMAPS 代表一种普通日志格式，并不是为平台增加内置 Datasource 类型。Decoder 只理解一份原始 SMAPS 文本；如果以后输入是“元数据 + 多个 SMAPS chunks”的大型容器，应在本 PACK 中增加理解该容器的 framer，再把每个 chunk 交给同一个 Decoder，而不是扩展平台协议。
