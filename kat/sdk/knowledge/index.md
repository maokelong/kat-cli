# KAT SDK 知识导航

本目录与当前安装的 SDK 实现一起发布。需要使用某个来源时，先阅读其 Guide，再按需查 API；公共 Python 函数的使用介绍从 kat Skill 的 references/libraries 查阅，本目录提供当前 SDK 版本的生成 API 参考。

- Demo 公共库：[当前版本 API](libraries/demo/greeting.api.md)。使用介绍位于 KAT Skill 的 `references/libraries/demo/greeting.md`。
- Demo Workflow：[使用说明](workflows/demo/greeting.md)、[API](workflows/demo/greeting.api.md)。
- 文本 Ftrace：[来源知识](providers/ftrace.md)、[API](providers/ftrace.api.md)。
- Trace Streamer SQLite：[来源知识](providers/trace-streamer-sqlite.md)、[API](providers/trace_streamer.api.md)。

公共 Workflow 通过 `kat inspect workflow --pack kat-sdk` 列出，并按名称读取参数与 Guide；执行沿用 `kat run` 或 `ctx.run("kat-sdk", workflow_name, **inputs)`。SDK 0.1.1 提供 demo 领域示例，后续能力随 SDK 更新进入对应领域目录。

SDK 在 KAT 使用的 Python 环境中运行，框架的 `kat` API、表类型及原生解码依赖继续由该环境提供。
