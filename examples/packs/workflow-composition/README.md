# 一个 Workflow 模型，三种普通用法

无外部服务或输入文件依赖。所有入口都是普通 Python Workflow，不引入父/子类型或调度 DSL。

- facts 生成两条示例事实。
- summarize 调用 facts，从只读 Catalog 查询汇总，返回自己的 Table。
- collect 顺序调用 facts 和 summarize，隐式返回 None；当前 Run 没有表，Manifest 保留两个直接子 Run。summarize 的 facts 后代不提升为 collect 的直接子 Run。

## 运行与验证

从仓库根目录执行，替换命令中的 Session ID：

```text
kat session create
kat inspect workflow --pack workflow-composition --workflow collect --pack-dir examples/packs/workflow-composition
kat run --session <SESSION_ID> --pack workflow-composition --workflow collect --pack-dir examples/packs/workflow-composition
kat inspect session --session <SESSION_ID>
kat query --session <SESSION_ID> --run <SUMMARIZE_RUN_ID> --sql "SELECT samples, total FROM output.main"
kat test --pack-dir examples/packs/workflow-composition
```

collect 一次执行产生 4 个 Run：collect、直接 facts、直接 summarize 和 summarize 的 facts。默认汇总为 2 条、合计 42。读 Guide → 执行 → inventory → 最少证据 → 分别解释，必要时才追加 Workflow。临时串联可先由 AI 逐次调用 kat run 并复用 Session；需要固定顺序时写成 collect 一样的 Python 入口。

`ctx.run` 返回 Catalog，不回传整表；每个 Workflow 在独立 Runtime 执行。测试的 `kat_run` 走同一执行/发布核心，再便利地读取 `dict[str, pyarrow.Table]`。pytest monkeypatch 不会传入 Workflow。空 dict 返回不合法；None 与有 Schema 的零行 Table 意义不同。
