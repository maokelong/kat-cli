---
status: accepted
---

# SDK 统一源码归属与安装交付

作者声明、通用数据框架、具体 Provider、原生解码、Runtime 与 Markdown 统一归 kat/sdk，构建成一个 kat-sdk distribution。CLI 及解释器由原有完整 Skill 交付，SDK 更新沿用原环境。

kat/sdk/kat 直接对应安装目录。_declarations 保存作者接口，dataprovider 保存通用数据框架，providers 按来源组织实现，providers/decoding 提供自定义 Provider 使用的窄解码能力；_runtime 是 CLI 内部执行实现。框架不导入具体 Provider。Rust 代码在 native，编译扩展安装到 providers._native。此布局替代 ADR-0012 的 platform 源码归属，保持源码与 Skill 部署视图分离。

随包知识集中在 kat/knowledge，按 authoring 和 providers/<来源> 分层。原有 pack-authoring-flow.md 完整迁入 authoring，声明、Context、数据框架与创作流程只维护这一份正文，Skill 保留读取入口。knowledge 提供总导航与统一 read()，公共 Provider inspection 从 knowledge 读取其 guide，PACK 自有知识规则不变；此决定调整 ADR-0081 的公共 Provider 资源位置。

SDK 根 pyproject 使用 Maturin 直接构建完整 wheel，替代 ADR-0045 的纯 Python 中间 wheel 及拆分安装边界。RECORD、平台标签与打包交给成熟 backend，不自行拼接多个 distributions。Python 测试集中按职责分组，Rust 测试随 native。

公共根入口导出 workflow、provider、Context 等；具体 Provider 从 kat.providers 导入。Python distribution 名为 kat-sdk，导入名为 kat。动态当前 PACK 命名空间继续为 kat.pack。旧模块和兼容别名不保留，CLI 切换到 kat._runtime。

完整 Skill 保持四个同级入口、scripts/targets/<平台> 与 assets/packs。首次迁移同步 CLI 和 PACK；之后经验证兼容的 SDK 可单独升级。维护流程见 kat/sdk/README.md，用户更新流程归 kat Skill。CLI pip 包、插件注册机制、PyPI 发布和自动更新不在本决定内。
