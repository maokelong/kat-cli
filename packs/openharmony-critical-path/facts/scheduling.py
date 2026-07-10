from __future__ import annotations

from kat import fact


@fact(title="Wakeup edges", description="Bounded sched_wakeup edges")
def wakeup_edges(kat, target_itid: int, start_ts: int, end_ts: int):
    return kat.sql(
        """
        select ts as wakeup_ts, ref as target_itid, wakeup_from as waker_itid, name
        from instant
        where ref_type = 'itid'
          and name like 'sched_wakeup%'
          and wakeup_from is not null
          and ref = $target_itid
          and ts between $start_ts and $end_ts
        order by ts
        """,
        target_itid=target_itid,
        start_ts=start_ts,
        end_ts=end_ts,
    )


@fact(title="Scheduling slices", description="Bounded scheduler slices by itid")
def sched_slices(kat, itid: int, start_ts: int, end_ts: int):
    return kat.sql(
        """
        select itid, ts, dur, ts_end, cpu, priority, end_state
        from sched_slice
        where itid = $itid
          and ts < $end_ts
          and ts + dur > $start_ts
        order by ts, dur
        """,
        itid=itid,
        start_ts=start_ts,
        end_ts=end_ts,
    )
