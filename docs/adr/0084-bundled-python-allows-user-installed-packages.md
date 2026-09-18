---
status: accepted
---

# Bundled Python Host 允许用户自行安装第三方库

PACK 作者需要使用 KAT 发布时未携带的第三方库。用户可以通过 Bundled Python Host 的 pip 向当前部署共享的 Python 环境安装库，并允许 pip 按原生规则更新已经携带的第三方依赖，包括 PyArrow 与 DataFusion；KAT 不增加包名白名单或内置第三方依赖版本保护，用户负责其修改后的共享环境及依赖冲突。此决定使用成熟包管理工具满足扩展需求，接受同一部署中的 Runtime 和各 PACK 会共同受到依赖变更影响这一取舍。

本决定局部替代 ADR-0002 中用户机器不下载或解析依赖的约束，以及 ADR-0004 中内置 Python 仅供 CLI 使用、部署内容始终只读和第三方依赖始终等同发布基线的约束。发布构建仍交付锁定并验证的初始环境；用户主动安装属于受支持操作，正常分析执行仍不自行修改部署。KAT 执行继续使用当前部署的 Python 和 isolated mode，安装到该解释器 site-packages 的库可供 PACK 使用。

允许安装不承诺每个第三方库都支持当前 CPython、操作系统或本机构建条件，也不把修改后的依赖集合当成 KAT 已验证的发布基线。安装与后续验证沿用 pip 和 KAT 的真实结果，不以包名或来源代替验证。

整套 KAT Skills 通过目录替换升级时，使用新版本随包发布的干净 Python 环境，不自动保留或迁移用户后来安装的库；用户按需重新安装额外依赖。这样保持整套交付边界清晰，并避免旧环境依赖混入新版本。这一决定仅适用于整套部署替换，[Issue #284](https://github.com/maokelong/kat-cli/issues/284) 的 SDK 拆分与原环境组件升级仍由其自身方案负责。

在用户已授权的 PACK、Provider 或 Workflow 创作任务中，`kat-author` 可以使用内置 Python 的 `-m pip install` 安装完成该任务所需的缺失依赖，不要求用户逐包再次确认；交付时说明安装的包、实际版本和验证结果。只读理解、检查或诊断本身不授予安装权限。常规分析流程遇到缺库时报告缺项，不自动安装；用户明确要求安装时按其授权执行。

最小实现范围和验证方案见 [Issue #288](https://github.com/maokelong/kat-cli/issues/288)。本决定不表示 rc.11 已提供 pip 安装能力。
