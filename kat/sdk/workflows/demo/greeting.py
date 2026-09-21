"""演示 Workflow 调用 SDK 公共库并返回结果表。"""

import kat
import pyarrow as pa
from kat.dataprovider import Table
from kat_sdk.helpers.demo.greeting import build_greeting


@kat.workflow(
    name="demo-greeting",
    description="调用 demo 公共库，生成一行问候结果。",
    parameters={"name": "称呼；首尾空白会被去除，不能全为空白。"},
    guide="workflows/demo/greeting.md",
)
def greeting(ctx: kat.Context, name: str = "KAT") -> Table:
    """生成问候结果表。

    Args:
        ctx: KAT 提供的执行上下文。
        name: 非空称呼，默认 KAT。

    Returns:
        一行结果表，message 列保存公共函数生成的问候语。

    Raises:
        ValueError: name 去除首尾空白后为空。
    """
    return Table.from_arrow(pa.table({"message": [build_greeting(name)]}))
