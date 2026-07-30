---
status: superseded by ADR-0047
---

# PACK name 拥有私有 Python namespace

> 历史决定，请勿实现；当前设计见 ADR-0047。

历史设计将每个精确 PACK name 映射到独立私有 Python namespace，并用标准 importlib 以所选 PACK directory 作为唯一搜索位置，让 inspect、run 与 test 共享加载语义且不修改 `sys.path`、复制源码或允许跨 PACK import。测试原计划通过 pytest importlib adapter 进入同一 namespace、仅支持根级 `conftest.py`，同一进程只加载一次 module 而每次调用重建 execution plane；这些约束用于避免两套 module identity，但已由 ADR-0047 取代。
