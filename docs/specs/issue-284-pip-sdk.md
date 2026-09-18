# KAT SDK 统一安装交付

关联：Issue #284；交付：PR #285；架构：ADR-0083。

目标是用单个 pip 安装包升级 SDK，保留 Skill 自带 Python、CLI、用户 PACK 与数据。SDK 包含作者声明、数据框架、公共 Provider、原生解码、Runtime 和 Markdown；不包含 CLI、解释器或用户 PACK。

Python 导入名统一为 kat，pip distribution 仍为 kat-sdk；不提供 kat_sdk 导入别名。源码在 kat/sdk/kat，直接对应安装后的 kat：_declarations 拥有作者声明与类型，dataprovider 只拥有通用表/查询/写入，providers 拥有具体来源实现，providers/decoding 保留自定义 Provider 的窄解码接口，_runtime 拥有内部执行。公共入口仍为 from kat import workflow, provider, Context。不保留旧模块别名。

随包 Markdown 集中于 knowledge，按 authoring、providers/<来源> 分层，原 pack-authoring-flow.md 完整迁入 authoring，统一拥有声明、Context、数据框架与创作流程知识，Skill 只保存读取入口；index.md 导航与 read() 入口统一访问，每份知识只保留一份。具体 Provider 依赖框架，框架不导入扩展。Rust 源码与原生测试在 native，编译扩展为 providers._native。Python 测试按 authoring/dataprovider/providers/runtime/packaging 分组。

SDK 根 pyproject 使用成熟 Maturin 直接构建唯一平台 wheel，由标准 backend 维护 RECORD；取消 API/Runtime/Datasource 中间 distributions 和手工合并。构建/检查/发布归 kat/sdk/README.md，用户下载和 pip 更新归 kat Skill。ABI 为 CPython 3.14，平台为 Windows x86_64 与 manylinux 2.28 x86_64。

CLI 只随完整 Skill 发布并使用相邻 Python。完整 Skill 保持四个同级入口、kat/scripts/targets/<平台>/ 与 kat/assets/packs/。首次切换命名空间需同步 CLI 与 PACK；后续兼容 SDK 升级保留 Python、CLI 和依赖。旧拆分安装先卸载原 distributions 再安装 SDK，避免文件归属交叉。

验证：标准构建与完整 wheel 文件归属；所有集中分层的 Markdown 及示例可读取执行；框架不会加载具体 Provider；Python/Rust 回归；Skill 原生 CLI 升级前后 Inspect/Test/Run/Query 和旧 Run 可用；Python、CLI、受检依赖、用户 PACK 与数据保留；双平台 SDK/Skill CI。

非目标：CLI pip 包、发布 PyPI、插件注册机制、自动更新器、保留旧导入兼容层。
