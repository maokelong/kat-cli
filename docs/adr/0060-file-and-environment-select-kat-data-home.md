---
status: accepted
---

# 配置文件与环境变量选择 KAT Data Home

## 决策

KAT 不提供 `kat config`、`config set` 或 `config get` 命令。用户直接编辑 Skill 根目录的 `config.json`，或在启动 KAT 的进程环境中设置 `KAT_DATA_HOME`。

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

该选择只决定 KAT 管理的默认 Dataset、External PACK、Run、Operation log 与 PACK Test Report；显式 `--dataset` 与 `--pack-dir` 保持用户提供的路径。只有实际消费 KAT 管理目录的操作才读取并校验配置；不消费该目录的显式路径操作（例如 `kat inspect --dataset`）不要求 Skill 或配置文件。帮助和参数解析错误同样不触发该读取。

Skill 升级仍由发行包覆盖旧 `config.json`，不保留用户原有配置。发行装配器必须在 Skill 根目录写入上述默认文件。

## 取舍

命令式 `set/get` 能统一路径写入并展示当前结果，但把本地设置变成额外 CLI 产品面，也无法满足脚本和部署环境以环境变量临时覆盖的需求。直接配置文件保留了人可见、可编辑的安装设置；环境变量提供不改文件的进程级候选。固定的文件优先级使安装设置不会被环境意外覆盖。

本决策取代 ADR-0002 中“运行时不修改 Skill 且不提供配置覆盖”的相关部分；ADR-0002 的原子发布和其他载荷边界继续有效。

## 不做

第一版不增加 `--data-home`、Profile、通用配置命令、路径缩写展开、数据迁移，或旧 Data Home 的移动、合并和回退读取。
