# Demo 公共库：问候语

本例演示按领域组织的普通 Python 函数，随 SDK 0.1.1 提供。它没有文件、网络或外部解析器依赖，适合先验证 SDK 的安装和导入。

## 使用

在已安装 SDK 的 KAT Python 环境中：

```python
from kat_sdk.helpers.demo.greeting import build_greeting

assert build_greeting(" KAT ") == "你好，KAT！"
assert build_greeting("小明") == "你好，小明！"
```

函数去除姓名首尾空白，保留内部字符。空串或全空白字符串抛出 `ValueError`；非字符串抛出 `TypeError`。它直接返回字符串，不注册为 Provider 或 Workflow。

## 源码与 API

源码路径：`kat/sdk/helpers/demo/greeting.py`。本页同时提供函数 API 说明：

```python
def build_greeting(name: str) -> str: ...
```

| 项目 | 合同 |
| --- | --- |
| `name` | 非空字符串，去除首尾空白后仍须有内容 |
| 返回值 | `str`，格式为 `你好，{去除首尾空白后的姓名}！` |
| `TypeError` | 输入不是字符串 |
| `ValueError` | 输入为空串或全空白 |

文档适用于 SDK 0.1.1 的上述接口。SDK 可独立升级；按 [Python 依赖管理](../../python-packages.md) 核对安装版本，必要时读取已安装函数的签名与 docstring：

```text
<bundled-python> -I -B -c "import inspect; from kat_sdk.helpers.demo.greeting import build_greeting; print(inspect.signature(build_greeting)); print(inspect.getdoc(build_greeting))"
```

SDK 的 `demo-greeting` Workflow（通过 `kat inspect workflow --pack kat-sdk --workflow demo-greeting` 读取 Guide） 调用此函数并将结果包装为框架 Table。修改函数时同步维护行为测试、docstring 和本 Guide；通过 kat 公共库导航发现函数，不增加 CLI 函数发现命令。

返回 [公共库导航](../index.md)。
