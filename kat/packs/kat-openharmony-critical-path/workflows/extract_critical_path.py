import kat

from kat.pack.helpers.critical_path import extract_critical_path


@kat.workflow(
    name="extract-critical-path",
    description="为一个线程窗口提取有界调度关键路径及调用栈证据。",
    parameters={
        "root_itid": "Root thread internal ID from frame_window.root_itid.",
        "start_ts": "Window start from frame_window.start_ts in boottime nanoseconds.",
        "end_ts": "Window end from frame_window.end_ts in boottime nanoseconds.",
        "max_depth": "Maximum upstream wakeup depth.",
        "min_segment_ms": "Minimum duration before recursive tracing continues.",
    },
)
def extract_critical_path_workflow(ctx: kat.Context, root_itid: int, start_ts: int, end_ts: int, max_depth: int = 8, min_segment_ms: float = 0.1):
    """为一个线程窗口提取有界调度关键路径及调用栈证据。"""
    return extract_critical_path(ctx, root_itid, start_ts, end_ts, max_depth, min_segment_ms)
