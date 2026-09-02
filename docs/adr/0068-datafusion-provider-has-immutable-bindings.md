---
status: accepted
---

# DataFusion Provider 使用不可变 relation 绑定并可重复查询

`dp.DataFusionProvider(tables=..., catalog=...)` 在构造时复制 `tables` Mapping 的结构并校验 relation 集合，随后不提供 `register()`、`remove()`、`replace()` 或其他可变 catalog API。修改原 Mapping 不改变 Provider；需要更换 relation 集合时，调用方构造新的 Provider。Provider 可以重复调用 `query()`，每次查询都创建独立短命 Session，并注册构造时绑定的不可变 Table。

Provider 保留构造参数中 Table 与 Catalog 的普通 Python 强引用。Provider 存活期间，这些输入及其必要 backing 不会因调用方删除原变量而释放；Provider 离开作用域且不存在其他引用后，再按普通 Python 生命周期回收，不要求日常显式 `del`。不可变 Table 在两次查询之间保持相同内容，每次查询返回的结果 Table 也是不可变值；需要更换 Table 时，调用方构造新的 Provider。

这项边界以构造新对象表达 relation 集合变化，避免重新引入 operation catalog、Binding 或隐式全局注册，同时允许同一组稳定来源重复执行不同 SQL。它细化 ADR-0076 确认的 Table 不可变与引用语义、ADR-0066 的短命 Session 语义，以及 ADR-0067 的 Catalog 输入边界。
