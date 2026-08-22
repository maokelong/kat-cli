---
status: accepted
---

# PostgreSQL common 保留原生错误类型

公共 PostgreSQL common 第一版不建立自定义公共异常层级。调用形状和绝对路径合同错误使用 `TypeError` 或 `ValueError`，文件不存在、不可读或编码无效保留标准文件与 Unicode 异常，连接、认证和 SQL 执行失败保留原始 `psycopg.Error`，rowset 形状与 PostgreSQL-to-Arrow 封闭映射失败使用明确的 `ValueError`。Workflow Runtime 继续把这些异常统一归入 Workflow 执行失败。

common 不主动记录 libpq 环境变量、密码或参数值；保留原生异常是为了提供数据库诊断事实，不形成将秘密写入日志或 Run 的授权。若将来真实 PACK 需要稳定捕获分类错误，再基于消费者需求设计 KAT-owned 异常。
