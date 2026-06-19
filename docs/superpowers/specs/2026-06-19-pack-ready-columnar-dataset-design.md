# Pack-ready columnar dataset 设计

## 背景

Issue #45 将 kat-rs 的长期方向收敛为：

```text
trace columnar data engine + semantic transform pack runtime
```

当前 `main` 已经具备 `.htrace`、Langfuse legacy、Arrow `RecordBatch`、DataFusion SQL 查询和本机 server。server 通过进程内 registry 复用已加载的 `TraceDatasource`，解决了连续查询反复构建 datasource 的问题。

但 PR #44 的真实数据验证已经暴露出下一层瓶颈：Langfuse legacy 大批次在内存物化后查询很快，但 5GiB+ 压缩输入会放大到接近 32GiB 机器的内存边界。这个问题不能靠继续延长 server 生命周期解决；需要把可查询事实层落到磁盘 columnar dataset，并让 server/REST 和后续 pack runtime 使用同一种 dataset 语义。

## 要解决的问题

本设计要解决：

1. 为 `.htrace` 和 Langfuse legacy 提供统一的本地 columnar dataset 格式。
2. 让 raw/direct tables 可以落盘为 Parquet，并由 DataFusion 从 dataset 重新注册查询。
3. 让 server/REST 和后续 pack runtime 共享同一个 dataset store 概念，避免“持久化数据”和“server 缓存”两套语义。
4. 为后续 pack transform DAG、derived tables 和 analysis runtime 提供最小 dataset 读写边界。
5. 用真实数据性能验证证明大数据集查询不再依赖全量内存 `MemTable`。

## 不做什么

1. 不在第一版实现完整 pack runtime、derived DAG、analysis plan 或 evidence runtime。
2. 不在第一版实现 multi-source append；一次 materialize 只生成一个新的完整 dataset。
3. 不在第一版实现 LRU、idle timeout、spill、水位淘汰或自动清理策略。
4. 不默认计算输入文件全量 hash，避免 materialize 前额外完整扫描大文件。
5. 不提交大体积真实数据 fixture。
6. 不让 JSON evidence 或 query result 成为中间事实层；中间事实层保持列式表。

## 方案选择

采用 **XDG data home 下的 named dataset store + Parquet tables + JSON metadata**。

备选方案及取舍：

1. 只给 server 增加磁盘 cache：改动较小，但会把 dataset 语义绑到 server 生命周期，后续 pack runtime 仍要重做边界。
2. Arrow IPC/Feather：Arrow schema 保真，但长期磁盘 dataset、跨工具生态和 DataFusion 常规路径不如 Parquet。
3. 抽象多种 table format：提前引入配置和扩展点，超出第一版需要。
4. Parquet + JSON：直接面向列式磁盘数据集，DataFusion 可注册查询，JSON catalog 适合作为机器生成、机器读取的表映射。

## Dataset store

dataset store 默认根目录遵循 XDG Base Directory Specification：

```text
${XDG_DATA_HOME:-$HOME/.local/share}/kat-rs/datasets/
```

如果 `XDG_DATA_HOME` 为空、未设置或不是绝对路径，按规范忽略它并回退到 `$HOME/.local/share`。如果无法解析 home directory，则 resolver 返回明确错误。

第一版默认 dataset 名为 `default`：

```text
$XDG_DATA_HOME/kat-rs/datasets/default/
  catalog.json
  tables/
    *.parquet
```

dataset locator 使用同一个 resolver：

- locator 为空时解析为 named dataset `default`。
- locator value 不含路径分隔符时解析为 `$XDG_DATA_HOME/kat-rs/datasets/<value>/`。
- locator value 是绝对路径或包含路径分隔符时按显式路径解析。
- named dataset 第一版限制为简单名称，拒绝空字符串、`.`、`..` 和路径分隔符。

这里不引入 `XDG_CACHE_HOME`。真正可丢弃的内部临时文件未来可以放到 cache home，但第一版的 dataset 是用户可复用的数据资产，不是 server 私有缓存。

## Dataset layout

dataset 目录结构：

```text
<dataset>/
  catalog.json
  tables/
    hitrace.profiler_plugin_data.parquet
    hitrace.sched_switch.parquet
    hitrace.native_hook_alloc.parquet
    langfuse.langfuse_observations.parquet
    langfuse.langfuse_traces.parquet
```

SQL 逻辑表名保持现有查询面，例如 `sched_switch`、`native_hook_alloc`、`langfuse_observations`。物理 Parquet 文件名可以带 source 前缀，避免未来不同 source 的物理文件名冲突；逻辑表名和物理路径通过 `catalog.json` 映射。

预留但不实现：

```text
derived/
  <pack_hash>/
runs/
  <run_id>/
```

第一版只写 `tables/` 下的 raw/direct tables。

## Metadata

第一版不写 dataset-level `manifest.json`。进程启动前已经存在的 dataset 一律视为外部输入，reader 只做足以注册查询的结构校验；版本、来源、输入文件和生成器信息不参与信任判断，也不在本切片里固化成兼容契约。后续若需要 dataset provenance、pack evidence 或跨版本迁移，应在独立 issue/SDD 中重新设计。

`catalog.json` 记录 table 级信息：

```json
{
  "tables": [
    { "name": "sched_switch", "path": "tables/hitrace.sched_switch.parquet" }
  ]
}
```

第一版 catalog 只保存 SQL 逻辑表名到 dataset 内 Parquet 相对路径的映射。Parquet 文件自身携带完整 schema；reader 以 Parquet metadata 为实际 schema 来源。这里不保存也不兼容 `formatVersion`、`rowCount`、`columns`、`schemaFingerprint`、`tableId` 或 table-level source/category，避免把当前临时 metadata 过早固化成兼容契约。后续 pack 若需要稳定 table id、schema 选择或 derived table provenance，应在对应 issue 中重新设计，不从第一版 catalog 继承。

## 接口边界

PR #62 已把用户可见 CLI 收敛为本机 server 生命周期与 OpenAPI 输出，业务能力应优先表现为 REST/OpenAPI resource。因此本切片不再新增 `kat-rs ingest` 或 `kat-rs query --dataset` CLI。

第一版交付 `kat-rs-datasource` 内核能力：

- `materialize_hitrace_dataset(...)`：把 `.htrace` 写成本地 Parquet dataset。
- `materialize_langfuse_legacy_dataset(...)`：把 Langfuse legacy observations/traces 写成本地 Parquet dataset。
- `TraceDatasource::from_dataset(...)`：从 dataset catalog 注册 Parquet 表并执行 DataFusion SQL。
- `DatasetStore` / `DatasetLocator`：提供 XDG data home 下 default/named dataset resolver，以及显式 path dataset。

server/REST 接入本地 dataset resolver、dataset materialization 和 dataset datasource 创建留给 #60，不混入本切片。

第一版只创建新的完整 dataset。目标路径已存在时直接失败，不追加、不覆盖，也不提供替换模式。用户需要保留多次 materialize 结果时，应使用不同 dataset name/path，例如 named dataset `run-2026-06-19`；删除或替换已有 dataset 留给后续 dataset 生命周期接口重新设计。

多次 materialize 到同一个 dataset 的语义：

- 如果目标 dataset 已存在，操作失败。
- 第一版不支持 append。append 会引入跨 source schema 合并、重复输入识别、分区布局、row-level provenance 和删除/更新语义，必须作为独立设计处理。

## Crate 与模块边界

`kat-rs-datasource` 拥有 source materialization、dataset writer 和 dataset reader：

```text
crates/kat-rs-datasource
  src/
    arrow_table.rs        # 内存态 Arrow table 集合
    materializer.rs       # source-specific: hitrace/langfuse -> Arrow -> dataset writer
    dataset/
      catalog.rs          # 磁盘 catalog JSON
      resolver.rs         # dataset name/path resolver
      writer.rs           # Arrow RecordBatch -> Parquet + catalog + publish
      reader.rs           # catalog + Parquet -> DataFusion tables
```

职责：

- source-specific materializer 从 `.htrace` 和 Langfuse legacy source 产出 Arrow tables/streams。
- dataset writer 不认识 source，只将 Arrow RecordBatch 写为 Parquet，生成 `catalog.json` 并发布 dataset。
- dataset reader 读取 `catalog.json` 和 Parquet，并注册 DataFusion 表。
- 提供 `TraceDatasource::from_dataset(path)`，由 DataFusion 注册 Parquet 表。

server/REST 后续接入时也调用同一个 resolver 和 reader，不拥有私有 dataset cache 格式。`kat-rs-cli` 只负责本机 server 生命周期和 OpenAPI 输出，不承载 dataset 业务参数。

## Materialization flow

`.htrace`：

```text
.htrace
  -> formats/hitrace
  -> domains/*
  -> record stream
  -> ArrowSink / ArrowTableSet
  -> dataset writer
  -> tables/*.parquet
  -> catalog.json
```

Langfuse legacy：

```text
observations.jsonl.gz + traces.jsonl.gz
  -> DataFusion JSON reader 或等价 batch reader
  -> RecordBatch stream
  -> dataset writer
  -> tables/*.parquet
  -> catalog.json
```

为真正缓解大数据集内存压力，persistent path 不能先 `collect()` 整张 Langfuse 表再写 Parquet；应按 DataFusion 或 Arrow reader 产生的 `RecordBatch` 边界写入 Parquet row groups。

`.htrace` 当前 `ArrowSink` 会把各表聚合后一次性 `finish()` 为 `ArrowTableSet`。第一版可以先复用这条路径完成格式落地和查询一致性验证，但真实大 trace 性能目标要求后续把 sink 改成可分批 flush 到 dataset writer。SDD 的实现计划应把 Langfuse streaming write 作为大数据验收的关键路径，把 `.htrace` streaming sink 作为同一 Arrow writer 边界下的后续小步。

## 读取与查询

`TraceDatasource::from_dataset(path)`：

1. 读取并校验 `catalog.json`。
2. 对每个 table 注册 catalog 中的 Parquet path。
3. SQL 查询仍由 DataFusion 执行。
4. JSON 输出仍沿用当前 `query_json` 最后一公里转换。

第一版把进程启动前已经存在的 dataset 视为外部输入，而不是可信运行状态。server 后续接入时也不应在启动时自动恢复旧 dataset 为 READY；只有用户显式打开 name/path/default dataset 时，reader 才做基础结构校验并注册 Parquet 表。旧 catalog 结构不兼容时，优先拒绝或要求重新 materialize，而不是做复杂迁移。

读取校验：

- `catalog` 引用的 Parquet 文件必须存在，并且可读取 Parquet metadata。
- 表名必须唯一。
- table path 必须是 dataset 目录内相对路径，不能逃逸到外部路径。

## Server 语义

server 后续使用同一 dataset resolver：

- 创建 datasource 时可以指定 dataset name/path。
- 不指定时使用 `default`。
- server 打开的 datasource 是进程内 DataFusion session/table handle。
- 删除 datasource 只释放 server handle，不删除 dataset。
- 删除 dataset 未来由独立 REST/OpenAPI resource 管理。

server 不再有独立磁盘 cache 语义。它只是同一 dataset store 的在线查询入口。

## 写入一致性

dataset 写入采用临时目录 + rename 的 best-effort 提交流程：

```text
<dataset>.tmp-<uuid>/
  catalog.json
  tables/
```

流程：

1. 解析目标 dataset。
2. 如果目标已存在，返回冲突错误。
3. 在同级目录创建临时目录。
4. 写入所有 Parquet 文件。
5. 写入 `catalog.json`。
6. 校验临时 dataset 可读。
7. rename 到目标目录。
8. 失败时清理临时目录；后续版本可以清理 `.tmp-*` 遗留目录，但第一版不把它们作为可自动恢复的可信状态。

resolver 应确保临时目录创建在目标同级目录，避免跨文件系统 rename。第一版的一致性目标是避免误删已有路径，并尽量只发布完整校验后的新 dataset；不追求跨崩溃事务语义。

## 错误处理

第一版错误应区分：

- dataset path 无法解析；
- named dataset 名称非法；
- dataset 已存在；
- catalog 格式不符合当前最小结构；
- catalog 引用文件缺失；
- source 读取或 Parquet 写入失败；
- SQL 查询失败。

当前 datasource crate API 保留结构化错误上下文；后续 REST/OpenAPI 接入时应映射为统一 error envelope。

## 性能验证合同

性能验证是本设计的交付要求，不是可选备注。PR 中必须像 PR #44 一样记录真实数据性能证据。

通用要求：

1. 只接受 `release` target 数据；debug 性能不作为上库证据。
2. 记录硬件环境：CPU、核心/逻辑处理器、内存容量、OS、磁盘类型；能稳定取得时记录内存频率。
3. 记录数据集规模：输入文件路径脱敏、压缩大小、表 row count、Parquet 总大小。
4. 同时记录 wall time 和命令或服务端内部 elapsed time。
5. 内存按固定间隔采样，记录峰值 RSS/Working Set/Private Memory；平台差异在 PR 中说明。
6. 真实数据验证不得记录凭据、COS URL、用户敏感 payload 或完整业务内容。

验证数据来源：

- `.htrace` 代表样本使用本地文件 `/root/data/hiprofiler-wechat-coldstart-smartperf-20260523-182338.7z`，测试前解压到临时目录，PR 只记录脱敏后的样本名、压缩包大小和解压后输入大小。
- Langfuse 真实大批次使用私有 Langfuse blob export。访问凭据、bucket endpoint 和对象路径通过本地环境变量或未跟踪 secrets 文件传入，不写入仓库、脚本、PR 正文或测试日志。

必须覆盖：

1. Langfuse 大批次：对比当前内存物化路径与新 dataset 路径，记录 materialize 耗时、峰值内存、Parquet 总大小、`count` 查询、`join limit 3` 查询。
2. 源文件脱离：materialize 成功后删除 hard link 或移动输入文件，再从 dataset 查询成功。
3. 独立进程验证：新进程或独立测试进程直接 `TraceDatasource::from_dataset(...)`，证明不是 server 进程内状态。
4. `.htrace` 代表样本：记录 materialize 耗时、表数量、row count、Parquet 大小，并证明关键查询结果与旧直接 datasource 路径一致。
5. server 后续接入时：证明 server 打开 dataset 不把全量数据重新 materialize 成内存表。

第一版不在 SDD 中写死绝对性能阈值。验收目标是：大数据集查询阶段峰值内存必须显著低于 PR #44 的内存物化路径，并且查询不再要求源文件存在。绝对阈值应在第一次真实数据跑完后沉淀到 PR 验证记录。

## 2026-06-19 release 验证记录

环境：

- OS: Linux 5.10.0-182.0.0.95.r3353_273.hce2.x86_64 x86_64。
- CPU: x86_64 KVM VM，48 logical CPUs，1 socket，24 cores/socket，2 threads/core，AMD General Purpose Processor。
- Memory: 94GiB total，0B swap。
- Disk: virtual block devices，`VBS fileIO`/virtio disk。
- Build: `cargo build --release -p kat-rs-cli`。

`.htrace` 代表样本：

- archive: `hiprofiler-wechat-coldstart-smartperf-20260523-182338.7z`，15,450,859 bytes。
- extracted `.htrace`: 159,513,402 bytes。
- materialize to dataset: 1.54s wall，393,224 KiB peak RSS。
- Parquet dataset: 161MiB，41 tables。
- selected row counts: `sched_switch` 345,796；`sched_blocked_reason` 27,132；`profiler_plugin_data` 26,338；`native_hook_alloc` 0。
- direct source query `select count(*) from sched_switch`: 1.21s wall，388,624 KiB peak RSS，result 345,796。
- dataset query `select count(*) from sched_switch`: 0.01s wall，约 58-61MiB peak RSS，result 345,796。
- dataset grouped sched query: 0.02s wall，67,152 KiB peak RSS。
- source-detached verification: 移动 extracted `.htrace` 后，新进程 dataset 查询仍返回 345,796，0.01s wall，60,744 KiB peak RSS。

Langfuse legacy 真实大批次：

- observations gzip: 650,477,372 bytes；traces gzip: 258,983,997 bytes。
- rows: `langfuse_observations` 28,934；`langfuse_traces` 6,634；join by `trace_id` count 28,710。
- materialize to dataset: 21.47s wall，5,181,204 KiB peak RSS。
- Parquet dataset: 3.6GiB；observations Parquet 2,740,307,554 bytes；traces Parquet 1,046,461,197 bytes。
- direct source query `select count(*) from langfuse_observations`: 15.30s wall，4,865,652 KiB peak RSS，result 28,934。
- dataset query `select count(*) from langfuse_observations`: 0.01s wall，57,900 KiB peak RSS，result 28,934。
- dataset join count query: 0.01s wall，66,668 KiB peak RSS，result 28,710。
- source-detached verification: 临时移动 observations/traces gzip 后，新进程 dataset 查询仍返回 28,934，0.01s wall，58,624 KiB peak RSS。

Langfuse legacy 超大批次：

- observations gzip: 6,394,662,404 bytes；traces gzip: 2,794,975,119 bytes；compressed total 9,189,637,523 bytes。
- rows: `langfuse_observations` 214,916；`langfuse_traces` 59,800。
- materialize to dataset: 4m30.34s wall，29,332,436 KiB peak RSS，exit status 0。
- Parquet dataset: 36GiB；observations Parquet 26,559,816,935 bytes；traces Parquet 11,170,211,752 bytes。
- dataset query `select count(*) from langfuse_observations`: 0.14s wall，56,184 KiB peak RSS，result 214,916。
- dataset query `select count(*) from langfuse_traces`: 0.06s wall，56,116 KiB peak RSS，result 59,800。

实现观察：

- Langfuse materialize 的 peak RSS 仍接近直接 source query，主要来自 DataFusion JSON/GZIP schema 推断和 batch 读取路径；超大批次已验证不会在 94GiB / no swap 测试机上 OOM，但 9.19GB gzip 输入会达到约 28.0GiB RSS，仍接近 32GiB 机器边界。本切片已经把 query 阶段从 GiB 级 RSS 降到约 56-67MiB RSS，并让重复查询不再依赖原始 gzip 文件。
- 真实 Langfuse export 中存在顶层空对象列，例如 `tool_definitions: {}`。DataFusion 会把这类列推断成空 `Struct([])`，Parquet writer 不支持写入空 struct。第一版在 Langfuse dataset materializer 中只对顶层空 struct 列做兼容转换：非 null 值写成 JSON 字符串 `"{}"`，null 仍为 null；后续若引入显式 Langfuse schema，应在同一边界替换该兼容逻辑。

## 测试计划

自动化测试：

1. `materialize_hitrace_dataset(...)` 生成 dataset，随后 `TraceDatasource::from_dataset(...)` 能查询 sched direct table。
2. `materialize_langfuse_legacy_dataset(...)` 生成 dataset，随后 `TraceDatasource::from_dataset(...)` 能 join observations/traces。
3. 设置临时 `XDG_DATA_HOME` 时，默认 resolver 写入并读取该目录下的 `kat-rs/datasets/default`。
4. 目标 dataset 已存在时返回错误。
5. `TraceDatasource::from_dataset` 只依赖 Parquet dataset；删除源输入文件后查询仍成功。
6. catalog 中重复表名、缺失 Parquet 文件、逃逸路径会被拒绝。
7. materialize 后不生成 `manifest.json`。
8. catalog root 只包含 `tables`，table entry 只包含 `name` 和 `path` 时可读取并查询；旧 version/table metadata 字段会被拒绝。

手动/真实数据验证：

- 使用 PR #44 同级别 Langfuse 真实大批次进行 release 性能记录。
- 使用至少一个包含 sched/native_hook direct tables 的 `.htrace` 代表样本进行 release 验证。

提交前基础验证：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python .github/scripts/test_pr_guard.py
```

## 最小交付切片

第一版交付顺序建议：

1. 定义最小 dataset catalog structs、resolver 和 JSON 读写。
2. 引入 Parquet 写入/读取依赖。
3. 实现 source-agnostic dataset writer。
4. 实现 Langfuse legacy streaming materializer，输出 RecordBatch stream 到 dataset writer。
5. 实现 `.htrace` materializer，先复用现有 `ArrowTableSet` 输出到 dataset writer。
6. 实现 `TraceDatasource::from_dataset`。
7. 不新增 CLI 业务命令；REST/OpenAPI dataset resource 留给 #60。
8. 补齐自动化测试。
9. 用真实数据补齐性能验证证据。

这个切片先让 raw/direct tables 成为可复用的磁盘列式 dataset。pack manifest、derived tables、analysis state 和 evidence runtime 在后续设计中接入，不混入本次 PR。
