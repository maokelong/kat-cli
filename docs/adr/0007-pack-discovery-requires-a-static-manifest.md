---
status: accepted
---

# PACK 发现要求静态清单

每个 PACK 必须提供只含非空根级 `name`、`title`、`description` 和 `owner` 的静态 `pack.toml`，未知字段或表直接失败，PACK name 使用小写 ASCII kebab-case 并拒绝 Windows 保留设备名；这让 CLI 无需执行任意 Python 就能发现 PACK，并把拼写或身份错误立即暴露出来。只有需要列出或选择 PACK 的操作才从两个固定默认目录的一级子目录及每个显式精确 PACK directory 做短命发现，所有候选 canonicalize 后去重并整体校验；KAT 不维护 registry、递归扫描、来源 kind 或覆盖优先级，不同目录的同名 PACK 直接冲突。CLI 是 manifest 的唯一解析者，Runtime 只接收所选 PACK 的 name 与 canonical path 并从代码发现 Workflow，避免在 TOML 中重复维护运行 Interface。
