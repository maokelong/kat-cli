import kat


@kat.workflow(
    name="thread-cpu-time",
    title="Thread CPU Time by CPU",
    required_tables=["sched_switch"],
    parameters={},
)
def thread_cpu_time(ctx: kat.Context):
    """Aggregate complete observed non-idle scheduling intervals by thread and CPU."""
    result = ctx.sql(
        """
        WITH ordered_switches AS (
            SELECT
                clock_domain,
                clock_value,
                cpu,
                previous_thread_id AS thread_id,
                arrow_cast(previous_thread_name, 'Utf8') AS thread_name,
                lag(clock_value) OVER (
                    PARTITION BY clock_domain, cpu
                    ORDER BY cpu_switch_sequence
                ) AS previous_clock_value
            FROM sched_switch
        ), observed_intervals AS (
            SELECT
                thread_id,
                thread_name,
                cpu,
                CAST(clock_value - previous_clock_value AS BIGINT)
                    AS observed_cpu_time_ns
            FROM ordered_switches
            WHERE previous_clock_value IS NOT NULL
              AND thread_id <> 0
        )
        SELECT
            thread_id,
            thread_name,
            cpu,
            COALESCE(SUM(observed_cpu_time_ns), CAST(0 AS BIGINT))
                AS observed_cpu_time_ns
        FROM observed_intervals
        GROUP BY thread_id, thread_name, cpu
        ORDER BY
            observed_cpu_time_ns DESC,
            thread_id ASC,
            thread_name ASC,
            cpu ASC
        """
    )
    return {"thread_cpu_time_by_cpu": result}
