# 汇总结果解释

先查询当前 Run 的 `output.main`：`samples` 是条数，`total` 是值之和。默认示例是 2 条、合计 42；不是性能结论。

需要核对来源时，通过 Session inventory 的当前 Run `child_runs` 定位直接 facts 子 Run，再读取其 Workflow detail Guide 和最少原始证据。父 Guide 只解释当前汇总；子结果仍按子 Guide 解释。不要把不同 Run 的 facts 当作同一份样本重复累加。
