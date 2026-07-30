---
status: accepted
---

# PACK inspect 校验生产 Interface，test 执行原生 pytest

`kat inspect --pack` 在不创建 Dataset execution plane 的前提下校验并投影静态 manifest、生产 Python 导入和完整 Workflow Interface，零 Workflow 仍是合法结果；`run` 与 `test` 复用同一生产校验，因此 KAT 不再提供重复的 `check` 操作。`kat test` 只预检生产 Interface 与随 PACK 版本化的普通 Test Datasets，随后按 ADR-0047 由隔离于宿主配置但不作为安全沙箱的 Bundled pytest 原生拥有测试模块、`conftest.py`、显式 plugin、收集和状态；inspect/run 不扫描测试，纯运行部署可以省略测试，但显式 test 在缺少或未收集到测试时失败。KAT 的私有 plugin 只提供复用生产 Table Grant、Execution Lease 与 Output publication 的 `kat_run` fixture，并返回 PyArrow Tables；pytest 负责 terminal/JUnit 报告与 node ID，KAT 负责 Operation log 和交付门，只有 pytest `OK` 且日志与报告均已交付才成功，最终失败归属遵循 ADR 0037。
