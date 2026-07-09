from __future__ import annotations

from kat import workflow
from compute.critical_path import critical_path


@workflow(
    title="WeChat first-frame critical path",
    description="Find the first WeChat window frame and extract its conservative critical path",
)
def wechat_first_frame_critical_path(
    kat,
    app_name: str = ".tencent.wechat",
    max_depth: int = 8,
    min_segment_ms: float = 0.1,
):
    target = kat.sql(
        """
        select
          frame_slice.itid as root_itid,
          frame_slice.ts as start_ts,
          frame_slice.ts + frame_slice.dur as end_ts
        from frame_slice
        join process
          on frame_slice.ipid = process.ipid
        left join thread
          on frame_slice.itid = thread.itid
        where process.name = :app_name
          and frame_slice.type = 0
          and frame_slice.dur > 0
        order by
          case when thread.is_main_thread = 1 then 0 else 1 end,
          frame_slice.ts
        limit 1
        """,
        app_name=app_name,
    )
    rows = target.collect()
    if not rows or rows[0].num_rows == 0:
        empty_nodes = kat.sql(
            """
            select
              cast(null as bigint) as depth,
              cast(null as bigint) as itid,
              cast(null as bigint) as tid,
              cast(null as varchar) as thread_name,
              cast(null as bigint) as pid,
              cast(null as varchar) as process_name,
              cast(null as bigint) as segment_start_ts,
              cast(null as bigint) as segment_end_ts,
              cast(null as bigint) as dur,
              cast(null as varchar) as state,
              cast('target_not_found' as varchar) as classification,
              cast(null as varchar) as evidence,
              cast('missing_first_frame' as varchar) as uncertainty
            where false
            """
        )
        empty_edges = kat.sql(
            """
            select
              cast(null as bigint) as parent_depth,
              cast(null as bigint) as child_depth,
              cast(null as bigint) as from_itid,
              cast(null as bigint) as to_itid,
              cast(null as bigint) as wakeup_ts,
              cast(null as varchar) as edge_type,
              cast(null as varchar) as confidence,
              cast(null as varchar) as reason
            where false
            """
        )
        return {"path_nodes": empty_nodes, "path_edges": empty_edges}

    data = rows[0].to_pydict()
    root_itid = data["root_itid"][0]
    start_ts = data["start_ts"][0]
    end_ts = data["end_ts"][0]
    return critical_path(
        kat,
        root_itid=root_itid,
        start_ts=start_ts,
        end_ts=end_ts,
        max_depth=max_depth,
        min_segment_ms=min_segment_ms,
    )
