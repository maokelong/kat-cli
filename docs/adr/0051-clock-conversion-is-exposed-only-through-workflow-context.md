---
status: accepted
---

# 第一版时钟换算只通过 Workflow Context 暴露

本决定部分取代 ADR-0005、ADR-0009、ADR-0032、ADR-0042 与 ADR-0043 中关于 SQL
`kat_convert_clock(...)` 和 `ctx.convert_clock(...)` 同时开放的内容，并保留这些
ADR 的其余决定。ADR-0045 规定的单一纯 Python Workflow Host wheel 继续有效。

DataFusion 54 的 Python `ScalarUDF` 只接受固定输入字段和固定返回字段，没有暴露
规划期的 `return_field_from_args`、logical analyzer rule 或通用 Logical Plan
visitor。Python UDF 真正执行时，字符串字面量、字符串列和已绑定参数都表现为普通
PyArrow array；参数绑定后的 Logical Plan 也不再保留目标来自参数的事实。因此，
Runtime 私有的纯 Python UDF 无法可靠证明第三个 SQL 参数是普通字符串字面量。

KAT 不用第二套 SQL parser 推测 DataFusion 语法，不维护 SQL/Logical Plan 类型白名单，
也不为了该门禁引入 PyO3 module、FFI capsule、native wheel 或 Rust UDF。第一版不再向
`SessionContext` 注册 `kat_convert_clock`，所以普通 `ctx.sql(...)` 中直接调用该名称
按照 DataFusion 的函数不存在语义失败；KAT 不扫描 SQL，也不定制该错误。

时钟换算只通过 Pack Authoring API 的
`ctx.convert_clock(clock_domain, clock_value, *, target_domain)` 暴露。该方法在构造
Expr 前要求 `type(target_domain) is str` 且字符串非空，因此拒绝空字符串、`None`、
其他类型和 `str` 子类。它随后把固定目标构造成 literal Expr，并调用 Runtime 私有的
`stable` Python/PyArrow scalar UDF。UDF 不注册为 SQL 名称，但同一个 UDF object 负责
所有 Context 时钟 Expr 的执行。

DataFusion 54 Python API 没有公开 schema-aware 类型查询，Python UDF 也没有规划期
coercion callback。把 `arrow_typeof(...)` 作为额外 UDF 参数只会在非零关系中产生行值，
零行关系无法据此证明来源类型；同一个 Logical Plan 会因行数不同而出现不同准入结果。
因此 `ctx.convert_clock(...)` 在构造私有 UDF Expr 时显式把两个来源 Expr 严格 cast 为
`Utf8` 与 `UInt64`，再由固定 UDF signature 接收规范类型。安全可转换的
`LargeUtf8`、`Utf8View` 或可表示的非负有符号整数可以使用；负数、越界、非法文本或
其他不安全转换使 Workflow 失败，不使用 `try_cast` 产生 NULL。KAT 明确保证规范
`Utf8`/`UInt64`、`LargeUtf8`/`Utf8View` domain 与可表示的非负 `Int64`；
其他来源类型即使能被固定版本引擎转换，也不属于 Pack Authoring API。零行与非零行
关系服从同一规划和转换合同。

实际换算继续对整批 Arrow arrays 使用 PyArrow checked kernels，不调用 `.as_py()`，
不建立 Python per-row loop。ADR-0042 已有的全空传播、半空失败、时钟定义、同域恒等、
跨域 `snapshot_id = 0`、缺失证据和 `u64` 越界语义均不改变。普通
`ctx.sql(...)`、参数绑定、`SHOW`、`DESCRIBE` 与 `EXPLAIN` 不经过任何时钟专用
计划检查。

以后只有 DataFusion Python API 提供可靠的规划期参数检查回调或等价的完整官方
visitor，并且真实 PACK 证明需要 SQL 入口时，才重新设计和交付 SQL
`kat_convert_clock(...)`。在此之前，函数未注册是公共 Pack Authoring API 的明确边界，
不是待补齐的兼容缺口。
