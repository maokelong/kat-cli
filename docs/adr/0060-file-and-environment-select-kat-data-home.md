---
status: accepted
---

# 配置文件与环境变量选择 KAT Data Home

## 决策

KAT Data Home 由启动 KAT 的进程环境中的 `KAT_DATA_HOME` 和可选的 `config.json` 选择。配置文件固定为 `directories::ProjectDirs::from("", "", "KAT")` 的 `data_dir()/config.json`，即使环境变量选中了其他 Data Home，其位置也不改变。

`config.json` 是 KAT 私有应用配置，不是 Agent Skill 约定或 `SKILL.md` 扩展。它位于平台默认 KAT 数据目录，使同一用户的 KAT 配置与安装目录、发行更新无关；环境变量则提供单次进程的更高优先级选择。

评估过 Figment 的 [`Json::file_exact()`](https://docs.rs/figment/0.10.19/figment/providers/struct.Data.html#method.file_exact)、[`Env`](https://docs.rs/figment/0.10.19/figment/providers/struct.Env.html) 和后序 [`merge()`](https://docs.rs/figment/0.10.19/figment/struct.Figment.html#method.merge)：它们可复用固定 JSON 文件、环境变量过滤和“后者覆盖前者”的通用能力。但本契约是短路选择，不是同时加载的合并：有效的 `KAT_DATA_HOME` 必须在读取低优先级文件之前被选中，因此低优先级的损坏 JSON 不能使该进程失败；空环境变量也必须等同缺失，而不是一个参与反序列化的空值。若为这两个语义在 Provider 外再预读环境变量、条件性装配 Provider，配置框架只会增加依赖和间接层，不能删除领域代码。故本切片继续复用标准 JSON 与 `serde_json` 处理文件格式，只保留来源短路、路径校验和 KAT 诊断这三项领域胶水；未来出现多个独立字段或不要求短路错误语义时再重新评估。

`config.json` 是可扩展 JSON 对象：未知字段允许保留。文件缺失、`kat_data_home` 缺失或其值为 `""` 都表示该来源没有选择 Data Home；`null`、数字、数组等非字符串值，以及不可读取或无法解析的已存在文件，均为配置错误。

选择顺序固定如下：

1. 非空的 `KAT_DATA_HOME`；
2. 仅在第一项为空或缺失时，非空的 `config.json.kat_data_home`；
3. 仅在前两项都未选择时，原有平台默认 Data Home。

配置文件和环境变量的非空值都必须直接给出一个可访问的绝对目录路径；KAT 不展开 `~`、`%USERPROFILE%`、`$HOME` 等缩写，并在使用前规范化路径。这样避免相对工作目录和调用者 shell 展开使同一选择落到不同位置，也让后续持久化产物只面对一个已解析目录。覆盖来源不按需创建：自动创建会把拼写错误或失效挂载静默变成新的 Data Home。任一已选择的非空候选无效时，操作失败，不再尝试下一层或默认目录。平台默认目录仍不作预检；它维持既有默认行为，具体产物在首次写入时按其原有规则创建所需目录。空字符串环境变量与未设置相同。

`config.json` 由用户按需在平台默认 KAT 数据目录提供和维护；KAT CLI 与发行制品都不创建或写入它。文件缺失时的回退规则使其不是运行前提。

## 与既有决策的关系

本决策只取代 ADR-0002 中“Data Home 只能使用平台默认目录且不提供覆盖来源”的部分；运行时不修改 Skill、Payload 或 PACK 的边界，以及 ADR-0002 的原子发布和其他载荷边界继续有效。
