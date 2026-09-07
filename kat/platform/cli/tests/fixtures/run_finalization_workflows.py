import pyarrow as pa
from kat import Context, dataprovider as dp, workflow
from kat.pack.run_finalization_helpers import record, replace_with_file


@workflow(name="child", description="Exercise Scratch finalization.",
          parameters={"mode": "Scenario", "evidence": "Test evidence directory"})
def child(ctx: Context, mode: str, evidence: str):
    scratch = record(ctx, evidence, "child")
    if mode in ("file", "business_file", "output_file"):
        replace_with_file(scratch)
    if mode in ("business", "business_file"):
        raise ValueError("primary execution failure")
    if mode in ("output", "output_file"):
        return {}
    if mode in ("missing", "missing_access"):
        scratch.joinpath("temporary").unlink()
        scratch.rmdir()
        if mode == "missing_access":
            ctx.scratch_root
    if mode in ("none", "missing"):
        return None
    values = [] if mode == "empty" else [7]
    return dp.Table.from_arrow(pa.table({"value": pa.array(values, type=pa.int64())}))
