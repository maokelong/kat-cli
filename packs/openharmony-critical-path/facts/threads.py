from __future__ import annotations

from kat import fact


@fact(title="Thread metadata", description="Thread and process names by itid")
def thread_metadata(kat, itid: int):
    return kat.sql(
        """
        select t.itid, t.tid, t.name as thread_name, p.pid, p.name as process_name
        from thread t
        left join process p on p.ipid = t.ipid
        where t.itid = :itid
        """,
        itid=itid,
    )


@fact(title="Thread state segments", description="Raw thread states and decoded blocking arguments")
def thread_state_segments(kat, itid: int, start_ts: int, end_ts: int):
    return kat.sql(
        """
        with decoded as (
          select
            a.argset,
            max(case when key_dict.data = 'iowait' then a.value end) as iowait,
            max(case when key_dict.data = 'caller' and a.datatype = 1 then value_dict.data end) as blocked_caller
          from args a
          join data_dict key_dict on key_dict.id = a.key
          left join data_dict value_dict on value_dict.id = a.value
          group by a.argset
        )
        select s.itid, s.ts, s.dur, s.state, s.cpu, s.arg_setid,
               decoded.iowait, decoded.blocked_caller
        from thread_state s
        left join decoded on decoded.argset = s.arg_setid
        where s.itid = :itid
          and s.ts < :end_ts
          and s.ts + s.dur > :start_ts
        order by s.ts, s.dur
        """,
        itid=itid,
        start_ts=start_ts,
        end_ts=end_ts,
    )
