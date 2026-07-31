---
status: accepted
---

# 合并配置文件与环境变量选择 KAT Data Home

## 决策

KAT Data Home 由启动 KAT 的进程环境中的 `KAT_DATA_HOME` 和可选的 `config.json` 合并得到。配置文件固定为 `directories::ProjectDirs::from("", "", "KAT")` 的 `data_dir()/config.json`，即使环境变量选中了其他 Data Home，其位置也不改变。

`config.json` 是 KAT 私有应用配置，不是 Agent Skill 约定或 `SKILL.md` 扩展。它位于平台默认 KAT 数据目录，使同一用户的 KAT 配置与安装目录、发行更新无关；环境变量则提供单次进程的更高优先级选择。

配置加载使用 `config` crate，并只启用 JSON feature。它承接 JSON Provider、分层覆盖和反序列化，项目代码只保留固定配置位置、精确环境变量名、空值规则、路径校验和 KAT 诊断。没有直接使用其 Environment Provider：KAT 只有一个明确的环境变量，扫描整个进程环境会把无关变量带入配置并扩大失败面。

所有已提供来源都必须有效，再参与合并。已存在的 `config.json` 必须可读取、UTF-8 与 JSON 语法有效且字段类型有效；即使环境变量最终覆盖其中的值，文件错误也会使操作失败。`config::File` 会宽松替换无效 UTF-8，项目因此先用标准库严格解码，再把字符串交给 JSON Provider；`config` 对标量反序列化也允许宽松转换，因此文件层在合并前还会检查原始 `ValueKind`，确保 `kat_data_home` 确实是 JSON 字符串。`KAT_DATA_HOME` 必须是有效 Unicode；空字符串等同未设置。`config.json` 是可扩展 JSON 对象：未知字段允许保留。文件缺失、`kat_data_home` 缺失或其值为 `""` 都表示该来源没有提供值；`null`、数字、数组等非字符串值均为配置错误。

选择顺序固定如下：

1. 非空的 `KAT_DATA_HOME` 覆盖配置文件；
2. 非空的 `config.json.kat_data_home` 覆盖默认值；
3. 前两项都未提供值时，使用原有平台默认 Data Home。

合并后胜出的非空值必须直接给出一个可访问的绝对目录路径；被更高优先级覆盖的路径字符串不访问文件系统。KAT 不展开 `~`、`%USERPROFILE%`、`$HOME` 等缩写，并在使用前规范化最终路径。这样避免相对工作目录和调用者 shell 展开使同一选择落到不同位置，也让后续持久化产物只面对一个已解析目录。覆盖来源不按需创建：自动创建会把拼写错误或失效挂载静默变成新的 Data Home。最终候选无效时，操作失败，不再回退。平台默认目录仍不作预检；它维持既有默认行为，具体产物在首次写入时按其原有规则创建所需目录。

`config.json` 由用户按需在平台默认 KAT 数据目录提供和维护；KAT CLI 与发行制品都不创建或写入它。文件缺失时的回退规则使其不是运行前提。

## 与既有决策的关系

本决策只取代 ADR-0002 中“Data Home 只能使用平台默认目录且不提供覆盖来源”的部分；运行时不修改 Skill、Payload 或 PACK 的边界，以及 ADR-0002 的原子发布和其他载荷边界继续有效。
