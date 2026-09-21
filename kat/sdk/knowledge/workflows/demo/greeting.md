# Demo Workflow：调用公共库生成问候结果

`demo-greeting` 随 SDK 0.1.1 提供，演示“Workflow → 公共库 → 结果表”的最小链路。公共 PACK 名称为 `kat-sdk`，无需为每个领域复制一份。

## 发现与执行

按当前 KAT Skill 的公共命令合同定位 `kat`，准备 Data Home 后执行：

```text
kat inspect workflow --pack kat-sdk --workflow demo-greeting
kat session create
kat run --session <session_id> --pack kat-sdk --workflow demo-greeting -- --name "小明"
kat query --session <session_id> --run <run_id> --sql "SELECT message FROM output.main"
```

`session_id` 来自创建 Session 的成功 Response；`run_id` 来自执行的成功 Response。省略 `--name` 时使用 `KAT`。输出 `main` 表只有一行，`message` 为字符串；上述输入得到“你好，小明！”。查询 Response 给出结果文件路径，按该路径读取结果。

全空白称呼导致执行失败；CLI 按框架错误 Response 报告，不能将失败视为得到空表。

## 在其他 Workflow 中组合调用

```python
catalog = ctx.run("kat-sdk", "demo-greeting", name="小明")
```

返回框架只读 Catalog，可通过框架表工具读取 `main`。仅需要字符串时，直接导入 公共函数 `kat_sdk.helpers.demo.greeting.build_greeting`（使用方法与 API 说明见 kat Skill 的 `references/helpers/demo/greeting.md`），无需启动子 Workflow。

## 开发与验证

入口源码：`kat/sdk/workflows/demo/greeting.py`，领域目录中不放 `__init__.py`。入口调用公共函数，再通过 `Table.from_arrow` 返回标准结果表。

[Workflow API 参考](greeting.api.md) 由注解和 docstring 生成。验收应检查默认称呼、自定义 Unicode 称呼、全空白输入，以及从其他 PACK 的组合调用。该例不解析真实数据，也不需要新增 Provider。

返回 [SDK 知识首页](../../index.md)。
