# SDK 检查、下载、安装与升级

SDK 是包含作者 API、Workflow Runtime、原生 Datasource 与 Markdown 的一个 kat-sdk 安装包。CLI 独立，完整 Skill 已内置 SDK。仅检查或下载时不安装。

## 查询和下载

从 [官方 Releases](https://github.com/maokelong/kat-cli/releases) 或 [Releases API](https://api.github.com/repos/maokelong/kat-cli/releases?per_page=100) 处理分页，筛选非草稿 sdk/ 标签；不能使用仓库通用 latest。指定版本精确匹配；“最新”按 PEP 440 比较，默认稳定版，用户选择预发布渠道才包含 rc/dev。

根据目标环境选择 CPython 3.14、Windows x86_64 或 manylinux 2.28 x86_64 wheel 及对应 .sha256。核对 SHA、METADATA、ABI/平台标签，确认 wheel 仅包含实际代码目录 kat/（_declarations、dataprovider、providers、_runtime、knowledge）和唯一 kat_sdk-<版本>.dist-info；没有 CLI、解释器或依赖旧 kat-workflow/kat-datasource。py3-none-any 是旧 API-only 候选或中间输入，不是当前完整 SDK。

尚无公开版本时如实说明；明确选定 CI 候选后，从 [SDK workflow](https://github.com/maokelong/kat-cli/actions/workflows/sdk-ci.yml) 成功运行的 kat-sdk-<平台> artifact 取最终 wheel，并核对源码 SHA、摘要和有效期。当前未发布 PyPI，不用裸 pip install -U kat-sdk 替代资产选择。

## 沿用原环境

完整 Skill 的 Python 位于 <kat-root>/scripts/targets/windows-x86_64/python/python.exe 或 <kat-root>/scripts/targets/linux-x86_64/python/bin/python3。运行 KAT 命令时使用原 Skill Python；仅直接使用 SDK Python API 时可选择用户指定的兼容环境。多个环境有歧义时询问缺项，后续升级保留同一解释器。下载文件放在环境和用户数据之外。

记录 sys.executable、sys.prefix、Python 版本及已有 KAT distributions。在停止使用该环境的任务后更新；同版本直接验收，降级仅按明确要求执行。

~~~text
<python> -m pip install --upgrade --only-binary=:all: <最终SDK-wheel绝对路径>
<python> -m pip check
~~~

自带精简 Python 没有 pip 时，使用 <工具python> -m pip --python <目标python绝对路径> install --upgrade --only-binary=:all: <wheel>，依赖检查使用相同前缀的 check。目标始终是原 Skill Python，保持 Skill 目录与解释器不变。

从旧拆分方案（含 kat-workflow 或 kat-datasource）迁移时，先下载并校验新 SDK 和包含配套原生 CLI 的完整 Skill，再在原环境卸载旧 kat-sdk、kat-workflow、kat-datasource，然后安装统一 SDK。不能先覆盖再卸载原所有者，否则旧 RECORD 会删除新文件。SDK 合并后不再安装旧组件 wheels。rc14 及之前的 CLI 仍启动旧 Runtime 模块，首次迁移必须同步更新 CLI，用户 PACK 继续使用 import kat、@kat.workflow 和 kat.dataprovider；之前试用 kat_sdk 的 PACK 改回 kat；其中具体 Provider 从 kat.providers.ftrace 或 kat.providers.trace_streamer 导入；kat_datasource 改为 kat.providers.decoding。完整 Skill 的原生 CLI 也需同步更新；后续兼容 SDK 更新无需替换 CLI。检查 pip check，不用 --no-deps 绕过约束。

失败时报告实际安装状态与阶段，不假设 pip 自动回滚，不删除用户 PACK/Data Home。

## 验收与知识

~~~text
<python> -I -X utf8 -c "from importlib.metadata import version; from kat import knowledge; import kat._runtime, kat.providers.decoding; print(version('kat-sdk')); print(knowledge.read('index'))"
~~~

确认旧 kat-workflow、kat-datasource distribution 已消失，SDK 版本正确且 pip check 通过。site-packages 中应有实际 kat/ 代码目录及 kat_sdk-<版本>.dist-info 安装记录，旧 kat_sdk/、_kat_runtime/、kat_datasource/ 均不存在，不提供旧导入别名。

AI 先读取 index，再按任务读取 knowledge.read("authoring")（完整创作、声明、Context 与数据框架知识），以及具体 Provider 主题，以安装版本为准。Context 执行能力仍由 Runtime 注入。完整 Skill 更新后按 [公共命令合同](command-reference.md) 验证 Inspect/provider；SDK 更新不改 Skill 文件或用户数据。
