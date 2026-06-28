SELECT
  itid,
  ts AS start_ts,
  ts + COALESCE(dur, 0) AS end_ts,
  state,
  CASE
    WHEN state IN ('R', 'R+') THEN 'runnable'
    WHEN state IN ('S') THEN 'sleeping'
    WHEN state IN ('D', 'D-IO') THEN 'uninterruptible'
    ELSE 'other'
  END AS state_class
FROM thread_state
WHERE itid = ${itid}
