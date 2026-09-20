# KAT 官方公共能力 SDK

SDK 是可选能力包，未安装或卸载后框架与不依赖它的 PACK 照常工作。SDK 提供公共 Provider、Workflow 及领域公共函数，框架 API 和 Runtime 保持在 `kat/platform/workflow`。公开导入使用 `kat_sdk`；旧的具体 Provider 导入不再保留。

## 构建与验证

在 CPython 3.14 环境安装构建工具后，从仓库根执行：

```text
python -m pip install build==1.6.1
python build/build_sdk_wheel.py --output target/sdk-wheel
python build/verify_sdk_install.py --kat <当前源码构建的CLI> --workflow-wheel <框架wheel> --datasource-wheel <当前平台原生wheel> --sdk-wheel <SDKwheel> --output target/sdk-verification
```

构建用标准 setuptools backend，Griffe 解析类型与 docstring，griffe2md 生成实际 Markdown。生成器只做静态分析；不会为了文档实例化 Provider 或加载原生解析器。构建依赖由 pyproject 固定，API MD 生成于临时构建目录，不提交生成物。发布 wheel 和对应 SHA256，安装时使用 KAT 当前 Python 的 `-m pip install --upgrade <wheel路径或URL>`。

当前 SDK 版本为 0.1.1，包含 demo 领域的公共库和 Workflow 示例，最低框架基线为 0.1.1rc13；已发布 rc12 不具备 SDK 发现能力。支持 CPython 3.14 的当前 Windows/Linux x86_64 部署。发布前在两种平台验证真实 wheel；普通 Python 能导入模块不等于具备 KAT 执行宿主。

验证脚本新建隔离 KAT 部署，先在未安装 SDK 时验证既有 PACK 的发现、执行、查询与测试，再安装指定 SDK wheel，检查公共发现、Guide、两种 Provider 实际查询，并构建两份仅测试用 SDK 验证新增 Workflow/Provider、直接执行、跨 PACK 调用、kat test、函数文档及旧文件清理。最后卸载 SDK 并重复既有 PACK 验证。验证期间保持 CLI 和框架版本不变，结果与 SHA256 写入输出目录的 `report.json`。外部 Trace Streamer 的完整 Trace 解码仍须由真实工具及样本另行验证；此脚本验证 SQLite 查询和受控解析器行为。

独立候选验收通过 GitHub Actions 的 `SDK CI` 手动触发：只构建一份 SDK wheel，两种平台下载同一 `kat-sdk-candidate` 产物后运行验证。两份 `sdk-evidence-*` 中的摘要必须与候选一致。确认成功后，将该候选中的 wheel 和 `.sha256` 原样上传至对应 SDK GitHub Release，不在验证后重新构建、不发布 PyPI。工作流不自动创建 Release。KAT 成套构建可选接收 SDK 路径、独立版本和 SHA256；三个参数一起提供或一起省略。官方成套 CI 仍可选择预装 SDK，不意味着框架依赖 SDK。

## 维护能力

新增或扩展 Workflow、Provider、公共函数前，执行 [开发前复用检查](../skills/kat-author/references/reuse-check.md)，先确认已有同类实现；开发 Workflow/Provider 时同时检查公共函数能否支撑构建，只实现明确的缺口。

Provider 与 helpers 模块通过 `__all__` 列出公开接口。Provider 的公开类声明名称、用途和 Guide，模块加入根模块的 `PROVIDER_MODULES`。实现通过框架公开 API 复用表工具；知识在 `knowledge/providers/`。

公共 Workflow 直接位于 SDK 根下的 `workflows/`，按领域分目录。每个入口自己定义一个 `@kat.workflow`，目录中不用 `__init__.py`；公共身份由装饰器声明，跨 Workflow 复用通过框架调用。不要把示例自动发布为正式能力。

公共函数是 `helpers/<领域>/` 中的普通 Python 模块，不注册为 KAT 能力。公共库的父包、领域包设置 `__init__.py`，并在 `pyproject.toml` 的 packages 中显式登记（如 `kat_sdk.helpers`、`kat_sdk.helpers.memory`）。API 参数与返回类型写注解，单位、限制、异常和示例写 docstring；公共函数的 API Markdown 与使用说明统一维护在 kat Skill。构建只为 Provider/Workflow 生成 `knowledge/<类别>/<模块相对路径>.api.md`，手写 Guide、教程和首页链接使用其他文件名，避免覆盖生成文件。

安装后的知识入口为 `kat_sdk/knowledge/index.md`，用 KAT 当前 Python 的 `importlib.resources.files("kat_sdk")` 定位，按相对链接读取当前版本。详细契约见仓库 `docs/specs/public-capability-sdk.md`。

## Demo 入门

SDK 0.1.1 中的 `helpers/demo/greeting.py` 提供 `build_greeting()`；`workflows/demo/greeting.py` 的 `demo-greeting` 调用该函数并返回一行 `message` 表。分别阅读 [公共库文档](../skills/kat/references/helpers/demo/greeting.md) 和 [Workflow 文档](knowledge/workflows/demo/greeting.md)，安装 wheel 后即可跟随示例执行。

公共库的使用介绍与 API Markdown 由 `kat/skills/kat/references/helpers/<领域>/` 维护并随 Skill 交付，SDK wheel 不携带公共库 Markdown。新增或修改公共库时同时更新 kat 的导航、介绍中的适用 SDK 版本，以及源码注解/docstring。
