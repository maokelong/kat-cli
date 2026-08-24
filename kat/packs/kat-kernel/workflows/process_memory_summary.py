import kat

from kat.pack.analysis.process_memory import summarize_process_memory


@kat.workflow(
    name="process-memory-summary",
    title="Process Memory by Pathname",
    parameters={},
)
def process_memory_summary(ctx: kat.Context):
    """按 SMAPS 快照汇总各 pathname 的常驻内存与按比例分摊内存。"""

    return {"process_memory_by_pathname": summarize_process_memory(ctx)}
