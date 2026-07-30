---
status: accepted
---

# Workflow 参数语义只属于 Python Runtime

`kat run` 用 `--` 分隔固定路由参数与 Workflow arguments，Rust CLI 只传递原始字符串且始终把 Dataset 当作可选路由输入，不解释类型、默认值或 Required tables，也不按 Workflow 动态生成 Clap Interface，从而避免 Rust 与 Python 两套参数语义和启发式转换。

Python Runtime 从顶层同步 Workflow 的函数签名、原生延迟标注和 `kat.workflow` metadata 生成唯一 Input Compiler，并以锁定的 Click 同时驱动 inspection、生产 run 与 `kat_run`；首参必须是显式 `kat.Context`，其余输入只接受封闭的标量、时间与字符串 Literal 集合，默认值、必填性和 choices 只来自签名，说明只来自 decorator/docstring，return annotation 不构成 Output 合同，PACK 也不能另写 parser。

Runtime 在执行前用同一编译结果解析命名 options、校验 Required tables、构造领域值并记录规范化输入，inspection 只发布 Skill 调用所需的稳定约束；第一版不支持位置参数、复杂 model、params file、通用 Schema、自定义类型或解析器，使数据留在 Dataset、稳定策略留在 PACK。
