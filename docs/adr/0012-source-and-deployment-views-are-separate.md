---
status: accepted
---

# 源码视图与部署视图分离

源码按 `kat/skill`、`kat/platform/{cli,datasource,workflow}` 与 `kat/packs` 分责：CLI 依赖拥有 Dataset Storage 与 Datasource 的单一 Rust package，跨 Rust package 必须公开的 symbols 只服务同版本 workspace，不形成 SDK；Workflow API 与 Runtime 属于同一构建单元，其中面向 External PACK 的 Pack Authoring API 仍是公共产品 Interface，但不成为独立 distribution 或升级面。内部 Module 只在真实调用者和生命周期出现时建立，PACK discovery、Dataset Storage、response 与应用编排各自隐藏实现，不为测试或想象中的复用预建通用 core、trait、registry、配置或横向层。部署由平台 Builder 产出黑盒 Payload，再由不理解 Payload 内部结构的薄 Skill Assembly Adapter 与 Skill 元数据、平台无关 Bundled PACK 组装标准 `dist/kat` 并独占其写入；Source view、Python import namespace 与 Skill deployment view 因职责不同而不追求目录同构。
