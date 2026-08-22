---
status: accepted
---

# PostgreSQL common 跨平台交付并首先在 Windows 实库验证

`kat.common.sql.postgresql` 是 Workflow Host 的跨平台公共能力，不使用 Windows-only 包或导入标记。`kat-workflow` 正式依赖 `psycopg[binary]==3.3.4`，Windows 另通过环境 marker 依赖 `tzdata==2026.3`；由同一 `pyproject.toml` 重新生成 Windows 和 Linux 的正式锁文件，使两个 Payload 都能安装并导入该能力。

首个实现切片只要求在 Windows 开发环境完成真实 PostgreSQL 连接与端到端验收；Linux 只要求依赖锁、wheel 与 Payload 构建合同不被破坏，不增加 Linux 实库集成验证。现有 PostgreSQL PACK devkit 的二次扩展锁不再作为公共 common 的正式运行依赖，迁移或保留该开发包由实现切片按最小兼容范围处理。
