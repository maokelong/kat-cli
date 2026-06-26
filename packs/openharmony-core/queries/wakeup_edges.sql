WITH window AS (
  SELECT itid, start_ts, end_ts FROM first_draw_window LIMIT 1
)
SELECT
  instant.ts AS wake_ts,
  window.itid AS target_itid,
  thread.itid AS waker_itid,
  instant.name AS wakeup_name
FROM instant
LEFT JOIN thread ON instant.ref = thread.itid
JOIN window ON instant.ts BETWEEN window.start_ts AND window.end_ts
WHERE instant.name LIKE '%sched_wakeup%';
