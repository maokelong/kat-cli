SELECT
  link.render_service_frame_id,
  link.render_service_itid,
  link.render_start_ts,
  link.render_end_ts,
  link.render_dur_ns,
  c.id AS callstack_id,
  c.name,
  c.ts AS start_ts,
  c.ts + COALESCE(c.dur, 0) AS end_ts,
  COALESCE(c.dur, 0) AS dur_ns
FROM frame_slice_link link
LEFT JOIN callstack c
  ON c.callid = link.render_service_itid
 AND c.ts < link.render_end_ts
 AND c.ts + COALESCE(c.dur, 0) > link.render_start_ts
ORDER BY dur_ns DESC, callstack_id ASC;
