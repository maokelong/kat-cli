SELECT
  cpu,
  itid,
  ts,
  dur,
  priority,
  end_state
FROM sched_slice
WHERE itid = ${itid}
  AND ts + COALESCE(dur, 0) > ${start_ts}
  AND ts < ${end_ts}
ORDER BY ts
LIMIT ${max_rows}
