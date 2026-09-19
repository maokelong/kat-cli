# KAT SDK 知识导航

本目录与当前安装的 SDK 实现一起发布。需要使用某个来源时，先阅读其 Guide，再按需查 API；公共 Python 函数从 libraries 下的主题文档查用法。

- 文本 Ftrace：[来源知识](providers/ftrace.md)、[API](providers/ftrace.api.md)。
- Trace Streamer SQLite：[来源知识](providers/trace-streamer-sqlite.md)、[API](providers/trace_streamer.api.md)。

公共 Workflow 通过 `kat inspect workflow --pack kat-sdk` 列出，并按名称读取参数与 Guide；执行沿用 `kat run` 或 `ctx.run("kat-sdk", workflow_name, **inputs)`。当前版本尚无正式公共 Workflow 或公共函数，后续能力随 SDK 更新进入对应目录。

SDK 在 KAT 使用的 Python 环境中运行，框架的 `kat` API、表类型及原生解码依赖继续由该环境提供。
