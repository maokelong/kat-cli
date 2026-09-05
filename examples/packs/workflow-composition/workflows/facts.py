import pyarrow as pa

from kat import Context, dataprovider as dp, workflow


@workflow(
    name="facts",
    description="生成两条示例事实，不代表真实测量。",
    parameters={"base": "第一条示例事实的值。"},
    guide="workflows/facts.md",
)
def facts(ctx: Context, base: int = 20):
    return {"facts": dp.Table.from_arrow(pa.table({"value": [base, base + 2]}))}
