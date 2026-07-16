---
status: accepted
---

# PACK manifest 携带非权威 owner

每个 `pack.toml` 必须声明唯一一个非空、可读的 owner；PACK discovery 负责校验该字段，inspect 将它作为随 PACK 分发的维护责任信息。缺失或空值由 PACK discovery 拒绝。owner 不参与 PACK name、重名判断、namespace、发布者认证或运行权限，同一 owner 可以出现在多个 PACK 中且修改它不改变 PACK identity；真实来源可信度仍由 PACK 的分发与审查渠道提供，以一个低成本字段换取私有部署和源码迁移后仍可见的组织归属，而不引入 publisher namespace、owner 唯一性约束或权限系统。
