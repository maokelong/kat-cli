from __future__ import annotations

from kat import fact


@fact(title="Thread metadata", description="Thread and process names by itid")
def thread_metadata(kat):
    return kat.sql(
        """
        select
          t.itid,
          t.tid,
          p.pid,
          t.name as thread_name,
          p.name as process_name
        from thread t
        left join process p on t.ipid = p.ipid
        """
    )


@fact(title="Wakeup edges", description="sched_wakeup edges from trace_streamer instant table")
def wakeup_edges(kat):
    return kat.sql(
        """
        select
          ts as wakeup_ts,
          ref as target_itid,
          wakeup_from as waker_itid,
          name
        from instant
        where ref_type = 'itid'
          and name like 'sched_wakeup%'
          and wakeup_from is not null
        """
    )
