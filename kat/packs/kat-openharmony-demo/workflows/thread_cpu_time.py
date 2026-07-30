import kat


@kat.workflow(
    name="thread-cpu-time",
    title="Thread CPU Time by CPU",
    required_tables=["sched_slice", "thread"],
    parameters={},
)
def thread_cpu_time(ctx: kat.Context):
    """仅统计可观测完整区间，避免把 Trace 首尾的未知 CPU 时间补入结果。"""
    result = ctx.sql(
        """
        WITH complete_slices AS (
            SELECT
                COALESCE(arrow_cast(thread.tid, 'Int32'), arrow_cast(0, 'Int32'))
                    AS thread_id,
                COALESCE(arrow_cast(thread.name, 'Utf8'), '') AS thread_name,
                COALESCE(arrow_cast(sched_slice.cpu, 'UInt32'), arrow_cast(0, 'UInt32'))
                    AS cpu,
                COALESCE(arrow_cast(sched_slice.dur, 'Int64'), arrow_cast(0, 'Int64'))
                    AS observed_cpu_time_ns
            FROM sched_slice
            INNER JOIN thread ON thread.itid = sched_slice.itid
            WHERE sched_slice.itid <> 0
              AND sched_slice.dur IS NOT NULL
              AND sched_slice.cpu IS NOT NULL
              AND thread.tid IS NOT NULL
              AND thread.name IS NOT NULL
        )
        SELECT
            thread_id,
            thread_name,
            cpu,
            COALESCE(
                arrow_cast(
                    SUM(CAST(observed_cpu_time_ns AS DECIMAL(38, 0))),
                    'Int64'
                ),
                CAST(0 AS BIGINT)
            ) AS observed_cpu_time_ns
        FROM complete_slices
        GROUP BY thread_id, thread_name, cpu
        ORDER BY
            observed_cpu_time_ns DESC,
            thread_id ASC,
            thread_name ASC,
            cpu ASC
        """
    )
    return {"thread_cpu_time_by_cpu": result}
