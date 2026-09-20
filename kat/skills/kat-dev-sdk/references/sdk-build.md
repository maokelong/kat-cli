# 构建与验收 kat-sdk

需要从源码生成 `kat-sdk` Python 库、检查 wheel 或验证安装升级时读取本指南。能力实现与知识布局见 [SDK 开发指南](sdk-development.md)。以下命令从 KAT 源码仓库根目录执行；Skill 安装目录不包含构建脚本，必须先定位源码仓库。

## 1. 准备构建环境

使用符合 `kat/sdk/pyproject.toml` 中 `requires-python` 的 Python；当前要求 CPython 3.14。Windows PowerShell 首次准备：

```powershell
py -3.14 -m venv .venv-sdk-build
if ($LASTEXITCODE -ne 0) { throw "创建构建环境失败" }
$buildPython = (Resolve-Path .venv-sdk-build/Scripts/python.exe).Path
& $buildPython -m pip install build==1.6.1
if ($LASTEXITCODE -ne 0) { throw "安装构建工具失败" }
```

没有 `py` 启动器时，用已安装的 CPython 3.14 解释器绝对路径替换 `py -3.14`。后续构建复用该环境即可。构建后端及 API 文档生成依赖已固定在 SDK 的 `pyproject.toml` 中，由构建隔离环境自动安装；首次构建需要能取得这些依赖。此脚本构建纯 Python SDK，不需要编译 Rust 或生成整套 KAT Release。

## 2. 执行一个脚本生成库

环境准备完成后，执行：

```powershell
$buildPython = (Resolve-Path .venv-sdk-build/Scripts/python.exe).Path
$output = "target/sdk-wheel-$(Get-Date -Format yyyyMMdd-HHmmss)"
& $buildPython build/build_sdk_wheel.py --output $output
if ($LASTEXITCODE -ne 0) { throw "SDK 构建失败，请检查上方日志" }
```

Linux 使用满足版本要求的开发 Python，安装同版本 `build` 后执行 `python build/build_sdk_wheel.py --output target/sdk-wheel`。

`--output` 必须是尚不存在的新目录，且不能位于 `kat/sdk/` 内；重建时换一个目录。版本来自 `kat/sdk/pyproject.toml` 的 `project.version`，独立于框架版本。正式升级先修改 SDK 版本并验证依赖兼容范围；可加 `--expected-version 0.1.1` 校验期望版本，此参数不会修改版本号。

脚本复制源码到临时目录，静态生成 Provider/Workflow API Markdown、校验知识链接，构建并检查 wheel，最后输出 wheel 和 SHA256 文件的路径。它不会自动安装或上传产物。

## 3. 检查生成产物

以 SDK 0.1.1 为例：

```text
target/sdk-wheel-<时间>/
├─ kat_sdk-0.1.1-py3-none-any.whl
└─ kat_sdk-0.1.1-py3-none-any.whl.sha256
```

查看文件、校验摘要并列出 wheel 内容：

```powershell
Get-ChildItem -LiteralPath $output
$wheels = @(Get-ChildItem -LiteralPath $output -Filter *.whl)
if ($wheels.Count -ne 1) { throw "预期只有一个 SDK wheel" }
$wheel = $wheels[0]
$expected = ((Get-Content -LiteralPath ($wheel.FullName + ".sha256") -Raw).Trim() -split "\s+")[0]
$actual = (Get-FileHash -LiteralPath $wheel.FullName -Algorithm SHA256).Hash
if ($actual -ne $expected) { throw "SDK SHA256 不匹配" }
& $buildPython -m zipfile -l $wheel.FullName
if ($LASTEXITCODE -ne 0) { throw "读取 wheel 失败" }
```

wheel 内主要结构如下，具体模块和领域以此次交付为准：

```text
kat_sdk/
├─ __init__.py
├─ pack.toml
├─ providers/
├─ workflows/<领域>/*.py
├─ helpers/<领域>/*.py
└─ knowledge/
   ├─ index.md
   ├─ providers/
   └─ workflows/<领域>/
kat_sdk-<版本>.dist-info/
```

确认新增模块、声明、Guide、生成 API 和导航均已进入 wheel。脚本拒绝测试与构建工具混入，但还需人工核对本次新增能力是否齐全。公共库的使用说明与 API Markdown 全部在 `kat/skills/kat/references/helpers/`，随 kat Skill 交付；它不在 SDK wheel 内，SDK 不生成或携带 `knowledge/helpers/`；修改公共库文档时也要同步交付 Skill。

构建 wheel 后还要检查 kat Skill 中的 Workflow、Provider 和公共库介绍及分类导航是否覆盖新增能力，适用版本、命令和导入路径是否一致。这些介绍随 Skill 交付，wheel 构建不会自动更新它们。

## 4. 常见构建问题

| 现象 | 处理 |
| --- | --- |
| 输出目录已存在 | 指定新的 `--output` 目录 |
| 找不到 `build` 模块 | 用同一个 `$buildPython` 安装 `build==1.6.1` |
| Guide 或本地链接校验失败 | 修正声明路径及知识导航，重新构建 |
| 生成 API 与手写文件冲突 | 不在源码维护同名 `*.api.md`，将手写说明放入独立 Guide |
| 新公共库未进入 wheel | 检查领域包的 `__init__.py` 和 `tool.setuptools.packages` 显式登记 |

## 5. 在真实 KAT 环境安装和验收

优先使用隔离的测试部署。保持当前 CLI、框架和原生依赖不变，在其相邻 Python 中安装已构建 wheel：

```text
<bundled-python> -m pip install --no-deps --upgrade <SDK-wheel绝对路径>
<bundled-python> -m pip show kat-sdk
<bundled-python> -m pip check
```

这里的 `--no-deps` 用于已有兼容依赖的 SDK 独立升级验收；它不解决依赖缺失或版本冲突。若 `pip check` 失败，先准备满足 SDK 元数据要求的受支持 KAT 部署。PowerShell 调用带引号的解释器路径时加 `&`。

按新增能力执行 inspection、实际调用和行为测试：

```text
kat inspect
kat inspect provider
kat inspect provider --provider <名称>
kat inspect workflow --pack kat-sdk
kat inspect workflow --pack kat-sdk --workflow <名称>
kat session create
kat run --session <id> --pack kat-sdk --workflow <名称> -- <业务参数>
kat test --pack-dir <已安装kat_sdk目录的绝对路径>
```

最后一条只在候选 PACK 配有测试时使用；正式 SDK wheel 不携带 `tests/`，不能把空测试集当成验证。SDK 源码测试用同一宿主的 `-I -B -m pytest <仓库>/kat/sdk/tests -q` 运行。公共函数另行验证隔离导入、返回值和知识导航。

源码仓库提供完整安装及升级验收脚本：

```text
python build/verify_sdk_install.py --kat <当前CLI> --workflow-wheel <框架wheel> --datasource-wheel <当前平台原生wheel> --sdk-wheel <SDKwheel> --output <新的验收目录>
```

脚本在独立环境中验证未安装、安装、升级、卸载，使用仅测试用能力检查发现和知识更新。它不能替代新增业务能力的专门测试。交付须覆盖：

- 新能力的发现、知识读取与真实执行；需要外部工具的能力明确记录实际工具和样本。
- SDK 升级后旧能力仍可使用，CLI 与框架版本保持不变。
- 未安装及卸载后，原有 PACK 的发现和不依赖 SDK 的执行继续可用。
- 当前支持的 Windows/Linux 环境分别验收；纯 Python wheel 不代表底层依赖已经跨平台验证。

交付说明 SDK 版本、wheel 路径、SHA256、已验证的平台和行为，以及未完成的验证。正式发布使用通过验收的同一 wheel 与校验文件；当前流程不发布 PyPI，也不在分析过程中自动更新 SDK。
