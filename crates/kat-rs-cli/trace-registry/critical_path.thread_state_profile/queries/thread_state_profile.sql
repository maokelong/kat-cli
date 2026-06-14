SELECT
  s.ts,
  s.dur,
  s.cpu,
  s.state,
  CASE WHEN s.state = 'D-IO' THEN 1 ELSE 0 END AS io_wait,
  NULL AS blocked_function,
  (
    SELECT i.wakeup_from
    FROM instant i
    WHERE i.name = 'sched_wakeup'
      AND s.state IN ('R', 'R+', 'S', 'D', 'D-IO')
      AND i.ref = s.itid
      AND i.wakeup_from IS NOT NULL
      AND i.ts >= s.ts
      AND i.ts < s.ts + COALESCE(s.dur, 0)
    ORDER BY i.ts
    LIMIT 1
  ) AS waker_itid,
  (
    SELECT i.ts
    FROM instant i
    WHERE i.name = 'sched_wakeup'
      AND s.state IN ('R', 'R+', 'S', 'D', 'D-IO')
      AND i.ref = s.itid
      AND i.wakeup_from IS NOT NULL
      AND i.ts >= s.ts
      AND i.ts < s.ts + COALESCE(s.dur, 0)
    ORDER BY i.ts
    LIMIT 1
  ) AS wakeup_ts,
  s.itid,
  s.tid,
  s.pid,
  t.name AS thread_name,
  p.name AS process_name
FROM thread_state s
LEFT JOIN thread t ON s.itid = t.itid
LEFT JOIN process p ON t.ipid = p.ipid
WHERE s.itid = ${itid}
  AND s.ts + COALESCE(s.dur, 0) > ${start_ts}
  AND s.ts < ${end_ts}
  AND COALESCE(s.dur, 0) >= ${min_segment_ns}
ORDER BY s.ts
LIMIT ${max_rows}
