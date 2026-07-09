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
    path_ctes = """
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
           and instant.ts between frontier.window_start and frontier.window_end
           and instant.wakeup_from is not null
          where frontier.depth < :max_depth
            and state.state in ('S', 'D')
        ),
        candidate_states as (
          select
            frontier.depth,
            frontier.itid,
            frontier.window_start,
            frontier.window_end,
            frontier.parent_itid,
            frontier.wakeup_ts,
            case
              when state.ts > frontier.window_start then state.ts
              else frontier.window_start
            end as segment_start_ts,
            case
              when state.ts + state.dur < frontier.window_end then state.ts + state.dur
              else frontier.window_end
            end as segment_end_ts,
            state.state
          from frontier
          join thread_state state
            on state.itid = frontier.itid
           and state.ts < frontier.window_end
           and state.ts + state.dur > frontier.window_start
        ),
        ranked_states as (
          select
            candidate_states.depth,
            candidate_states.itid,
            candidate_states.window_start,
            candidate_states.window_end,
            candidate_states.parent_itid,
            candidate_states.wakeup_ts,
            candidate_states.segment_start_ts,
            candidate_states.segment_end_ts,
            candidate_states.segment_end_ts - candidate_states.segment_start_ts as dur,
            candidate_states.state,
            row_number() over (
              partition by candidate_states.depth, candidate_states.itid, candidate_states.window_start, candidate_states.window_end
              order by candidate_states.segment_end_ts - candidate_states.segment_start_ts desc, candidate_states.segment_start_ts
            ) as rank
          from candidate_states
          where candidate_states.segment_end_ts - candidate_states.segment_start_ts >= :min_segment_ns
        ),
        path_nodes as (
          select *
          from ranked_states
          where rank = 1
        )
    """
    nodes = kat.sql(
        path_ctes
        + """
        select
          path_nodes.depth,
          path_nodes.itid,
          thread.tid,
          thread.name as thread_name,
          process.pid,
          process.name as process_name,
          path_nodes.segment_start_ts,
          path_nodes.segment_end_ts,
          path_nodes.dur,
          path_nodes.state,
          case
            when path_nodes.state = 'Running' then 'self_running'
            when path_nodes.state in ('R', 'R+') then 'scheduler_wait'
            when path_nodes.state in ('S', 'D') and path_nodes.wakeup_ts is not null then 'waiting_for_waker'
            when path_nodes.state in ('S', 'D') then 'blocked_without_waker'
            else 'unknown'
          end as classification,
          callstack.name as evidence,
          case
            when path_nodes.state in ('S', 'D') and path_nodes.wakeup_ts is null then 'missing_waker'
            else null
          end as uncertainty
        from path_nodes
        left join thread
          on path_nodes.itid = thread.itid
        left join process
          on thread.ipid = process.ipid
        left join callstack
          on callstack.callid = path_nodes.itid
         and callstack.ts < path_nodes.segment_end_ts
         and callstack.ts + callstack.dur > path_nodes.segment_start_ts
        order by path_nodes.depth, path_nodes.segment_start_ts
        """,
        root_itid=root_itid,
        start_ts=start_ts,
        end_ts=end_ts,
        max_depth=max_depth,
        min_segment_ns=min_segment_ns,
    )

    edges = kat.sql(
        path_ctes
        + """
        select
          child.depth - 1 as parent_depth,
          child.depth as child_depth,
          child.itid as from_itid,
          child.parent_itid as to_itid,
          child.wakeup_ts,
          'sched_wakeup' as edge_type,
          'fact' as confidence,
          'instant.sched_wakeup wakeup_from' as reason
        from path_nodes child
        where child.depth > 0
          and exists (
            select 1
            from path_nodes parent
            where parent.depth = child.depth - 1
              and parent.itid = child.parent_itid
          )
        order by child.depth, child.wakeup_ts
        """,
        root_itid=root_itid,
        start_ts=start_ts,
        end_ts=end_ts,
        max_depth=max_depth,
        min_segment_ns=min_segment_ns,
    )

    return {"path_nodes": nodes, "path_edges": edges}
