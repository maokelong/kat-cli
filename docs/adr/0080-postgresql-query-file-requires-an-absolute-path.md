---
status: accepted
---

# PostgreSQL query_file 只接受绝对路径

公共 PostgreSQL common 的 `execute_sql_file()` 只接受调用 Workflow 明确提供的绝对文件系统路径，不相对于 Workflow Host 当前工作目录、PACK 根目录或隐式资源根解析。PACK 内文件由 Workflow 使用 `__file__` 自行构造绝对路径，外部共享文件可以使用固定绝对路径、Workflow 输入或由 Workflow 从环境配置构造。common 不设置允许读取的目录白名单；路径不是绝对路径、文件不存在或不可读时明确失败。
