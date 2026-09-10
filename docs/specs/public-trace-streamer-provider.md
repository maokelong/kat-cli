# 公共 Trace Streamer Provider 轻量设计

关联：[Issue #274](https://github.com/maokelong/kat-cli/issues/274)。

## 已确认的目标

将 Trace Streamer Provider 的实现、声明和来源 guide 提升为平台公共能力，供 PACK 直接使用；保持接口与实现尽量简单。沿用 ADR-0081（公共 Provider 所有权，关联 [Issue #269](https://github.com/maokelong/kat-cli/issues/269)） 的公共 Provider 所有权与独立 inspection 模型，本次是其首个 Ftrace 切片之外的后续范围。

公共 Provider 必须覆盖“原始 trace → SQLite → 查询”。解析器可执行文件路径由调用方通过参数提供。guide 从 [上游 Trace Streamer 文档](https://gitcode.com/openharmony/developtools_smartperf_host/tree/master/smartperf_host/trace_streamer/doc) 总结，说明公共接口实际支持的能力。

同时保留直接打开已有 SQLite 的入口，以承接现有正式 PACK 并避免重复解码；两个入口共用同一套只读查询能力。

解析器固定使用 `<executable> <source> -e <sqlite-path>` 命令合同。调用方只提供可执行文件路径，不增加命令模板或额外参数透传。Provider 使用参数列表启动进程，不经 shell 拼接命令。

使用原始 trace 入口时，Provider 在初始化期间完成 SQLite 准备，成功构造后即可查询；当前工作环境已有对应解码结果时直接复用，不重复解码。不采用调用方先构造、再显式调用 `decode()` 的两阶段使用方式，也不在首次查询时延迟解码。

## 沿用的物化决定

按 [ADR-0077](../adr/0077-analysis-session-groups-multi-workflow-state.md)，Workflow 传入的 `ctx.datasource_root` 已指向 `KAT_DATA_HOME/sessions/<session-id>/materializations/`。Provider 按 `Path(source).stem` 定位其直接子目录，遵守已有名称校验规则；不使用进程当前目录，不扫描其他位置猜测结果，不新增缓存索引或来源 hash。

首次物化完整关闭并校验后以 no-replace 方式发布。同一 Session 的已发布槽位不可原位替换；复用必须检查可读性和本次来源合同，目录存在本身不证明可以复用。损坏或不兼容时报错，不删除后重新解码；新内容使用新 stem 或新 Session。并发发布沿用候选隔离和胜者校验规则。

ADR-0077 中 `dp.open()` 与 Parquet relation 的具体检查针对 Parquet backend；本 Provider 的 SQLite backend 使用 SQLite 读取与校验，不要求先转成 Parquet。SQLite 在 source-stem 目录中的文件布局与最小准入检查在本规格中细化。

## 当前代码依据

- 两个正式 OpenHarmony PACK 中的 `TraceStreamerSQLiteProvider` 接收已有 SQLite 路径并执行只读查询，返回 `dp.Table`。
- `dataprovider-pack` 示例中的 `TraceStreamerProvider` 接收 `source`、`executable`、`workspace`，通过 `decode()` 调用外部程序导出 SQLite，再执行只读查询。
- 示例包含删除既有 workspace 的行为；不能据此认定公共 Provider 已确定目录所有权与复用合同。

## Guide 范围

guide 是精简的 Trace Streamer SQL 编写参考，不包含 Provider 调用示例。聚焦常用表及关联键、时间字段语义和容易写错的查询条件；具体内容依据上游文档核对后总结，所需表字段和时间规则直接总结在 guide 中，不引用外部链接，也不复制完整数据字典。Provider 构造、解析器参数与物化生命周期属于作者接口文档，不塞进 SQL guide。分析策略仍由各 Workflow 拥有。

## 已确认的公共接口

使用一个 `TraceStreamerProvider` 类，两个构造参数组互斥：

```python
TraceStreamerProvider(source=..., executable=..., workspace_root=ctx.datasource_root)
TraceStreamerProvider(sqlite_path=...)
```

统一使用 `query(sql, schema=..., params=...) -> dp.Table`。沿用已有查询合同：只读 SQLite SQL、显式 PyArrow 结果 Schema、列名与顺序精确匹配、命名参数绑定。零行查询仍返回有明确 Schema 的 Table。

## 实现细化

以下是依据既有代码和 ADR 收敛的实现选择，不增加新的产品入口：

- 公共类位于 `kat.dataprovider.trace_streamer`，声明名称沿用 `trace-streamer-sqlite`，接入已有公共 inspection。
- 解码产物为 `<workspace_root>/<source-stem>/trace.db`。先检查已发布槽位，命中后不启动解析器；没有物化时才检查解码所需输入并启动外部程序。
- 首次导出至独立候选目录，检查退出码、数据库为普通文件、SQLite `quick_check` 成功且至少有一个非系统 relation，再按已有 no-replace 约定发布。复用时执行同样的数据库准入检查；不固定表白名单或新增版本兼容层。失败只清理本次候选，不修改已有槽位。
- 路径遵守 ADR-0077 的普通目录与文件约束。已有 SQLite 入口只读使用调用方指定文件，不复制到物化目录，也不负责删除它。
- 子进程使用解析器所在目录作为工作目录，source 和输出路径使用绝对路径。上游从相对路径 `config/config.json` 读取配置；配置与解析器配套部署，不增加单独配置参数。
- 保留可用于排查的进程退出信息与解析错误；不增加自动重试、解析器下载或安装管理。

## 已确认的迁移范围

本次同时迁移 `kat-openharmony-thread-cpu-time`、`kat-openharmony-critical-path` 和示例 `dataprovider-pack`。两个正式 PACK 继续使用已有 SQLite 入口，示例使用初始化解码入口；保持各自分析行为与 Output 不变。消费方直接使用公共 Provider，删除重复的来源实现、声明和来源 guide；分析专用 SQL 与策略留在各自 PACK，不随来源实现上移。

## 验收条件

后续实现应验证真实 trace 经指定解析器导出 SQLite 后可查询并返回 `dp.Table`；解析失败不会被当作可用数据库；公共 inspection 无需选择 PACK 即可读取声明和 guide；迁移的 Workflow 保持预期 Output。

补充覆盖两组参数混用或缺失、命中物化时解析器不被调用、损坏物化不被覆盖、并发发布不破坏胜者、已有 SQLite 不被修改、SQL 写操作被拒绝、零行与 Schema 不匹配。公共声明和 guide 应随 wheel 交付，三个消费方均须通过相应回归。

本文设计与 SQL guide 草稿已同步到 Issue #274；本地实现与实际验证记录见下文。

## SQL guide 草稿

以下正文是待随公共 Provider 交付的精简 guide。依据本机上游文档副本核对；未声称已验证远端 master 的最新内容。

### Trace Streamer SQL

使用 SQLite 方言。先查询 `sqlite_schema` 的 `name`、`sql` 确认实际表和列；不同采集内容、解析器版本不保证表结构完全相同。关联使用内部 ID，不把系统 `pid`、`tid` 当作内部键。

#### 常用表

| 表 | 字段与关联 |
| --- | --- |
| `process`、`thread` | `pid`、`tid` 是系统编号，`name` 是名称；`thread.ipid = process.id` 表达线程所属进程 |
| `sched_slice` | `ts`、`dur` 是调度片段的开始和持续时间，`cpu` 是执行 CPU；`itid` 关联 `thread.id` |
| `thread_state` | `ts`、`dur`、`state` 表达线程状态区间；`itid` 关联 `thread.id`。`R` 是可运行，不能当作正在运行；CPU 执行时间优先查询 `sched_slice` |
| `callstack` | `ts`、`dur`、`name` 描述调用，`parent_id` 关联父调用的 `id`。同步调用的 `callid` 指向线程；异步调用带 `cookie`，`callid` 指向进程、`child_callid` 指向线程，`depth` 仅对同步调用有意义 |
| `frame_slice` | `type=0` 为实际帧，`type=1` 为期望帧；`ts`、`dur` 描述帧区间，`ipid`、`itid` 关联进程和线程，`callstack_id` 关联调用。`dur` 缺失表示数据不完整 |
| `native_hook` | `ipid`、`itid` 关联进程和线程；`event_type` 区分 `AllocEvent`、`FreeEvent`、`MmapEvent`、`MunmapEvent`，`addr` 是地址，`heap_size` 是事件涉及的内存大小 |

Native Hook 的 `start_ts`、`end_ts`、`dur` 描述分配的活跃区间；`all_heap_size` 是该时刻的活跃内存总量，堆事件与映射事件分别统计。不要把不同事件类型的 `heap_size` 全部相加解释成存活内存，也不要把逐事件的 `all_heap_size` 累加。

#### 时间与查询条件

- 数据库中的事件时间通常已转换到 BOOTTIME；`datasource_clockid(data_source_name, clock_id)` 记录的是来源原始时钟，不能据此对数据库时间再次换算。需要核对时钟对齐时，查看 `clock_snapshot(clock_id, ts, clock_name)`。
- BOOTTIME 包含系统休眠时间，MONOTONIC 不包含；两者都不是墙上时间。跨库或跨来源比较前确认时间单位和对齐依据，不直接比较来源不明的整数。
- Ftrace、Native Hook、Hilog、FPS 使用事件内时间；内存、网络、CPU、进程、磁盘和 HiSysEvent 的周期数据使用插件上报时间，两者不一定表示同一时刻。
- 对完整、有效的持续区间，窗口交集条件为 `ts < :end AND ts + dur > :start`；窗口内时长为 `MIN(ts + dur, :end) - MAX(ts, :start)`。先排除缺失或无效时长，瞬时事件用 `ts >= :start AND ts < :end`。
- 先按目标进程、线程、事件类型和时间范围筛选，再关联与聚合；一对多关联会放大行数，避免重复累计时长或内存。
- 输出明确列名、别名和顺序，与结果 Schema 一致；筛选值使用命名参数，避免拼接 SQL。

## 实现与验证（2026-09-09，Windows）

公共 Provider、随 wheel 交付的 SQL guide、无需 PACK 的 CLI/Runtime inspection 和三个消费方迁移已实现。公共发现复用主线已合入的公共 Ftrace Provider 入口，列表同时包含 `ftrace-text` 与 `trace-streamer-sqlite`；本次不另建 CLI 或 Runtime 协议。

- `cargo test -p kat-cli`：167 passed，11 ignored；`cargo clippy -p kat-cli --all-targets -- -D warnings` 与 `cargo fmt --all -- --check` 通过。
- 在安装实际构建 wheel 的 Python 3.14 环境中运行 `python -B -m pytest kat/platform/workflow/tests -q --tb=short`：175 passed、3 skipped、294 subtests passed。随后补充进程启动失败清理与拒绝写入后可继续查询的回归；公共 Provider 与公共 inspection 定向重跑为 26 passed、1 skipped（符号链接创建权限不足）。
- 两个正式 PACK 经真实 Runtime 执行：线程 CPU 时间 6 passed，关键路径 28 passed；关键路径的专用 SQL 与 Workflow 回归保留在 PACK。
- wheel 构建相关测试：14 passed、10 subtests passed。重新从源码构建并安装最终 wheel，确认其中包含公共实现与完整 SQL guide；相关 Python 文件通过 Ruff F 规则检查。
- 使用本机上游 `trace_streamer.exe` 与真实 `all_memory_full.htrace`，验证解码、移除解析器路径后复用、直接打开 SQLite 三条路径；Native Hook 四类事件计数合计 225,456。最终安装包下通过示例真实 Workflow 在同一 Session 连续运行两次，四类事件计数及堆大小聚合均与已有 fixture 预期一致。
- Standards 与 Spec 独立审查完成；Standards 指出的测试保留缺口已补齐并复核，无剩余发现。

以上是 Windows 实跑证据，未执行 Linux 验证；忽略或跳过的用例不计作通过。真实 trace、解析器、生成 SQLite、测试环境和构建日志均未纳入源码提交。


### PR 主线整合验证

基于 `c4c1d564`（已合入公共 Ftrace 与 rc.9）整理 PR，删除重复的 CLI/协议实现，复用主线公共 inspection 与 `knowledge/providers/` 打包目录。

- `cargo build --locked -p kat-cli` 通过；构建并安装 rc.9 workflow wheel。
- 新 wheel 下公共 Provider、Runtime 及 reference PACK 回归：54 passed、1 skipped、37 subtests passed。
- 实际 CLI 列表包含两个公共 Provider，Trace Streamer detail 返回 SQL guide；两个正式 PACK 在新 CLI/Runtime 下重跑为 6 passed 与 28 passed。
- Standards 与 Spec 再次检查主线整合；旧作者文档仅列出 Ftrace 的两处表述已修正。
