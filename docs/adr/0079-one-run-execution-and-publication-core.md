# ADR-0079：所有 Workflow 调用共用执行与发布核心

状态：接受。关联：[#252](https://github.com/maokelong/kat-cli/issues/252)、PR #251。

## 问题与边界

组合能力已经存在，但顶层/子 Run、生产/测试分别实现生命周期；子错误原因丢失。这里只收拢当前交付，不增加 DAG、调度、重试或跨 Run 事务。

## 决定

CLI、`ctx.run`、`kat_run` 共用 Rust 的执行/发布核心。Rust 管 Session lease、候选目录、独立 Runtime、直接子 Run ledger、文件归属和 Manifest 提交。Python 管 PACK 正式加载、Input Compiler、执行与 Parquet 语义。删除嵌套调用专有的 inspection Runtime；保留执行 Runtime 内正式 PACK 检查。

私有协议仅一个 `run_workflow`，输入为互斥的 Arguments / TypedInputs，不兼容旧请求。原始参数和严格 Python 标量不相互降级转换。生产和测试共用一个 JSONL pump。

`kat_run` 在每测试独立临时 Session 中运行真实独立 Runtime；同测试多次调用共享来源物化。它读取已发布 Catalog，继续返回 `dict[str, pyarrow.Table]`。固定被测 PACK，依赖从既有 roots 发现。删除进程内测试父 Workflow 的 Run scope 协议。pytest monkeypatch 不再影响实际 Workflow；普通 helper 单测仍可 monkeypatch。

安全业务诊断传给 `RunError`，详细诊断留在命令退出后可访问的日志；不增加稳定 phase、ID、path 或 retry 字段。

Python writer 在首次发布前验证 footer/Schema/行数。Rust 不重建 PyArrow 类型展示器。Session inventory 校验 Manifest 身份、文件存在和布局，展示发布时元数据；不承诺此刻内容可查询。内容损坏在 PyArrow/DataFusion 实际读取时失败。

本决定局部替代 ADR-0078 的双预检、重复 footer 复核和进程内 `kat_run`，以及 ADR-0047 中 Workflow 执行受 pytest 模块 monkeypatch 影响的合同。其他组合、关闭收拢、lease 与 Manifest 提交语义不变。

## 最小切片与验证

先在真实 CLI/Host 上锁定丢失诊断，再收拢执行发布与协议，最后迁移 `kat_run`。验证直接/嵌套/测试三路输入及输出一致、诊断可追溯、并发关联/关闭、多层直接关系/孤儿、发布失败与 Session lease。最终运行 Rust 检查、Python/builder 测试、真实 Host 与 bundled PACK、Payload smoke。无输出 Run 单独由 ADR-0080 验收。
