# SDK API 文档生成与交付

只在生成或更新 SDK Python API 文档时读取。源码定位、能力实现与普通构建按 [SDK 入口](../SKILL.md) 选择流程；框架作者文档位于相邻作者 Skill 的 [base-api.md](../../kat-author/references/base-api.md)，本流程只读。

## 1. 定义公共 Python API

`kat/sdk/__init__.py` 中私有、有序、字面量形式的 `_API_MODULES` 是 SDK Python API 的唯一模块清单：

```python
_API_MODULES = (
    "kat_sdk.providers.ftrace",
    "kat_sdk.providers.trace_streamer",
    "kat_sdk.helpers.demo.greeting",
)
```

生成顺序严格沿用该元组；每个 reference 的符号顺序严格沿用对应模块的字面量 `__all__`。未登记模块、未由 `__all__` 导出的符号和以下划线开头的成员均视为内部实现，不从可导入性、测试或旧文档反向推断为公共 API。

新增、移除或改变公共 API 行为时更新 SDK 版本。当前字面量公共常量值属于该版本的 API 合同。

## 2. 从源码生成 API 文档

`kat-dev-sdk` 直接读取当前源码 checkout，生成：

```text
kat/sdk/docs/
├─ api.md
└─ reference/
   └─ <每个 _API_MODULES 模块一份 Markdown>
```

生成期间禁止 import 或执行 SDK 模块，也不要求先构建或安装 wheel。按以下步骤执行：

1. 从 `pyproject.toml` 读取 SDK 版本，静态解析 `_API_MODULES`。模块缺失、重复登记或元组无法按字面量读取时停止。
2. 按登记顺序静态解析每个模块的字面量 `__all__`。导出名缺失、重复、以下划线开头或无法解析时停止。
3. 提取公开符号和公开成员的签名、参数种类与顺序、类型、默认值、返回类型、属性和 docstring。递归展开以下划线命名的私有类型别名；无法静态展开时停止，不能把私有类型名写入文档。
4. 结合实现、行为测试和真实消费者提炼一句话用途、稳定错误、重要边界与副作用。测试和旧文档只能为已由 `__all__` 确认的符号提供证据，不能扩大 API 面。
5. 示例只从已有测试或真实消费者收敛；没有可验证示例时省略。不要发明示例、行为或错误合同。
6. 任一导出符号缺少足够证据时列出该符号并停止正式生成；不得静默遗漏，也不得以“待补充”占位。

`api.md` 只写标题、SDK wheel 版本，以及按模块排列的公开导入路径、一句话用途和 reference 相对链接。每个 reference 写模块路径、一句话用途、该模块全部公开符号的签名、参数、返回值、公开错误、必要边界或副作用，以及可选的经验证示例。不要复制架构说明或 Runtime Guide 的来源关系、Schema 和领域解释。

错误只记录调用方可识别的公开异常类型与稳定触发条件；不固定异常消息，也不穷举 Python、文件系统或第三方库可能产生的全部低层错误。公开常量记录完整导入路径、当前字面值和用途。

## 3. 人工审核与作者侧交付

生成文件首先留在 `kat/sdk/docs/`。人工逐项核对版本、模块和符号顺序、签名、证据、链接以及内容是否精简准确；未经审核的结果不是正式 API 文档。

审核完成后，由用户自行复制：

```text
kat/sdk/docs/api.md          -> kat/skills/kat-author/references/api.md
kat/sdk/docs/reference/      -> kat/skills/kat-author/references/reference/
```

`kat-dev-sdk` 不写入 `kat-author`，wheel 构建和 Skill 装配也不自动同步。框架侧 `base-api.md` 由框架开发流程维护，本 Skill 始终只读。
