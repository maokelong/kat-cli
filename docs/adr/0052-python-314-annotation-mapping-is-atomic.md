---
status: accepted
---

# Python 3.14 annotation mapping 按原子边界读取

Bundled Python 精确锁定 3.14，并使用标准库 `annotationlib` 的 `Format.STRING` 读取 Workflow function 的 annotation mapping。KAT 只需要解析 `ctx` 与用户输入，不把 return annotation 当作 Output contract；但 Python 公开 Interface 只能原子调用 function 的 annotate function 取得完整 mapping，不能要求它只产生选定 key。即使使用 STRING format，少量常量表达式仍可能在 mapping 形成时执行或失败。

Input Compiler 对取得的 `ctx` 与用户输入字符串逐项交给 `typing.get_type_hints()`，不把 return 值交给它求值、校验或发布。普通 unresolved return forward reference 因 STRING format 保持字符串，不影响 inspection；若 annotate function 在形成完整 mapping 时已经失败，完整 PACK Interface 也失败。KAT 不为绕过这一原子标准库边界而读取 annotation 源码、拆解表达式、修改 bytecode、调用私有 API 或建立第二套 parser。

这意味着作者可以按普通 Python 惯例书写或省略 return annotation，但不能依赖一个本身无法被 Python 3.14 STRING mapping 安全取得的 return 表达式仍被 KAT 忽略。诊断归 PACK Interface 加载边界所有，不把 return annotation 提升为 KAT Output schema。

本决定修订 ADR-0035 中“return annotation 不得因批量解析完整 mapping 使 inspection 失败”的旧约束；其余“不求值、不校验、不展示 return annotation”决定继续有效。
