---
status: accepted
---

# PostgreSQL common 使用封闭 Arrow 类型映射

公共 PostgreSQL common 根据 PostgreSQL 返回列的类型身份使用明确、受测试的 PostgreSQL-to-Arrow scalar 映射，并在零行结果中仍从列描述构造确定 Schema；NULL 保持 NULL，受支持的整数、浮点、布尔、Decimal、文本、日期与时间值不被统一字符串化。未进入封闭映射的数组、复合、扩展或其他类型使查询明确失败并指出列与类型，Fixed SQL File 中的 SQL 可以显式 cast 到受支持类型；common 不使用只看样本行的 PyArrow 猜测，也不静默降级为 string。具体受支持类型集合由首个实现切片和回归测试锁定。
