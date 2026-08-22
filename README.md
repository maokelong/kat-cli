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
验证候选归档的装配、重定位、Bundled Python 选择及 Import → Inspect → `kat test` →
Run → Query 机制链路。该 Windows smoke 不构成无系统级 VC Runtime 的干净客户端验收，
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

- `kat import hitrace`：将 HiProfiler Hitrace 导入为受管理 Dataset。
- `kat import trace-streamer`：预发布联调用的 deprecated Trace Streamer 导入。
- `kat inspect`：列出或检查 PACK。
- `kat inspect --dataset <directory>`：只读检查 Dataset 与 Parquet Schema。
- `kat test`：通过私有 Runtime 执行 PACK 测试。
- `kat run`：执行一个 Workflow 并原子发布 Run。
- `kat query`：只读查询已发布 Run 的 `output.*`。

使用外部 PACK 的调用模板如下；`/path/to/example-pack/pack.toml` 的 `name`
应为 `example`，并声明 `analyze` Workflow：

```bash
kat import --dataset ./dataset hitrace --trace ./capture.htrace
kat inspect --dataset ./dataset
kat run \
  --pack example \
  --workflow analyze \
  --pack-dir /path/to/example-pack \
  --dataset ./dataset \
  -- \
  --limit 20
```

`kat run` 将 `--` 后的 token 原样交给 Workflow Input Compiler。Operation log
可能保留解析后的路径和这些参数，因此不得通过 Workflow arguments 传递秘密。

需要在网络受限的 Windows 环境连接真实 PostgreSQL 开发 External PACK 时，见
[PostgreSQL External PACK 开发与离线环境](docs/postgresql-pack-development.md)。该链路
直接从远程数据库生成 Run Output，不经过 `kat import` 或 Dataset。

## Run 公开合同

`manifest.json` 是 Run 的唯一发布门禁；只有 Runtime 成功结束、Operation log 和
Response 都通过校验后，CLI 才发布 Manifest。`kat query` 只接受已发布 Run，并通过
`output.<name>` 查询 Manifest 声明的输出；不存在、未发布或损坏的 Run 都明确失败。

PACK Authoring API 通过显式的 `kat.Context` 暴露受管理能力：

- `ctx.sql(sql, **params)`：普通只读 SQL。
- `ctx.from_arrow(table)`：将 PyArrow Table 放入当前 execution plane。
- `ctx.convert_clock(..., target_domain="...")`：通过 Runtime 私有的稳定
  Python/PyArrow batch UDF 换算时钟。

`kat_convert_clock(...)` 不注册为 SQL 函数；SQL 直接调用会按未知函数失败。

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
