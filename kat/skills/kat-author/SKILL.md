---
name: kat-author
description: 使用 KAT 理解、创建、修改、验证和诊断 PACK、Provider、Workflow 与 PACK 内领域公共库。适用于 PACK 创作维护、来源能力接入、领域函数库开发、固定 Workflow 组合与 PACK 测试。
---

# KAT Author

按已授权的开发目标理解或维护 PACK，使用公开知识与实际 inspection/test 结果交付。

## 定位共享部署

从本 `SKILL.md` 的绝对位置解析相邻 `../kat/`，确认其中的 `SKILL.md`，读取 [公共命令合同](../kat/references/command-reference.md)。该 `kat/SKILL.md` 的父目录才是公共合同中的 `<kat-root>`；不要把当前任务目录或工作目录当作载荷根。

每次 KAT 调用按公共合同选择 CLI 的绝对路径，复用其 Data Home、Response 与失败边界。四个 Skill 必须来自同一套部署；缺少共享目录或载荷时说明部署缺项，不从其他版本或 `PATH` 拼装。直接使用本 Skill 无需先调用总路由。

本目录为 `<author-root>`，拥有 [PACK 骨架脚本](scripts/scaffold_pack.py) 和 [作者示例 PACK](references/examples/dataprovider-pack/README.md)。从本文件解析这些资源的绝对路径，不依赖当前工作目录。

已授权的 PACK、Provider 或 Workflow 创作中，可补装任务所需的缺失依赖，无需逐包再次询问；需要时读取共享 [Python 依赖管理](../kat/references/python-packages.md)。只读理解、检查、测试或诊断本身不授权安装；用户明确要求安装时按其授权执行。

## 创作与交付

1. 读取 [PACK 创作与维护流程](references/pack-authoring-flow.md)。新增 Workflow、Provider 或公共函数，或者修改其行为、输入、输出、依赖或数据处理步骤前，必须执行 [开发前复用检查](references/reuse-check.md)；只改文案、格式，或只改不影响能力的测试时可以跳过。新增或提取 PACK 内函数库时读取 [领域公共库开发](references/pack-helpers.md)，完成实现、文档及测试。
2. 理解、检查、测试和诊断不自动授权修改 PACK；只有用户明确要求创建、修改或修复时才写入指定目标。创建骨架时使用自带脚本并保留其拒绝覆盖行为。
3. 脚本承载确定性执行，Workflow Guide 解释关键输出与推理依据；核对文件归属、Guide 与装饰器的关联及实际 detail 回读内容，再核准输出口径并完成适用的 PACK 测试。
4. 交付前读取 [作者结果契约](references/result-contract.md)，说明实际变更、验证证据和仍存限制。
