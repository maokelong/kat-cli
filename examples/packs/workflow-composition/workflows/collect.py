from kat import Context, workflow


@workflow(
    name="collect",
    description="顺序执行两个工作步骤，仅发布子 Run 关系，不制造占位表。",
    guide="workflows/collect.md",
)
def collect(ctx: Context):
    ctx.run("workflow-composition", "facts", base=5)
    ctx.run("workflow-composition", "summarize", base=20)
    # 隐式 None：当前 Run 无 Output，成功子 Run 仍独立发布。
