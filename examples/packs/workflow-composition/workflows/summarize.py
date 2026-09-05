from kat import Context, dataprovider as dp, workflow


@workflow(
    name="summarize",
    description="调用 facts 并查询已发布 Catalog，形成独立的汇总 Output。",
    parameters={"base": "传递给 facts 的起始值。"},
    guide="workflows/summarize.md",
)
def summarize(ctx: Context, base: int = 20):
    facts = ctx.run("workflow-composition", "facts", base=base)
    return dp.DataFusionProvider(catalog=facts).query(
        "SELECT COUNT(*) AS samples, SUM(value) AS total FROM facts"
    )
