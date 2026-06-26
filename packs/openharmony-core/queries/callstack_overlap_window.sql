WITH RECURSIVE window AS (
  SELECT callstack_id, root_callstack_id, itid, start_ts, end_ts FROM first_draw_window LIMIT 1
),
callstack_tree AS (
  SELECT c.id
  FROM callstack c
  JOIN window ON c.id = window.root_callstack_id

  UNION ALL

  SELECT child.id
  FROM callstack child
  JOIN callstack_tree parent ON child.parent_id = parent.id
)
SELECT
  c.id AS callstack_id,
  c.parent_id,
  c.callid AS itid,
  c.name,
  c.ts AS start_ts,
  c.ts + COALESCE(c.dur, 0) AS end_ts,
  COALESCE(c.dur, 0) AS dur_ns,
  MAX(c.ts, window.start_ts) AS overlap_start_ts,
  MIN(c.ts + COALESCE(c.dur, 0), window.end_ts) AS overlap_end_ts,
  CASE
    WHEN MIN(c.ts + COALESCE(c.dur, 0), window.end_ts) > MAX(c.ts, window.start_ts)
    THEN MIN(c.ts + COALESCE(c.dur, 0), window.end_ts) - MAX(c.ts, window.start_ts)
    ELSE 0
  END AS overlap_dur_ns
FROM callstack c
JOIN window ON c.callid = window.itid
JOIN callstack_tree tree ON tree.id = c.id
WHERE c.ts < window.end_ts
  AND c.ts + COALESCE(c.dur, 0) > window.start_ts
ORDER BY overlap_dur_ns DESC, c.id ASC;
