# KAT

KAT 是面向性能分析的可扩展平台。KAT Skill 是唯一面向用户的交付物；其中包含 Skill
约束、Bundled PACK、短命的 `kat` CLI、Linux x86_64 私有 Workflow Runtime，以及
Windows x86_64 预发布候选 Runtime。仓库不再交付旧 `kat-rs` CLI、daemon、REST API
或独立的服务端发布面。

项目仍处于 `0.1` 预发布阶段，公共接口和本地布局尚未承诺跨版本兼容。

## 源码开发边界

源码 checkout 可以构建新的 Rust CLI：

```bash
cargo build --release -p kat-cli
```

该命令只生成 Rust 二进制，不装配相邻 Python Host。Cargo 输出可以用于编译检查和
不依赖 Workflow Host 的开发验证，但不能直接执行 `kat inspect workflow`、
`kat inspect provider` 或 `kat run`。
仅做 Rust 开发时使用 `cargo test -p kat-cli`；需要执行 Workflow 的调用必须满足
下述运行前提。

## 运行前提

`kat inspect workflow`、`kat inspect provider` 和 `kat run` 需要带有相邻 Python Host 的完整 KAT Skill
deployment；任意 Cargo 输出目录中的 Rust 二进制不能直接执行它们。CLI 只从相邻的
`python` 目录启动 `_kat_runtime`，不会回退到系统 Python 或从环境变量寻找另一套 Host。
PACK 可以来自内置目录、平台数据目录或显式的 `--pack-dir`。

私有 Python Host 同时安装两个边界独立、版本一致的 wheel：纯 Python
`kat-workflow` 提供顶层 `kat` Pack Authoring API 和 `_kat_runtime`，平台原生
`kat-datasource` 提供 `kat_datasource.hitrace`。两个 distribution 互不依赖，也都不是
可单独下载、混装或兼容的公共 SDK；Platform Payload 将它们与 CLI 一起原子交付。

完整的 Skill 装配和 Platform Payload 发布拓扑遵循
[ADR-0002](docs/adr/0002-skill-and-runtime-ship-atomically.md)。两个原生 payload 只是发布流水线的
私有输入，不是可单独下载或兼容的产品。

## 发布

固定版本的 `dist` 读取 [`dist-workspace.toml`](dist-workspace.toml)，并生成
`.github/workflows/kat-release.yml`。该 workflow 的 tag trigger 限定为 `kat` namespace；
这是 cargo-dist 提供的前缀过滤，正式 tag 仍必须精确为 `kat/<version>`。任何仅命中该前缀
但不符合正式合同的 tag，都会在构建和托管前被发布通道门禁拒绝。日常 PR 只运行固定版本的
`dist plan`，不构建或上传 Payload；带 `full-ci` 标签的 PR 由 `Full CI` 和独立的
`Build KAT Platform Payloads` workflow 执行双平台测试、Payload 构建、Skill 装配与 smoke，
并只上传临时验证产物；同一 PR 推送新提交时会取消仍在执行的旧 Payload 验证。改动 tag、
host、announce、finalizer、公开资产或发布通道时，先把
prerelease RC 合入集成分支，再按[发布候选演练手册](docs/release-rehearsal.md)，在同一生成
workflow 上发布新的 canonical prerelease，完成真实 host → announce → finalizer 与完成态
重跑。PR 门禁只决定 RC 能否进入 `main`；演练和证据评审通过后才能关闭交付 Issue 或进入
stable promotion。
符合合同的 stable 或
prerelease tag 会触发 Linux/Windows payload 构建、唯一 Skill 装配、SHA-256 校验和与
GitHub Release；prerelease 不得成为 Latest。Release 的用户可安装资产只有
`kat-skill-<version>.tar.gz` 及其校验文件。固定的 `dist 0.32` 不能在发布计划中登记自定义
global job 生成的 opaque Skill，且其 `dist-manifest.json` 会声明未公开的原生 payload
归档；生成流水线因此在 `post-announce` 阶段校验最终资产和 SHA-256，再从 Release 删除该
计划中间产物。Linux glibc 2.28 job 从最终压缩包完成发布资格闭环；Windows job 只在
GitHub 托管的 `windows-2025` builder image
验证候选归档的装配、重定位、Bundled Python 选择及 Hitrace decode →
`dp.open`/`DataFusionProvider` → Inspect → `kat test` → Run → Query 机制链路。该 Windows
smoke 不构成无系统级 VC Runtime 的干净客户端验收，
Windows 10/11 正式支持仍由 [Issue #143](https://github.com/maokelong/kat-cli/issues/143) 跟踪。

发布版本以 [`release/kat/dist.toml`](release/kat/dist.toml) 为入口；Cargo workspace 与
Workflow Host 的 package metadata 必须同步为该版本，发布准备阶段会拒绝三者不一致。

发布配置和生成 workflow 必须保持同步：

```bash
python -I -B build/verify_release_versions.py
dist generate --check
dist plan
```

PR 中生成的 Release workflow 同样会运行固定版本的 `dist plan`；`dist 0.32` 会在该命令
开始时拒绝过期或被手改的生成 workflow，不另建一套 YAML 同步门禁。

仓库不提交 payload、完整 Skill、wheel 或其他构建产物。

## Data Home 配置

KAT 默认使用 `directories::ProjectDirs::from("", "", "KAT")` 解析的 Data Home。配置文件路径为：

- Linux：`$XDG_DATA_HOME/kat/config.json`，未设置时为 `$HOME/.local/share/kat/config.json`。
- Windows：`%APPDATA%\KAT\data\config.json`。

若需选择另一个已存在的目录，可在该文件中提供：

```json
{"kat_data_home":"/absolute/path/to/kat-data"}
```

也可以为一次进程设置 `KAT_DATA_HOME`。选择顺序固定为：非空
`KAT_DATA_HOME`、非空 `config.json.kat_data_home`、平台默认目录。所有已提供的配置来源
必须有效后才按该优先级合并，因此已存在的配置文件即使被环境变量覆盖，也必须可读取且
具有有效的 JSON 语法和字段类型。环境变量为空或配置文件不存在表示该来源未提供值；
合并后选中的值必须是可访问的绝对目录，非法值会使操作失败，不会回退。KAT 不展开
`~`、`%USERPROFILE%` 或 `$HOME` 等路径缩写。

## 完整部署中的 `kat` 当前操作

以下命令只适用于满足上述拓扑的完整 KAT Skill deployment：

- `kat inspect`：只读取 manifest，发现 PACK。
- `kat inspect workflow`：发现或读取 Workflow 分析知识。
- `kat inspect provider`：发现或读取 Provider 开发知识。
- `kat inspect session`：按已知 Session ID 列出其中已发布 Run 的公开 inventory。
- `kat session create`：显式发布一个可以为空的 Analysis Session。
- `kat test`：通过私有 Runtime 执行 PACK 测试。
- `kat run`：在必填的已有 `--session` 中原子发布一个 Run。
- `kat query`：用 Session ID 与 Run ID 只读查询已发布 Run 的 `output.*`，并发布单文件 NDJSON Query Result。
- `kat session delete`：按已知 Session ID 永久删除整个 Session。

使用外部 PACK 的调用模板如下；`/path/to/example-pack/pack.toml` 的 `name`
应为 `example`，并声明 `analyze` Workflow：

```bash
kat inspect \
  --pack-dir /path/to/example-pack
kat inspect workflow \
  --pack example \
  --workflow analyze \
  --pack-dir /path/to/example-pack
kat test --pack-dir /path/to/example-pack
kat session create
kat run --session <session-id> \
  --pack example \
  --workflow analyze \
  --pack-dir /path/to/example-pack \
  -- \
  --source-path /absolute/path/to/source \
  --limit 20
```

`kat session create` 不接受 Session ID，成功 Response 精确返回 `result.session_id`；空
Session 可以 inspection，并一直保留到显式删除。每次生产 `kat run` 都必须在外层提供这个
已经存在的 `--session <session-id>`，没有隐式 current/last Session，也不会由 Run 创建、
复用或猜测 Session。`kat run` 将 `--` 后的 token 原样交给 Workflow Input Compiler。
Operation log 可能保留解析后的路径和这些参数，因此不得通过 Workflow arguments 传递秘密。
Run 成功 Response 同时返回 `result.session_id`、`result.run_id` 和 `result.outputs`。

## Analysis Session 与 Run 公开合同

一次 Analysis Session 可以包含不同 PACK 的多个独立 Run，并把 Session 共享来源物化、
每次候选执行的临时数据和不可变 Run Output 分别放在 `materializations/`、`scratch/` 与
`runs/` 下。Session 与 Run 各有独立 UUIDv7；Run 的公开地址始终是
`(session_id, run_id)`，KAT 不按 Run ID 扫描其他 Session，也不维护全局 locator。

`session.json` 是 Session 的不可变公开标记，由独立的 `kat session create` 在任何生产
Workflow 执行前发布；`manifest.json` 是每个 Run 的唯一发布门禁。Run 失败不返回本次
Run ID，也不删除预先存在的 Session；scratch 清理失败同样不发布 Run。Manifest 记录
Session/Run identity、PACK、Workflow、有效输入、直接 `child_runs` 和 Output 元数据，
不记录来源物化 provenance。叶子 Run 的 `child_runs` 是空数组。

`kat query` 只接受已发布 Run，并且新建一个 fresh DataFusion Session；其中只注册该 Run
的 `output.<name>` Parquet，不扫描 Datasource、PACK 文件或其他 Run。成功 Response 的
`result` 精确返回 `format`、`path` 和 `columns`：`format` 为 `ndjson`，`path` 指向 Runtime
直接写出的单个 NDJSON 文件，文件中每行是一个使用查询列名的 JSON object。不存在、
未发布、双 ID 错配或损坏的 Run，以及非只读或多语句 SQL，都明确失败。调用形状为：

```bash
kat query --session <session-id> --run <run-id> --sql \
  'SELECT * FROM output.main LIMIT 20'
kat inspect workflow --session <session-id> --run <run-id>
kat inspect session --session <session-id>
kat session delete --session <session-id>
```

Session inspection 返回按 Run ID 排序的平坦已发布 Run inventory；每项包含 PACK、Workflow、
Outputs，以及按 Run ID 排序但语义无序的直接 `child_runs`，叶子为 `[]`。它允许空
`runs: []`，不递归嵌入调用树，也不暴露 inputs、materializations、scratch、失败调用、
执行计划或物理路径。Session delete 是唯一删除入口，会
永久删除该 Session 的 Runs、Outputs、materializations 与 scratch；活跃操作持有 lease 时
删除立即失败。它不删除 Session 外的 Operation logs 或 Query Results，也不提供单 Run
删除、Session list/current、TTL 或自动 GC。

这是 `0.1` 阶段的破坏性布局切换。新版本不读取、扫描、迁移或删除旧的 Data Home 顶层
`runs/` 与 PACK datasource roots；切换 Data Home 后，原 Session 地址在新 Data Home 中
也无效。

PACK Authoring API 向每次显式 Workflow 调用提供一个 `kat.Context`。Context 只暴露：

- `ctx.datasource_root`：当前 Analysis Session 共享的 `materializations/` 根；同一 Session
  中的 Workflow 可以按明确 Source stem 与数据合同跨 Run、跨 PACK 复用完整物化。
- `ctx.scratch_root`：当前候选执行私有的临时根；一次性工作只能放在这里，执行结束时清理，
  后续 Workflow 不得把它当作输入。
- `ctx.run(pack_name, workflow_name, /, **inputs)`：在同一 Session 中同步执行另一个完整命名的
  Workflow，并在子 Run 发布后返回其只读 `dp.Catalog`；路由参数仅限位置，目标输入仅限
  关键字。同 PACK 和跨 PACK 使用同一发现与执行边界。

组合 Workflow 使用普通 Python 控制流和必要的标准线程能力，最终仍发布自己的
`dp.Table | dict[str, dp.Table]`；Catalog 不能作为嵌套输入或 Run Output。KAT Skill 也可以
为当前任务在同一 Session 中临时运行多个正式 Workflow，但这些顶层调用各自形成独立根
Run，不创建临时 Workflow、父 Run 或其他编排对象。需要复用、inspection 和测试的固定
组合才固化为普通 Python Workflow。

PACK 可在顶层 `datasources/` 中定义普通 Provider 类并由 Workflow 显式调用；
生产 Workflow Runtime 不扫描、注册、构造或包装 Provider，只有显式的 Provider
inspection 会扫描其 metadata declaration。不可变 Table、Datasource Schema、唯一的
流式 Parquet 写入、Parquet Catalog 打开与显式本地融合由 `kat.dataprovider` Toolkit 提供，推荐导入为
`from kat import dataprovider as dp`；多个内存 Table、至多一个 Parquet Catalog 或两者的混合
通过普通 `dp.DataFusionProvider` 查询，不进入 Workflow Context 的隐式 catalog。
Workflow 必须返回精确的 `dp.Table`，或返回一个非空普通 `dict`，其字符串键是 Output
名称、值均为精确的 `dp.Table`；PyArrow Table、引擎惰性值、空或混合 Mapping 都失败。

需要原生 Hitrace 解码时，PACK 显式导入独立 wheel 的
`from kat_datasource import hitrace`。一次性解码在 `ctx.scratch_root` 下使用尚不存在的
destination；需要在同一 Session 复用时，以 `Path(source).stem` 作为
`ctx.datasource_root` 的直接子目录名。复用命中必须先 `dp.open(root=destination)` 并校验
所需 relation、columns、物理类型、nullability 与版本合同；Hitrace 当前要求每个实际 Parquet relation 的 Arrow
Schema metadata 都含 `kat.materialization.version=hitrace-v1`。只有目标不存在时才 decode。已存在但打不开或合同
不兼容的目标必须使当前执行失败，不能删除、覆盖或原位修复。成功解码得到只含扁平
`*.parquet` relation 的目录和 unsupported-content report，不会自动成为 Run Output。
Workflow 随后用 `dp.open(root=destination)` 打开 Catalog。需要融合另一份磁盘
Catalog 时，先用一个 `dp.DataFusionProvider(catalog=...)` 把其中所需 relation 查询成
eager Table，再把该 Table、来源 Provider 返回的其他内存 Table 与另一个 Catalog 显式
交给 `dp.DataFusionProvider(tables=..., catalog=...)`。

完整的 PACK 级本地多表查询与融合写法见
[`examples/packs/local-parquet-fusion`](examples/packs/local-parquet-fusion/README.md)。

## 开发验证

Rust：

```bash
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Workflow Runtime：

```bash
python -I -B -m unittest discover -s kat/platform/workflow/tests -p "test_*.py"
```

## 架构与领域文档

- [领域词汇](CONTEXT.md)
- [架构决策](docs/adr/README.md)
- [贡献协议](AGENTS.md)
