# Demo 公共库：问候语

本例演示按领域组织的普通 Python 函数，随 SDK 0.1.1 提供。它没有文件、网络或外部解析器依赖，适合先验证 SDK 的安装和导入。

## 使用

在已安装 SDK 的 KAT Python 环境中：

```python
from kat_sdk.libraries.demo.greeting import build_greeting

assert build_greeting(" KAT ") == "你好，KAT！"
assert build_greeting("小明") == "你好，小明！"
```

函数去除姓名首尾空白，保留内部字符。空串或全空白字符串抛出 `ValueError`；非字符串抛出 `TypeError`。它直接返回字符串，不注册为 Provider 或 Workflow。

## 源码与 API

源码路径：`kat/sdk/libraries/demo/greeting.py`。完整类型、异常与示例见构建生成的 [API 参考](greeting.api.md)。

[Demo Workflow](../../workflows/demo/greeting.md) 调用此函数并将结果包装为框架 Table。修改函数时同步维护行为测试、docstring 和本 Guide；通过知识首页发现函数，不增加 CLI 函数发现命令。

返回 [SDK 知识首页](../../index.md)。
