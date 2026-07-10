from __future__ import annotations

from kat import fact


@fact(title="First frame window", description="First positive application frame on the main thread")
def first_frame_window(kat, app_name: str):
    return kat.sql(
        """
        select f.itid as root_itid, f.ts as start_ts, f.ts + f.dur as end_ts
        from frame_slice f
        join process p on p.ipid = f.ipid
        join thread t on t.itid = f.itid
        where p.name = :app_name
          and f.type = 0
          and f.dur > 0
        order by t.is_main_thread desc, f.ts
        limit 1
        """,
        app_name=app_name,
    )
