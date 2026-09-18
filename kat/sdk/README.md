# KAT SDK 维护指南

kat-sdk 是 pip distribution 名，Python 导入名为 kat。kat-sdk 是唯一 Python 安装包，包含作者声明、通用数据框架、公共 Provider、内部 Runtime 和随版本知识。CLI 和解释器由完整 Skill 提供。

## 源码与依赖

~~~text
kat/sdk/
├── pyproject.toml          # 唯一 Python 构建入口
├── kat/                   # 与安装后 Python 包结构一致
│   ├── __init__.py        # workflow、provider、Context 等公共入口
│   ├── _declarations/     # 声明和公共类型
│   ├── dataprovider/      # Table、Schema、查询与写入
│   ├── providers/         # ftrace/、trace_streamer/ 具体实现
│   │   └── decoding/      # 自定义 Provider 可调用的窄解码接口
│   ├── _runtime/          # CLI 使用的内部执行实现
│   └── knowledge/         # 随包 Markdown、总导航与统一读取入口
│       ├── index.md
│       ├── authoring/     # 完整 pack-authoring-flow.md
│       └── providers/     # ftrace/guide.md、trace_streamer/guide.md
├── native/                # Rust 解码、协议、代码生成和 Rust 测试
└── tests/                 # authoring、dataprovider、providers、runtime、packaging
~~~

具体 Provider 依赖数据框架，框架不导入具体来源。Rust 扩展安装为 kat.providers._native，由解码接口封装。SDK 不保留 api、datasource、runtime 的旧顶层命名空间。CLI 启动 kat._runtime；PACK 通过 from kat import workflow, provider, Context 和 from kat import dataprovider as dp 使用作者接口。

## 构建

先运行 python build/verify_release_versions.py。在 build/runtime-inputs.json 对应的 Windows x86_64 或 manylinux 2.28 x86_64 Builder 上安装锁定构建依赖，然后运行：

~~~text
python build/build_sdk_wheel.py --python <CPython3.14路径> --platform <windows-x86_64或linux-x86_64> --expected-version <PEP440版本> --output <全新目录>
~~~

Maturin 从 SDK 根 pyproject.toml 直接构建完整 kat-sdk，输出 wheel、SHA256SUMS 和同名 .sha256；没有 API/Runtime/Datasource 中间 wheel。开发环境也可使用 python -m pip wheel --no-deps ./kat/sdk，发布候选使用上述平台校验脚本。

Skill Payload Builder 使用 --sdk-wheel、--sdk-wheel-version、--sdk-wheel-sha256 接收同平台 SDK，安装到私有 Python。完整 Skill 的 scripts/targets 与 assets/packs 布局保持不变。

## 检查

在新 CPython 3.14 环境安装最终 wheel，运行 pip check、build/fixtures/verify_installed_sdk.py、kat/sdk/tests/packaging/test_knowledge.py。所有模块与 Markdown 必须由一个 kat_sdk-<版本>.dist-info 的 RECORD 管理，环境中没有 kat-workflow、kat-datasource、kat-cli distributions。

Python 测试按 tests/ 下各职责目录执行；原生测试运行 cargo test --locked -p kat-datasource。完整 SDK 回归可使用 python -I -B -m pytest -q -p no:cacheprovider kat/sdk/tests；Runtime 测试前需构建当前原生 CLI。

运行 python build/fixtures/verify_sdk_upgrade.py --wheels <SDK目录> --cli <原生CLI路径> --work <全新测试目录>，验证相邻 Python、CLI、受检依赖、用户 PACK、数据和旧 Run 保留，升级前后 Inspect/Test/Run/Query 可用。

从旧命名空间迁移需同步更新 CLI 的 Runtime 入口和 PACK 导入；旧拆分 distributions 先卸载再安装统一 SDK，避免旧 RECORD 删除新文件。之后兼容 SDK 更新可保留 Python 与 CLI。

## 发布

仅在用户请求发布时执行；只要求构建则交付候选。SDK 标签 sdk/<源码版本> 指向准确构建 SHA；kat/<版本> 仍发布完整 Skill。PyPI 尚未配置。

核对成功 CI 的 checkout SHA、平台、版本和摘要。先创建草稿，上传两平台 wheel 与摘要，重新下载校验后发布；rc/dev 标为 prerelease，不设 Latest，不覆盖已发布同名资产。说明列明 Python ABI、平台、源码 SHA、测试证据与迁移要求，发布后从公开资产重新安装验证。Windows builder-image smoke 不替代 #143 干净客户端验收。

随包 Markdown 集中在 kat/knowledge，按功能与来源分层，index.md 提供导航；工程维护文档保留在本 README；接口变化同步签名、限制、示例和错误语义。用户流程见 [SDK 安装与升级](../skills/kat/references/sdk-install.md)。
