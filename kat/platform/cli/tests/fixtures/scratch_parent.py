from kat import Context, RunError, workflow
from kat.pack.scratch_helpers import record, replace_with_file


@workflow(name="parent", description="Use the same gate for nested calls.",
          parameters={"mode": "Scenario", "evidence": "Test evidence directory"})
def parent(ctx: Context, mode: str, evidence: str):
    scratch = record(ctx, evidence, "parent")
    if mode == "catch":
        try:
            ctx.run("scratch", "child", mode="file", evidence=evidence)
        except RunError:
            return None
        raise AssertionError("replaced child Scratch was published")
    ctx.run("scratch", "child", mode="table" if mode == "parent_file" else mode,
            evidence=evidence)
    if mode == "parent_file":
        replace_with_file(scratch)
    return None
