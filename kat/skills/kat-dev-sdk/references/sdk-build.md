# 构建与验收 kat-sdk

需要从源码构建 `kat-sdk`、检查 wheel 或验证安装升级时读取本指南。能力实现、API 文档生成和 Runtime Guide 布局见 [SDK 开发指南](sdk-development.md)。命令从 KAT 源码仓库根目录执行。

## 1. 准备构建环境

使用满足 `kat/sdk/pyproject.toml` 中 `requires-python` 的 Python；当前要求 CPython 3.14。Windows PowerShell 首次准备：

```powershell
py -3.14 -m venv .venv-sdk-build
if ($LASTEXITCODE -ne 0) { throw "创建构建环境失败" }
$buildPython = (Resolve-Path .venv-sdk-build/Scripts/python.exe).Path
& $buildPython -m pip install build==1.6.1
if ($LASTEXITCODE -ne 0) { throw "安装构建工具失败" }
```

没有 `py` 启动器时使用 CPython 3.14 解释器绝对路径。构建后端依赖由 `pyproject.toml` 的隔离构建环境安装；纯 Python SDK 构建不需要编译 Rust 或生成整套 KAT Release。

## 2. 构建 wheel

```powershell
$buildPython = (Resolve-Path .venv-sdk-build/Scripts/python.exe).Path
$output = "target/sdk-wheel-$(Get-Date -Format yyyyMMdd-HHmmss)"
& $buildPython build/build_sdk_wheel.py --output $output
if ($LASTEXITCODE -ne 0) { throw "SDK 构建失败，请检查上方日志" }
```

Linux 使用满足版本要求的 Python 执行 `python build/build_sdk_wheel.py --output target/sdk-wheel`。`--output` 必须是尚不存在、且不在 `kat/sdk/` 内的目录。版本来自 `kat/sdk/pyproject.toml`；可加 `--expected-version 0.1.1` 只校验期望值。

构建脚本复制源码到临时目录、静态校验 Provider/Workflow declaration 引用的 Runtime Guide 与本地链接、构建 wheel 并输出 SHA256。它不生成、修改、复制或安装 API 文档，也不上传产物。

## 3. 检查产物

以 SDK 0.1.1 为例：

```text
target/sdk-wheel-<时间>/
├─ kat_sdk-0.1.1-py3-none-any.whl
└─ kat_sdk-0.1.1-py3-none-any.whl.sha256
```

校验摘要并列出 wheel 内容：

```powershell
$wheels = @(Get-ChildItem -LiteralPath $output -Filter *.whl)
if ($wheels.Count -ne 1) { throw "预期只有一个 SDK wheel" }
$wheel = $wheels[0]
$expected = ((Get-Content -LiteralPath ($wheel.FullName + ".sha256") -Raw).Trim() -split "\s+")[0]
$actual = (Get-FileHash -LiteralPath $wheel.FullName -Algorithm SHA256).Hash
if ($actual -ne $expected) { throw "SDK SHA256 不匹配" }
& $buildPython -m zipfile -l $wheel.FullName
if ($LASTEXITCODE -ne 0) { throw "读取 wheel 失败" }
```

wheel 的主要内容为：

```text
kat_sdk/
├─ __init__.py
├─ pack.toml
├─ providers/
├─ workflows/<领域>/*.py
├─ helpers/<领域>/*.py
└─ knowledge/
   ├─ providers/
   └─ workflows/<领域>/
kat_sdk-<版本>.dist-info/
```

确认新增代码、PACK manifest 和 declaration 指向的具体 Guide 已进入 wheel。以下 API 文档必须全部缺席：

- `kat_sdk/docs/**`
- `kat_sdk/api.md`
- `kat_sdk/reference/**`
- `kat_sdk/knowledge/**/*.api.md`

源码侧 `kat/sdk/docs/api.md` 与 `docs/reference/` 由 `kat-dev-sdk` 生成并接受人工审核；它们和用户复制到 `kat-author` 的快照都不属于 wheel 构建输入。

## 4. 常见构建问题

| 现象 | 处理 |
| --- | --- |
| 输出目录已存在 | 指定新的 `--output` 目录 |
| 找不到 `build` 模块 | 用同一个 `$buildPython` 安装 `build==1.6.1` |
| Guide 或本地链接校验失败 | 修正 declaration 路径或具体 Guide 内链接后重建 |
| 新公共模块未进入 wheel | 检查包 `__init__.py`、`tool.setuptools.packages` 和 package-data |
| wheel 含 API 文档 | 修正打包配置；不要移动或复制源码 `docs/` 到 wheel |

## 5. 在真实 KAT 环境安装和验收

优先使用隔离测试部署。保持 CLI、框架和原生依赖不变，在相邻 Python 中安装候选 wheel：

```text
<bundled-python> -m pip install --no-deps --upgrade <SDK-wheel绝对路径>
<bundled-python> -m pip show kat-sdk
<bundled-python> -m pip check
```

按新增能力执行 inspection、真实调用和行为测试：

```text
kat inspect provider
kat inspect provider --provider <名称>
kat inspect workflow --pack kat-sdk
kat inspect workflow --pack kat-sdk --workflow <名称>
kat session create
kat run --session <id> --pack kat-sdk --workflow <名称> -- <业务参数>
kat test --pack-dir <已安装kat_sdk目录的绝对路径>
```

最后一条只在候选 PACK 配有测试时使用；正式 wheel 不携带 `tests/`。SDK 源码测试使用同一宿主运行 `-I -B -m pytest <仓库>/kat/sdk/tests -q`。公共 Python API 还须按本次变更执行隔离导入和行为验证。

完整安装及升级验收：

```text
python build/verify_sdk_install.py --kat <当前CLI> --workflow-wheel <框架wheel> --datasource-wheel <当前平台原生wheel> --sdk-wheel <SDKwheel> --output <新的验收目录>
```

验收覆盖未安装、安装、升级与卸载；检查公共发现、declaration 直接引用的 Runtime Guide、真实查询和执行、旧文件清理，以及卸载后框架与无关 PACK 的回归。外部工具能力还需记录实际工具和样本。Windows 与 Linux 使用同一个候选 wheel 分别验收。

交付说明 SDK 版本、wheel 路径、SHA256、已验证平台和行为、API 文档人工审核状态及未完成验证。正式发布使用已验收的同一 wheel 与摘要；当前流程不发布 PyPI，也不自动创建 Release。
