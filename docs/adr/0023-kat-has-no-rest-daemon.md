---
status: accepted
---

# KAT 不提供 REST daemon

KAT 第一版删除整个 `kat-rs-daemon`、HTTP API、OpenAPI 以及 `serve`、`stop` 等服务生命周期命令，不为未来用途保留空壳。KAT Skill 为每次操作启动短命 KAT CLI；CLI 直接管理本地 Dataset 与 Run，只在需要列出或选择 PACK 时执行 PACK discovery，并在对应操作需要 Python 能力时通过文件式 KAT Runtime IPC 启动一次性 Workflow Runtime。继续保留 REST 层只会制造第二套部署、接口和生命周期，而当前没有需要常驻服务解决的问题。
