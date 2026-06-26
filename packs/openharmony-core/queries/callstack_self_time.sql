WITH child_time AS (
  SELECT
    parent_id AS callstack_id,
    SUM(overlap_dur_ns) AS child_overlap_dur_ns
  FROM callstack_overlap_window
  WHERE parent_id IS NOT NULL
  GROUP BY parent_id
)
SELECT
  c.callstack_id,
  c.parent_id,
  c.itid,
  c.name,
  c.overlap_dur_ns AS inclusive_dur_ns,
  MAX(c.overlap_dur_ns - COALESCE(child_time.child_overlap_dur_ns, 0), 0) AS exclusive_dur_ns,
  ROW_NUMBER() OVER (ORDER BY c.overlap_dur_ns DESC, c.callstack_id ASC) AS inclusive_rank,
  ROW_NUMBER() OVER (ORDER BY MAX(c.overlap_dur_ns - COALESCE(child_time.child_overlap_dur_ns, 0), 0) DESC, c.callstack_id ASC) AS exclusive_rank
FROM callstack_overlap_window c
LEFT JOIN child_time ON c.callstack_id = child_time.callstack_id
ORDER BY inclusive_rank;
