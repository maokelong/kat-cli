---
status: accepted
---

# 外部 PACK 是一等部署单位

KAT 从第一版支持 External PACK 作为自包含的一等扩展与发布边界，因为外部领域策略是核心产品能力；它只交付源码、领域资源和可选的同目录测试/Test Dataset，Python 依赖由 KAT Skill 统一提供，纯运行部署可以省略测试但显式 `kat test` 必须失败。Bundled PACK 与 External PACK 只在交付来源上不同，使用相同的 Pack Authoring API、Runtime 权限和扁平 PACK name 作用域；来源不是公共 kind、身份或特权，`kat-` 前缀不保留，不同 canonical directories 的同名 PACK 直接冲突。KAT 不维护 registry，只在需要列出或选择 PACK 时从 Skill、KAT Data Home 和显式精确 PACK directories 做短命发现并按 canonical directory 去重；第一版 PACK 彼此不能依赖、导入或调用，以保持独立部署。
