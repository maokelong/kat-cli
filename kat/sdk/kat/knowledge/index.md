# KAT SDK 知识导航

使用 `from kat import knowledge` 读取与安装版本一致的知识。创作正文保留为一份完整的 [PACK 创作与维护流程](authoring/pack-authoring-flow.md)，通过 `knowledge.read("authoring")` 读取；以下按章节区分知识用途，不另建重复说明。

## 框架知识

编写或修改通用接口用法时，在完整创作流程中阅读对应章节。

| 内容 | 章节 |
| --- | --- |
| Workflow、Provider 声明及知识关联 | 第 3 节 |
| 输入、输出、Context 与数据合同 | 第 4 节 |
| 嵌套执行、Catalog 与 RunError | 第 5 节 |
| Guide 的组织与引用规则 | 第 6 节 |
| Provider inspection 执行边界 | 第 7 节 |
| Table、Schema、写入与数据融合 API | 第 8 节的数据框架部分 |

框架接口签名可用 Python `help()` 查看。

## 框架应用知识

创建 PACK、接入来源或完成交付时，阅读完整创作流程的第 1—2、8—9 节。第 8 节同时包含原生 Hitrace 解码示例与通用数据框架用法，按当前任务选择。

具体来源合同由以下独立指南维护，复用 Provider 时按需阅读：

| 主题 | 内容 | 文档 |
| --- | --- | --- |
| ftrace | FtraceProvider 的接入、物化与查询 | [Ftrace](providers/ftrace/guide.md) |
| trace_streamer | TraceStreamerProvider 的解析与 SQLite 查询 | [Trace Streamer](providers/trace_streamer/guide.md) |

例如 `knowledge.read("ftrace")`。具体实现位于 `kat.providers`，通用数据框架位于 `kat.dataprovider`。

## AI 阅读顺序

先读取完整创作流程，按任务定位框架或应用章节；涉及具体来源时，通过 `kat inspect provider` 发现能力并读取目标详情。将 Provider 特有的 SQL、Schema、配置和物化约定限定在该来源内，通用 API 与约束以框架知识为准。

CLI、骨架脚本和作者示例由完整 Skill 提供；SDK 单独安装即可读取本文与创作知识。
