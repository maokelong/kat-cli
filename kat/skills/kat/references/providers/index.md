# SDK Provider 导航

需要选择数据来源适配器或在 Python/Workflow 中复用 Provider 时读取本目录。先按 [命令合同](../command-reference.md) 定位 CLI，执行 `kat inspect provider` 核对当前安装能力，再读取目标详情中的 Guide。

| Provider | 适用来源 |
| --- | --- |
| [ftrace-text](ftrace-text.md) | tracefs 文本，转换为可查询的类型化关系 |
| [trace-streamer-sqlite](trace-streamer-sqlite.md) | Trace Streamer 解码来源或已有 SQLite 数据库 |

这些介绍随 kat Skill 交付，标明适用 SDK 版本；当前表结构、数据语义和限制以安装版本的 Guide 与实际来源为准。Provider 由 Python 导入使用，不能用 `kat run` 直接执行；已有分析能力从 [Workflow 导航](../workflows/index.md) 选择。

新增或修改 SDK Provider 使用 [kat-dev-sdk](../../../kat-dev-sdk/SKILL.md)，同步维护本目录介绍与导航。
