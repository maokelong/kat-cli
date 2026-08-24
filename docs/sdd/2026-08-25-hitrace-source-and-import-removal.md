# Hitrace Source 与顶层 Import 退场

## 1. 问题

PACK Sources 已经建立统一的 DataFusion Source Schema 边界，但产品仍保留顶层 `kat import`，Hitrace 的 Rust parser 也没有可由 `kat-kernel` Source Entry 返回的 Provider。直接删命令会让 `.htrace` 暂时失去产品入口；保留命令则让用户继续面对两套来源模型。

本切片先把现有 Hitrace 事实接到官方 DataFusion FFI，再删除整个 Import 命令和已经决定退场的 Trace Streamer 闭包。同时收口此前确认、但当前 Sources 实现仍未完成的跨 PACK External Query、Materialized REDO recipe 与单文件 Parquet 语义。

## 2. 交付与不做

交付：

- `kat-kernel/hitrace` Source Entry，输入为一个 `trace: pathlib.Path`；
- Linux/Windows Bundled Host 中真实可导入的 Hitrace native extension；
- External Query、Workflow、Materialization 和 Materialized Query 共用一个 DataFusion 查询面；
- 跨 PACK External Binding 的惰性解析；
- Materialized Binding 保留 REDO recipe，但查询不回退 External Provider；
- KAT Materialized tables 只使用 `<table>.parquet`；
- 删除顶层 `kat import`、Trace Streamer、SQLite 依赖和两个预发布 PACK；
- 同步 README、Skill、CONTEXT、ADR、Issue 与 CI。

不做：

- 多 `.htrace` 或多 capture 聚合；
- 为 Hitrace、Rust 或 Python 定义 KAT 自有 Provider 协议；
- 给 `kat materialize` 增加来源特判、bulk profile 或平台 Parser 调度器；
- 发布 Native Hook descriptor-derived facts；
- 把 unsupported content 设计为新的 Source table；
- 来源哈希、一致性、版本、迁移、回滚或自动恢复；
- Materialized table fragments、Dataset overlay 或多 Dataset Query。

## 3. 用户 Interface

公开命令只剩：

```text
kat bind --pack kat-kernel --source hitrace --dataset <dataset> [--pack-dir <pack> ...] -- --trace <file.htrace>
kat materialize --pack kat-kernel --source hitrace --dataset <dataset> [--table <name> ...] [--replace] [--pack-dir <pack> ...]
kat query --dataset <dataset> [--pack-dir <pack> ...] --sql <sql>
```

`kat bind` 只编译参数并保存 External Binding。第一次查询 `"kat-kernel".hitrace.<table>` 或执行使用该 schema 的 Workflow 时，Runtime 才调用 Source Entry。`kat materialize` 没有本次 argv 时重放 Binding 保存的 recipe；有本次 argv 时以本次 cwd 解释 Path，并在已有 Binding 时要求 `--replace`。

`kat import` 必须成为 Clap unknown operation；不保留隐藏 alias。Dataset Query 与 `query --run` 对关联 Dataset 使用相同 Source Resolution，因此两者都可以执行实际被 SQL 解析的 External Binding。显式选择 Dataset 即是对这些 Source 的执行授权。

## 4. Hitrace native Source

新增 Hitrace 专用 PyO3 extension，公开给 PACK Entry 的 Python 对象只实现：

```python
__datafusion_schema_provider__(codec) -> object
```

实现严格遵循 DataFusion 54 官方 FFI example：

- 从 codec PyCapsule 取得 `FFI_LogicalExtensionCodec`；
- 把拥有 Hitrace 表和临时资源生命周期的 `Arc<dyn SchemaProvider + Send>` 包装为 `FFI_SchemaProvider::new_with_ffi_codec(provider, None, codec)`；
- 返回名为 `datafusion_schema_provider` 的 PyCapsule。

Rust workspace、`datafusion-ffi`、`datafusion-python-util` 与 Bundled Python Host 的 DataFusion 锁定到同一 54.0.0 系列，避免在首个生产 exporter 中同时验证跨版本 FFI。PyO3 extension 作为 Hitrace 专用 Adapter，不进入 `kat` Python authoring API，也不形成其他 PACK 必须采用的依赖合同。

Provider 在 Source Resolution 时解析一份 `.htrace`。第一版复用已验证的 Rust parser/sink，把结果写到 Provider 自己拥有的空临时目录，再用普通 DataFusion Parquet TableProviders 组成 `MemorySchemaProvider`；该目录由 SchemaProvider 的 `Arc` 生命周期持有并在 operation 结束后清理。这个 staging 只是 Adapter 实现，不能发布或改写 Dataset Binding，也不能被后续进程发现或复用。只有 `kat materialize` 可以把查询结果发布到 Dataset。

Provider 的表集合只包含现有产品合同中的 `clock_domain`、`clock_snapshot` 和可选 `sched_switch`。损坏输入沿 Source Resolution 保留原始错误链；合法但不受支持的内容继续按既有 parser 规则处理，不借迁移增加 Source table。一次 operation 内 FFI exporter、Hitrace 解析和每张表的 provider 都只建立一次。

`kat/packs/kat-kernel/sources/hitrace.py` 只声明 Source Entry 并构造 native provider。Entry 不知道 Dataset 目标、不自行执行 Materialization，也不发布 Run。

## 5. Native wheel 与 Payload

Linux 和 Windows Payload builder 在构建同版本 CLI 时，同时用锁定的 maturin/PyO3 工具链构建 Hitrace extension wheel，并在剪裁 Bundled Python Host 前以 `--no-deps --no-index` 安装。构建产物必须：

- 对应 CPython 3.14 和当前平台；
- 使用与 KAT release 相同的版本；
- 进入 native dependency / glibc 基线检查；
- 不把 wheel、Cargo artifact 或构建工具留在最终 Payload；
- 通过 Bundled Host 的真实 import 与 FFI Query smoke test。

纯 Python `kat-workflow` wheel 继续只构建一次并供双平台使用；Hitrace wheel 是平台 Payload 的私有组成，不改变外部 PACK 的安装模型。

## 6. 跨 PACK Source 装载

Workflow 所属 PACK 继续作为唯一公开 `kat.pack`。Source operation 发现 Dataset 中 External Bindings 后，为每个 PACK 建立由 canonical PACK directory 派生、仅在本进程可见的私有 module root；同一 PACK 的 `sources/`、decoder 和 helper 在该 root 下按正常 package 关系加载。

Source-owned imports 必须使用相对形式，例如：

```python
from ..decoders.smaps import decode_smaps
```

不能使用 `from kat.pack...`，因为那只表示当前 Workflow PACK。私有 root 不进入 inspect identity、Binding metadata、SQL 名称或错误中的公共 Source identity。多个不同 PACK 的 External Sources 可以在一次 operation 中同时加载；同名 PACK candidates 仍在执行任何 Source code 前失败。

`kat query` 增加可重复 `--pack-dir`，并把 candidates 传入 Dataset/Run Query request。Runtime 只加载 SQL 实际解析到的 External Source，不因 Dataset 中存在 Binding 就调用 Entry、访问远端或解析文件。

## 7. Binding 与 Materialized table

Binding metadata 的 Materialized 形态增加并保留：

- `arguments`: 原始 argv token array；
- `working_directory`: 绑定或本次显式 materialize 的绝对 cwd；
- `tables`: 已发布表名。

External 转 Materialized 时复制原 recipe；直接用显式 argv 物化时保存本次 recipe；零参数/default Source 保存空 argv 与 cwd。Inspection 仍不展示 argv、cwd 或凭据。

Materialized Binding resolution 只注册 `tables/<table>.parquet`。不存在的表返回 table-not-found，不读取 recipe，不调用 Source Entry。Partial materialize 的选中集合完整替换该 Source 的当前 `tables`；不做表级 merge。再次 materialize 时 recipe 只用于重新取得 Provider。

删除 fragment directory 的 metadata、inspection、query 与测试分支。外部 Provider 自己可以读取多文件 Parquet，但写入 KAT Dataset 后每个逻辑表只有一个受管理文件。

## 8. Import 与 Trace Streamer 删除闭包

删除：

- CLI `Import` operation、Hitrace/Trace Streamer args、handlers、responses、errors 和集成测试；
- Datasource Trace Streamer module、tests 和 SQLite fixtures；
- workspace、Datasource 与 CLI test 中仅为 Trace Streamer 存在的 `rusqlite`；
- `kat-openharmony-critical-path` 与 `kat-openharmony-thread-cpu-time` 两个预发布 PACK；
- README、Skill、Payload CI 中的 Import/Trace Streamer 产品路径。

保留：

- Hitrace framing/parser、ftrace/native-hook domains、Arrow sinks、record 与 mmap；
- Hitrace proto、codegen、descriptor-derived 设施及合同测试；
- 时钟、`sched_switch`、capture integrity 与 unknown-content 的既有领域实现；
- research 与 proto 中对上游 Trace Streamer 的来源说明。

Payload CI 改用 `kat-kernel/raw_smaps` 执行 `bind → materialize → inspect → test → run → query`。Hitrace FFI 另有真实 native wheel E2E，避免普通 payload smoke 依赖大 fixture。

## 9. 验收

至少证明：

1. Hitrace native exporter 在 Bundled Host 中返回可被 DataFusion 54 接收的真实 FFI capsule；
2. 同一 External Hitrace Binding 可被 Dataset Query 和 Workflow 查询，重复表访问不重复导出或解析；
3. 通用 `kat materialize` 全表及 `--table` 子集成功，删除原始 trace 后 Materialized Query 仍成功；
4. Materialized 缺失表不回退 External recipe；再次 materialize 可以重放 recipe；
5. 一次 SQL 可 join 两个不同 PACK 的 External Sources，未引用 External Source 零调用；
6. 两个 Source PACK 使用相同 module 相对路径时互不污染；实际执行的绝对 `kat.pack` Source import 不能借用当前 Workflow PACK 而成功，按普通 Python import 语义失败，Runtime 不读取或分析源码文本；
7. fragment directory 被 Dataset inspection 拒绝；单文件表保持既有 Schema inspection；
8. `kat import`、Trace Streamer 和两个预发布 PACK 不再出现在 CLI、Skill、Payload 或依赖树；
9. Rust、Workflow Host、PACK、builder、Linux/Windows Payload contract、format、clippy 与 diff checks 全部通过。
