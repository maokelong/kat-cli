---
status: accepted
---

# 公共 common 通过 kat.common 导入

公共 common 的源码目录与 Workflow `api/`、`runtime/` 并列位于 `kat/platform/workflow/common/`，但在同一个私有 Workflow Host wheel 中映射为 `kat.common` 子包。PACK 使用 `from kat.common.sql import postgresql`，再调用 `postgresql.execute_sql_file()` 或 `postgresql.execute_sql_text()`；不发布容易与第三方模块冲突的顶层 `common` 包，也不建立独立 common wheel、版本或安装流程。

第一版只增加实际需要的 `kat.common.sql.postgresql` 能力，不保留已取消 ResourceCatalog 方案所需的 `common.resources`，也不预建空的分析领域目录、驱动注册表或通用数据库基类。External PACK 与 Bundled PACK 使用同一个 wheel 内公共实现。
