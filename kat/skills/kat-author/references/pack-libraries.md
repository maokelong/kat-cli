# PACK 内领域公共库开发

用户要在自己的 PACK 中新增函数库、提取多个 Workflow 或 Provider 共用的函数时读取本指南。这里的“公共”指当前 PACK 内复用；实现随 PACK 交付。跨领域官方能力由 [kat-dev-sdk](../../kat-dev-sdk/SKILL.md) 维护。

## 1. 先检查可复用能力

执行 [SDK 公共库复用检查](../../kat/references/libraries/index.md#新开发前的复用检查)，同时检查目标 PACK 已有的 `helpers/`、文档和实际调用点。对比用途、输入输出、语义及限制；满足需求时直接复用，有缺口时说明检查范围与缺口，只实现已授权的必要部分。检查失败时停止该函数开发。

## 2. 在当前 PACK 内组织实现

沿用现有 `helpers/`，按功能分模块；模块较多时按子领域组织。只创建实际需要的文件，例如：

```text
<pack>/
├─ pack.toml
├─ README.md
├─ helpers/
│  └─ memory/
│     └─ units.py
├─ workflows/
│  └─ memory_size.py
├─ knowledge/
│  └─ libraries/
│     ├─ index.md
│     └─ memory/units.md
└─ tests/
   └─ test_memory_units.py
```

领域库采用普通 Python 函数，写明类型注解、单位、异常与边界。纯计算尽量与文件读取、数据库连接和 Context 分开，通过参数传入必要值，模块导入时不执行业务操作。示例 `helpers/memory/units.py`：

```python
__all__ = ["bytes_to_mib"]


def bytes_to_mib(value: int) -> float:
    """将非负字节数换算为 MiB（1 MiB = 1024 ** 2 字节）。"""
    if type(value) is not int:
        raise TypeError("value must be an integer byte count")
    if value < 0:
        raise ValueError("value must be non-negative")
    return value / (1024 ** 2)
```

这是组织与测试示例，实际项目仍须先检查是否已有等价函数。辅助目录可按现有 PACK 命名空间作为无初始化文件的包使用，无需为了上述布局新增 `__init__.py`。

## 3. 在 Workflow 或 Provider 中导入

在当前 PACK 的执行环境中：

```python
from kat.pack.helpers.memory.units import bytes_to_mib
```

`kat.pack` 指向当前执行的 PACK；不同 PACK 不通过此路径互相导入辅助库，不手工修改 `sys.path`。普通系统 Python 未绑定 PACK 环境时不能直接使用该导入，运行和测试遵循 KAT 命令合同。

Workflow 调用示例：

```python
import kat
import pyarrow as pa
from kat import dataprovider as dp
from kat.pack.helpers.memory.units import bytes_to_mib


@kat.workflow(
    name="memory-size",
    description="将字节数换算为 MiB。",
    parameters={"size_bytes": "非负字节数"},
)
def memory_size(ctx: kat.Context, *, size_bytes: int):
    return dp.Table.from_arrow(pa.table({"size_mib": [bytes_to_mib(size_bytes)]}))
```

普通函数不加 Workflow/Provider 装饰器，不增加函数发现命令。返回的标量由调用方使用；只有 Workflow 返回的框架 Table 才按框架合同发布为 Run Output。

## 4. 文档随领域 PACK 交付

在 PACK 自己的 `knowledge/libraries/<子领域>/<模块>.md` 说明导入路径、签名、参数、返回值、单位、异常、示例及复用边界，从 `knowledge/libraries/index.md` 导航，并在 PACK 的 README 链接该导航。简单库可省略子领域层。

这些文档属于当前 PACK，不写入 kat Skill 的官方 SDK 公共库目录，也不写入 SDK。AI 从 PACK README 和库导航读取；`kat inspect` 不发现普通函数或返回其文档。Workflow/Provider 自身 Guide 仍按 [PACK 创作流程](pack-authoring-flow.md) 关联，不用库文档替代其 Guide。

## 5. 验证函数和调用链路

在 `tests/test_memory_units.py` 中分别验证函数行为与真实 Workflow 调用：

```python
import pytest
from kat.pack.helpers.memory.units import bytes_to_mib


def test_bytes_to_mib():
    assert bytes_to_mib(0) == 0.0
    assert bytes_to_mib(1572864) == 1.5
    with pytest.raises(ValueError):
        bytes_to_mib(-1)
    with pytest.raises(TypeError):
        bytes_to_mib(True)


def test_workflow_uses_library(kat_run):
    result = kat_run(workflow="memory-size", arguments=["--size-bytes", "1572864"])
    assert result["main"].to_pydict() == {"size_mib": [1.5]}
```

按 [命令合同](../../kat/references/command-reference.md) 定位 CLI，执行 `kat test --pack-dir <PACK绝对路径>`。新增或修改 Workflow/Provider 声明时另做相应 inspection。检查 README、库导航与模块文档的相对链接有效；交付实现路径、实际导入方式、复用选择或能力缺口、文档位置及测试结果。
