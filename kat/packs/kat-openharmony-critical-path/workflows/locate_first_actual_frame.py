import kat

from kat.pack.datasources.trace_streamer import TraceStreamerSQLiteProvider
from kat.pack.helpers.critical_path import TraceStreamerFacts, locate_first_actual_frame


@kat.workflow(
    name="locate-first-actual-frame",
    description="定位指定进程最早完成且持续时间为正的实际帧。",
    parameters={
        "sqlite_path": "Absolute path to a Trace Streamer SQLite database.",
        "process_name": "Exact process name to locate.",
    },
)
def locate_first_actual_frame_workflow(
    ctx: kat.Context,
    sqlite_path: str,
    process_name: str,
):
    """定位指定进程最早完成且持续时间为正的实际帧。"""
    del ctx
    provider = TraceStreamerSQLiteProvider(sqlite_path=sqlite_path)
    return {"frame_window": locate_first_actual_frame(TraceStreamerFacts(provider), process_name)}
