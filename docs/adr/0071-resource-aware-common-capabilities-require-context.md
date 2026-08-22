---
status: superseded by ADR-0079
---

# Resource-aware common 能力显式接收 Workflow Context

需要本次 Resource discovery、数据库连接或其他 KAT-managed 运行环境的公共 common 函数必须把 `kat.Context` 作为第一个显式参数；Runtime 将本次可用 ResourceCatalog 绑定到 Workflow Context，common 只依赖公共 API seam，不导入 `_kat_runtime`，也不使用 module global 保存隐式当前 catalog。此 Context 依赖不形成 `required_sources`、grant 或 allowlist，受信任 Workflow 可以按稳定名称解析本次发现到的 resource；不需要执行环境的纯计算 common 函数继续不接收 Context。本决定扩展 ADR-0032 的同一显式能力原则。
