import kat
import pyarrow as pa

from kat.pack.datasources.trace_streamer import TraceStreamerSQLiteProvider


THREAD_CPU_TIME_SCHEMA = pa.schema(
    [
        pa.field("thread_id", pa.int32(), nullable=False),
        pa.field("thread_name", pa.string(), nullable=False),
        pa.field("cpu", pa.uint32(), nullable=False),
        pa.field("observed_cpu_time_ns", pa.int64(), nullable=False),
    ]
)

THREAD_CPU_TIME_SQL = """
WITH complete_slices AS (
    SELECT
        thread.tid AS thread_id,
        thread.name AS thread_name,
        sched_slice.cpu AS cpu,
        sched_slice.dur AS observed_cpu_time_ns
    FROM sched_slice
    INNER JOIN thread ON thread.itid = sched_slice.itid
    WHERE sched_slice.itid <> :idle_itid
      AND sched_slice.dur IS NOT NULL
      AND sched_slice.cpu IS NOT NULL
      AND thread.tid IS NOT NULL
      AND thread.name IS NOT NULL
)
SELECT
    thread_id,
    thread_name,
    cpu,
    COALESCE(SUM(observed_cpu_time_ns), 0) AS observed_cpu_time_ns
FROM complete_slices
GROUP BY thread_id, thread_name, cpu
ORDER BY
    observed_cpu_time_ns DESC,
    thread_id ASC,
    thread_name ASC,
    cpu ASC
"""


@kat.workflow(
    name="thread-cpu-time",
    description="仅统计可观测完整区间，避免把 Trace 首尾的未知 CPU 时间补入结果。",
    parameters={
        "sqlite_path": "Absolute path to a Trace Streamer SQLite database.",
    },
)
def thread_cpu_time(ctx: kat.Context, sqlite_path: str):
    """仅统计可观测完整区间，避免把 Trace 首尾的未知 CPU 时间补入结果。"""
    del ctx
    provider = TraceStreamerSQLiteProvider(sqlite_path=sqlite_path)
    return {
        "thread_cpu_time_by_cpu": provider.query(
            THREAD_CPU_TIME_SQL,
            schema=THREAD_CPU_TIME_SCHEMA,
            params={"idle_itid": 0},
        )
    }
