# ADR-0079：所有 Workflow 调用共用执行与发布核心

状态：接受。关联：[#252](https://github.com/maokelong/kat-cli/issues/252)、PR #251、[#254](https://github.com/maokelong/kat-cli/issues/254)。

## 问题与边界

组合能力已经存在，但顶层/子 Run、生产/测试分别实现生命周期；子错误原因丢失。这里只收拢当前交付，不增加 DAG、调度、重试或跨 Run 事务。

## 决定

CLI、`ctx.run`、`kat_run` 共用 Rust 的执行/发布核心。Rust 管 Session lease、候选目录、独立 Runtime、直接子 Run ledger、文件归属和 Manifest 提交。Python 管 PACK 正式加载、Input Compiler、执行与 Parquet 语义。删除嵌套调用专有的 inspection Runtime；保留执行 Runtime 内正式 PACK 检查。

私有协议仅一个 `run_workflow`，输入为互斥的 Arguments / TypedInputs，不兼容旧请求。原始参数和严格 Python 标量不相互降级转换。生产和测试共用一个 JSONL pump。

`kat_run` 在每测试独立临时 Session 中运行真实独立 Runtime；同测试多次调用共享来源物化。它读取已发布 Catalog，继续返回 `dict[str, pyarrow.Table]`。固定被测 PACK，依赖从既有 roots 发现。删除进程内测试父 Workflow 的 Run scope 协议。pytest monkeypatch 不再影响实际 Workflow；普通 helper 单测仍可 monkeypatch。

安全业务诊断传给 `RunError`，详细诊断留在命令退出后可访问的日志；不增加稳定 phase、ID、path 或 retry 字段。

Scratch 收尾与发布判定只由 Rust 的 Run allocation 和执行核心负责。成功、业务失败、Host/协议失败以及已分配目录后的准备失败，都在受控退出时显式收尾；Runtime 及受管理子调用结束后才能开始清理。日志由执行核心持有到收尾完成，成功路径继续经过 Output/child ledger 检查、日志交付和唯一 Manifest 提交门。`Drop` 仅负责未发布项的幂等尽力回收，不产生业务诊断，也不能把它当作成功发布的验证证据。

收尾只接受当前 allocation 保存的精确 Scratch 路径，重新验证 Session 根和直接父目录；不跟随 symlink、junction 或 reparse point。普通目录删除后必须确认条目已不存在，原本缺失仍须验证父目录归属。文件或链接替换即使被安全回收也阻止发布；父目录归属无效则停止回收，不能删除外部目标。该边界不检测已恢复的替换历史或同路径普通目录重建，不增加 inode 快照、恢复状态或进程沙箱。

Python 只负责 Context 的调用期有效性、子调用收拢和失效，不再删除 Scratch 或向异常链追加清理失败。Scratch getter 只读取已分配的目录，删除后再次访问不能自动重建；Datasource getter 的既有行为不变。

成功执行的 Scratch 收尾失败由 Rust 清理诊断阻止发布。已有 Workflow/Output 或 Host/协议失败时，收尾是失败回收，保留原主诊断，独立的清理失败仅写入 Operation log，不合并为虚假的因果链。日志写入或最终交付失败仍按 [ADR-0037](0037-one-diagnostic-model-drives-json-and-terminal-output.md) 接管最终诊断，可读的部分日志保留路径。这局部取代 Python 将清理错误追加到公开 `causes` 的行为；[ADR-0077](0077-analysis-session-groups-multi-workflow-state.md) 的 Session lease 持续覆盖收尾，失败父级不影响成功子 Run 和共享物化。

Python writer 在首次发布前验证 footer/Schema/行数。Rust 不重建 PyArrow 类型展示器。Session inventory 校验 Manifest 身份、文件存在和布局，展示发布时元数据；不承诺此刻内容可查询。内容损坏在 PyArrow/DataFusion 实际读取时失败。

本决定局部替代 ADR-0078 的双预检、重复 footer 复核和进程内 `kat_run`，以及 ADR-0047 中 Workflow 执行受 pytest 模块 monkeypatch 影响的合同。其他组合、关闭收拢、lease 与 Manifest 提交语义不变。

## 最小切片与验证

先在真实 CLI/Host 上锁定丢失诊断，再收拢执行发布与协议，最后迁移 `kat_run`。验证直接/嵌套/测试三路输入及输出一致、诊断可追溯、并发关联/关闭、多层直接关系/孤儿、发布失败与 Session lease。最终运行 Rust 检查、Python/builder 测试、真实 Host 与 bundled PACK、Payload smoke。无输出 Run 单独由 ADR-0080 验收。
