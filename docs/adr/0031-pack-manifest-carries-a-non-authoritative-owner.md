---
status: accepted
---

# PACK manifest 携带非权威 owner

每个 `pack.toml` 必须声明一个非空、可读的 owner，并由 discovery 校验、由 inspect 展示，使维护责任在私有部署和源码迁移后仍可见。

owner 只是非权威归属信息，不参与 PACK identity、namespace、发布者认证、权限或唯一性；真实可信度仍由分发与审查渠道提供。
