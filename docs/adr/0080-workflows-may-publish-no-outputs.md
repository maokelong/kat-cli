# ADR-0080：纯编排 Workflow 可以不产生 Output

状态：接受。关联：[#253](https://github.com/maokelong/kat-cli/issues/253)。局部替代 ADR-0078 的至少一个 Output 限制。

## 决定

Workflow 返回 `None`（含隐式 return）表示无 Output，仍经过普通发布门，生成 `outputs: {}` 和实际直接 `child_runs`。空 dict 返回继续拒绝，避免两种无输出写法；零行、有 Schema 的 Table 仍是正常命名 Output。

`ctx.run` 始终返回 Catalog，无输出时 `tables == ()`。`dp.open(tables={})` 可构造空 Catalog；空 `root` 的拒绝规则不变，避免把尚未解析的数据误认为缓存命中。`kat_run` 返回 `{}`，CLI 和 inventory 正常展示无输出 Run；query 不编造关系。

Guide 仍只解释所属 Workflow；缺省可以不解释。无输出或零行均不意味着没有问题，需要时按子 Run 自己的 Guide 读取证据。没有新的 Workflow 类型或 AI 调度运行时。

## 验证

在作者 API 和真实 Host 上验证 None、空 dict 拒绝、零行表、空 Catalog、纯编排多层发布与直接子关系、父失败保留成功子 Run、inventory/query/kat_run。提供可执行组合示例及 Guide。
