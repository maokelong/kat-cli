# kat-rs

KAT 是面向性能分析的可扩展平台。仓库正从旧的常驻 REST/DataFusion 数据面迁移到
短命的 `kat` CLI 和受管理的 Workflow Runtime。

当前交付专注于 `kat run` 闭环。旧 `kat-rs` CLI、daemon、REST API 和 Rust
DataFusion 查询面仍保留为既有代码，但不参与 `kat run`，也没有通过 feature、
shim 或 wrapper 与新执行面建立兼容关系。它们的直接删除属于后续独立变更。

项目仍处于 `0.1.0` 的早期演进阶段，公共接口和本地布局尚未承诺跨版本兼容。

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
[ADR-0002](docs/adr/0002-skill-and-runtime-ship-atomically.md)，由
[PR #160](https://github.com/maokelong/kat-rs/pull/160) 跟踪；本 PR 只交付 `kat run`
纵向闭环。

## 完整部署中的 `kat` 当前操作

以下命令只适用于满足上述拓扑的完整 KAT Skill deployment：

- `kat import hitrace`：将 HiProfiler Hitrace 导入为受管理 Dataset。
- `kat import trace-streamer`：预发布联调用的 deprecated Trace Streamer 导入。
- `kat inspect`：列出或检查 PACK。
- `kat inspect --dataset <directory>`：只读检查 Dataset 与 Parquet Schema。
- `kat run`：执行一个 Workflow 并原子发布 Run。

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

## Run 公开合同

`manifest.json` 是 Run 的唯一发布门禁；只有 Runtime 成功结束、Operation log 和
Response 都通过校验后，CLI 才发布 Manifest。当前切片只返回 Run metadata；`kat query`、
`output.*` 和 Dataset 三态查询属于
[issue #139](https://github.com/maokelong/kat-rs/issues/139) 的后续独立交付。

PACK Authoring API 通过显式的 `kat.Context` 暴露受管理能力：

- `ctx.sql(sql, **params)`：普通只读 SQL。
- `ctx.from_arrow(table)`：将 PyArrow Table 放入当前 execution plane。
- `ctx.convert_clock(..., target_domain="...")`：通过 Runtime 私有的稳定
  Python/PyArrow batch UDF 换算时钟。

`kat_convert_clock(...)` 不注册为 SQL 函数；SQL 直接调用会按未知函数失败。

现存的 `kat-rs-cli`、daemon、REST API 和 Rust DataFusion 查询面与新 `kat run` 彼此
独立，仍保持原有行为；其移除由
[issue #176](https://github.com/maokelong/kat-rs/issues/176) 跟踪。

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
