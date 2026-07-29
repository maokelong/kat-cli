---
status: accepted
---

# 配置文件与环境变量选择 KAT Data Home

## 决策

KAT Data Home 由 Skill 根目录的 `config.json` 和启动 KAT 的进程环境中的 `KAT_DATA_HOME` 选择。

发行包在 Skill 根目录提供默认配置：

```json
{"kat_data_home":""}
```

`config.json` 是可扩展 JSON 对象：未知字段允许保留。文件缺失、`kat_data_home` 缺失或其值为 `""` 都表示该来源没有选择 Data Home；`null`、数字、数组等非字符串值，以及不可读取或无法解析的已存在文件，均为配置错误。

选择顺序固定如下：

1. 非空的 `config.json.kat_data_home`；
2. 仅在第一项为空或缺失时，非空的 `KAT_DATA_HOME`；
3. 仅在前两项都未选择时，原有平台默认 Data Home。

配置文件和环境变量的非空值都必须直接给出一个可访问的绝对目录路径；KAT 不展开 `~`、`%USERPROFILE%`、`$HOME` 等缩写，并在使用前规范化路径。任一已选择的非空候选无效时，操作失败，不再尝试下一层或默认目录。空字符串环境变量与未设置相同。

Skill 升级仍由发行包覆盖旧 `config.json`，不保留用户原有配置。发行装配器必须在 Skill 根目录写入上述默认文件。

## 与既有决策的关系

本决策取代 ADR-0002 中“运行时不修改 Skill 且不提供配置覆盖”的相关部分；ADR-0002 的原子发布和其他载荷边界继续有效。
