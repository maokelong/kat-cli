# Demo Workflow：生成问候结果

适用于 SDK 0.1.1 的 `kat-sdk/demo-greeting`。用于验证“Workflow → 公共库 → 结果表”的最小链路，无需数据文件或外部解析器。

| 项目 | 用法 |
| --- | --- |
| 参数 | `name: str`，默认 `KAT`；去除首尾空白后须非空 |
| 输出 | `main` 表，一行字符串列 `message` |
| 示例结果 | 输入“小明”，输出“你好，小明！” |
| 失败输入 | 空串或全空白称呼导致执行失败 |

先按 [命令合同](../../command-reference.md) 定位当前 CLI 和 Data Home：

```text
kat inspect workflow --pack kat-sdk --workflow demo-greeting
kat session create
kat run --session <session_id> --pack kat-sdk --workflow demo-greeting -- --name "小明"
kat query --session <session_id> --run <run_id> --sql "SELECT message FROM output.main"
```

Session 和 Run ID 分别取自对应成功 Response；查询结果读取 query Response 的文件路径。inspection 返回当前 SDK 的参数和 Guide，执行前以其核对本介绍。

在其他 Workflow 中用 `ctx.run("kat-sdk", "demo-greeting", name="小明")` 组合调用，返回只读 Catalog。只需要字符串时使用 [build_greeting 公共函数](../../helpers/demo/greeting.md)。

返回 [Workflow 导航](../index.md)。
