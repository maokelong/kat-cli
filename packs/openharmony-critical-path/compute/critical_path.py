from __future__ import annotations

from kat import compute


@compute(title="Critical path", description="Extract a conservative thread dependency path")
def critical_path(
    kat,
    root_itid: int,
    start_ts: int,
    end_ts: int,
    max_depth: int = 8,
    min_segment_ms: float = 0.1,
):
    min_segment_ns = int(min_segment_ms * 1_000_000)
    nodes = kat.sql(
        """
        with recursive frontier(depth, itid, window_start, window_end, parent_itid, wakeup_ts) as (
          select
            0 as depth,
            cast(:root_itid as bigint) as itid,
            cast(:start_ts as bigint) as window_start,
            cast(:end_ts as bigint) as window_end,
            cast(null as bigint) as parent_itid,
            cast(null as bigint) as wakeup_ts
          union all
          select
            frontier.depth + 1 as depth,
            instant.wakeup_from as itid,
            frontier.window_start as window_start,
            instant.ts as window_end,
            frontier.itid as parent_itid,
            instant.ts as wakeup_ts
          from frontier
          join thread_state state
            on state.itid = frontier.itid
           and state.ts < frontier.window_end
           and state.ts + state.dur > frontier.window_start
          join instant
            on instant.ref = frontier.itid
           and instant.ref_type = 'itid'
           and instant.name like 'sched_wakeup%'
           and instant.ts between state.ts and state.ts + state.dur
           and instant.wakeup_from is not null
          left join thread waker_thread
            on waker_thread.itid = instant.wakeup_from
          where frontier.depth < :max_depth
            and state.state in ('S', 'D')
            and coalesce(waker_thread.name, '') <> 'udk-irq'
        ),
        ranked_states as (
          select
            frontier.depth,
            frontier.itid,
            frontier.parent_itid,
            frontier.wakeup_ts,
            state.ts as segment_start_ts,
            state.ts + state.dur as segment_end_ts,
            state.dur,
            state.state,
            row_number() over (
              partition by frontier.depth, frontier.itid, frontier.window_start, frontier.window_end
              order by state.dur desc, state.ts
            ) as rank
          from frontier
          join thread_state state
            on state.itid = frontier.itid
           and state.ts < frontier.window_end
           and state.ts + state.dur > frontier.window_start
          where state.dur >= :min_segment_ns
        )
        select
          ranked_states.depth,
          ranked_states.itid,
          thread.tid,
          thread.name as thread_name,
          process.pid,
          process.name as process_name,
          ranked_states.segment_start_ts,
          ranked_states.segment_end_ts,
          ranked_states.dur,
          ranked_states.state,
          case
            when ranked_states.state = 'Running' then 'self_running'
            when ranked_states.state in ('R', 'R+') then 'scheduler_wait'
            when ranked_states.state in ('S', 'D') and ranked_states.wakeup_ts is not null then 'waiting_for_waker'
            when ranked_states.state in ('S', 'D') then 'blocked_without_waker'
            else 'unknown'
          end as classification,
          callstack.name as evidence,
          case
            when ranked_states.state in ('S', 'D') and ranked_states.wakeup_ts is null then 'missing_waker'
            else null
          end as uncertainty
        from ranked_states
        left join thread
          on ranked_states.itid = thread.itid
        left join process
          on thread.ipid = process.ipid
        left join callstack
          on callstack.callid = ranked_states.itid
         and callstack.ts < ranked_states.segment_end_ts
         and callstack.ts + callstack.dur > ranked_states.segment_start_ts
        where ranked_states.rank = 1
        order by ranked_states.depth, ranked_states.segment_start_ts
        """,
        root_itid=root_itid,
        start_ts=start_ts,
        end_ts=end_ts,
        max_depth=max_depth,
        min_segment_ns=min_segment_ns,
    )

    edges = kat.sql(
        """
        with recursive frontier(depth, itid, window_start, window_end, parent_itid, wakeup_ts) as (
          select
            0 as depth,
            cast(:root_itid as bigint) as itid,
            cast(:start_ts as bigint) as window_start,
            cast(:end_ts as bigint) as window_end,
            cast(null as bigint) as parent_itid,
            cast(null as bigint) as wakeup_ts
          union all
          select
            frontier.depth + 1 as depth,
            instant.wakeup_from as itid,
            frontier.window_start as window_start,
            instant.ts as window_end,
            frontier.itid as parent_itid,
            instant.ts as wakeup_ts
          from frontier
          join thread_state state
            on state.itid = frontier.itid
           and state.ts < frontier.window_end
           and state.ts + state.dur > frontier.window_start
          join instant
            on instant.ref = frontier.itid
           and instant.ref_type = 'itid'
           and instant.name like 'sched_wakeup%'
           and instant.ts between state.ts and state.ts + state.dur
           and instant.wakeup_from is not null
          left join thread waker_thread
            on waker_thread.itid = instant.wakeup_from
          where frontier.depth < :max_depth
            and state.state in ('S', 'D')
            and coalesce(waker_thread.name, '') <> 'udk-irq'
        )
        select
          depth - 1 as parent_depth,
          depth as child_depth,
          itid as from_itid,
          parent_itid as to_itid,
          wakeup_ts,
          'sched_wakeup' as edge_type,
          'fact' as confidence,
          'instant.sched_wakeup wakeup_from' as reason
        from frontier
        where depth > 0
        order by depth, wakeup_ts
        """,
        root_itid=root_itid,
        start_ts=start_ts,
        end_ts=end_ts,
        max_depth=max_depth,
    )

    return {"path_nodes": nodes, "path_edges": edges}
