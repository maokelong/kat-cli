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
不依赖 Workflow Host 的开发验证，但不能直接执行 `kat inspect --pack` 或 `kat run`。
仅做 Rust 开发时使用 `cargo test -p kat-cli`；需要执行 Workflow 的调用必须满足
下述运行前提。

## 运行前提

`kat inspect --pack` 和 `kat run` 需要带有相邻 Python Host 的完整 KAT Skill
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

- `kat inspect`：列出或检查 PACK。
- `kat test`：通过私有 Runtime 执行 PACK 测试。
- `kat run`：执行一个 Workflow 并原子发布 Run。
- `kat query`：只读查询已发布 Run 的 `output.*`，并发布单文件 NDJSON Query Result。

使用外部 PACK 的调用模板如下；`/path/to/example-pack/pack.toml` 的 `name`
应为 `example`，并声明 `analyze` Workflow：

```bash
kat inspect \
  --pack example \
  --pack-dir /path/to/example-pack
kat test --pack-dir /path/to/example-pack
kat run \
  --pack example \
  --workflow analyze \
  --pack-dir /path/to/example-pack \
  -- \
  --source-path /absolute/path/to/source \
  --limit 20
```

`kat run` 将 `--` 后的 token 原样交给 Workflow Input Compiler。Operation log
可能保留解析后的路径和这些参数，因此不得通过 Workflow arguments 传递秘密。

## Run 公开合同

`manifest.json` 是 Run 的唯一发布门禁；只有 Runtime 成功结束、Operation log 和
Response 都通过校验后，CLI 才发布 Manifest。新 Manifest 只记录 Run、PACK、Workflow、
有效输入和 Output 元数据；Query 读取历史 Manifest 时会忽略任意 JSON 形状的旧
`dataset` 字段，不把它注册为查询关系或恢复成当前能力。

`kat query` 只接受已发布 Run，并且新建一个 fresh DataFusion Session；其中只注册该 Run
的 `output.<name>` Parquet，不扫描 Datasource、PACK 文件或其他 Run。成功 Response 的
`result` 精确返回 `format`、`path` 和 `columns`：`format` 为 `ndjson`，`path` 指向 Runtime
直接写出的单个 NDJSON 文件，文件中每行是一个使用查询列名的 JSON object。不存在、
未发布或损坏的 Run，以及非只读或多语句 SQL，都明确失败。

PACK Authoring API 向每次显式 Workflow 调用提供一个 `kat.Context`。Context 只暴露：

- `ctx.datasource_root`：当前 PACK 在 KAT Data Home 下的私有 Datasource 根；
  文件 Provider 通常在其下创建当前 Workflow 的临时 workspace。

PACK 可在顶层 `datasources/` 中定义普通 Provider 类并由 Workflow 显式调用；
KAT 不扫描、注册、构造或包装 Provider。可追加 Table、Schema、Parquet 写入/打开
与显式本地融合统一由 `kat.dataprovider` Toolkit 提供，推荐导入为
`from kat import dataprovider as dp`；多个内存 Table、Parquet Catalog 或两者的混合
通过普通 `dp.DataFusionProvider` 查询，不进入 Workflow Context 的隐式 catalog。
Workflow 必须返回精确的 `dp.Table`，或返回一个非空普通 `dict`，其字符串键是 Output
名称、值均为精确的 `dp.Table`；PyArrow Table、引擎惰性值、空或混合 Mapping 都失败。

需要原生 Hitrace 解码时，PACK 显式导入独立 wheel 的
`from kat_datasource import hitrace`，并在 `ctx.datasource_root` 下的当前 Workflow 临时目录
调用 `hitrace.decode(source, destination)`。`destination` 必须尚不存在；成功结果是一个
只含扁平 `*.parquet` relation 的目录和一份 unsupported-content report，不是平台持久
状态。Workflow 随后用 `dp.open(root=destination)` 打开 Catalog。需要融合另一份磁盘
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
