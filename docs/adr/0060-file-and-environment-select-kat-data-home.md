---
status: accepted
---

# 配置文件与环境变量选择 KAT Data Home

## 决策

KAT Data Home 由启动 KAT 的进程环境中的 `KAT_DATA_HOME` 和可选的 `config.json` 选择。配置文件固定为 `directories::ProjectDirs::from("", "", "KAT")` 的 `data_dir()/config.json`，即使环境变量选中了其他 Data Home，其位置也不改变。

`config.json` 是 KAT 私有应用配置，不是 Agent Skill 约定或 `SKILL.md` 扩展。它位于平台默认 KAT 数据目录，使同一用户的 KAT 配置与安装目录、发行更新无关；环境变量则提供单次进程的更高优先级选择。当前只有一个配置字段，因此直接使用标准 JSON 与 `serde_json`，不引入配置框架。

`config.json` 是可扩展 JSON 对象：未知字段允许保留。文件缺失、`kat_data_home` 缺失或其值为 `""` 都表示该来源没有选择 Data Home；`null`、数字、数组等非字符串值，以及不可读取或无法解析的已存在文件，均为配置错误。

选择顺序固定如下：

1. 非空的 `KAT_DATA_HOME`；
2. 仅在第一项为空或缺失时，非空的 `config.json.kat_data_home`；
3. 仅在前两项都未选择时，原有平台默认 Data Home。

配置文件和环境变量的非空值都必须直接给出一个可访问的绝对目录路径；KAT 不展开 `~`、`%USERPROFILE%`、`$HOME` 等缩写，并在使用前规范化路径。任一已选择的非空候选无效时，操作失败，不再尝试下一层或默认目录。空字符串环境变量与未设置相同。

`config.json` 由用户按需在平台默认 KAT 数据目录提供和维护；KAT CLI 与发行制品都不创建或写入它。文件缺失时的回退规则使其不是运行前提。

## 与既有决策的关系

本决策取代 ADR-0002 中“运行时不修改 Skill 且不提供配置覆盖”的相关部分；ADR-0002 的原子发布和其他载荷边界继续有效。
