# `kat_sdk.helpers.demo.greeting`

生成经过首尾空白规范化的中文问候语。

## `build_greeting`

```python
def build_greeting(name: str) -> str
```

`name` 必须是字符串，去除首尾空白后仍须非空。返回 `你好，{规范化后的姓名}！`，并保留姓名内部字符。非字符串输入抛出 `TypeError`；空串或全空白输入抛出 `ValueError`。

经行为测试验证的用法：

```python
from kat_sdk.helpers.demo.greeting import build_greeting

assert build_greeting(" KAT ") == "你好，KAT！"
```
