---
status: accepted
---

# Skill 与运行时原子发布

KAT 将 Skill 定义及选择逻辑、KAT CLI、Bundled Python Host、Bundled PACK 与各平台二进制载荷按同一版本原子发布，只承诺 Skill constraints 稳定，不为底层 CLI、私有 Python Host 或 Pack Authoring API 提供独立版本与兼容面，以避免组件独立升级造成协议错配。成熟发布工具在原生平台构建 Linux x86_64（glibc 2.28 及以上）与 Windows 10 及以上 x86_64 客户端的完整离线载荷，再由薄 Skill Assembly Adapter 汇总；载荷包含固定、可重定位的 Python 与依赖闭包，Windows 依赖使用 app-local VC Runtime，不在用户机器下载、安装或回退到用户环境。运行时按平台拒绝不受支持目标，KAT 平台不修改 Skill、Platform Payload 或 PACK 源码，持久状态写入平台标准 KAT Data Home（用户仍可另选 Dataset 位置），从而使部署可整体替换且不与状态混杂。
