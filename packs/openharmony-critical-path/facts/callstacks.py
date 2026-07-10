from __future__ import annotations

from kat import fact


@fact(title="Callstack slices", description="Bounded callstack slices by itid")
def callstack_slices(kat, itid: int, start_ts: int, end_ts: int):
    return kat.sql(
        """
        select callid as itid, ts, dur, name
        from callstack
        where callid = $itid
          and ts < $end_ts
          and ts + dur > $start_ts
        order by ts, dur
        """,
        itid=itid,
        start_ts=start_ts,
        end_ts=end_ts,
    )
