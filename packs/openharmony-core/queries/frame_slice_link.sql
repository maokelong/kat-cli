WITH window AS (
  SELECT itid, ipid, vsync_id, start_ts, end_ts FROM first_draw_window LIMIT 1
),
app_frame AS (
  SELECT *
  FROM frame_slice, window
  WHERE frame_slice.itid = window.itid
    AND frame_slice.ipid = window.ipid
    AND frame_slice.ts <= window.start_ts
    AND frame_slice.ts + COALESCE(frame_slice.dur, 0) >= window.end_ts
  ORDER BY frame_slice.ts DESC
  LIMIT 1
),
rs_frame AS (
  SELECT rs.*
  FROM frame_slice rs
  JOIN app_frame app ON instr(COALESCE(rs.src, ''), CAST(app.id AS TEXT)) > 0
  ORDER BY rs.ts ASC
  LIMIT 1
)
SELECT
  app_frame.id AS app_frame_id,
  app_frame.itid AS app_itid,
  app_frame.ipid AS app_ipid,
  app_frame.ts AS app_start_ts,
  app_frame.ts + COALESCE(app_frame.dur, 0) AS app_end_ts,
  COALESCE(app_frame.dur, 0) AS app_dur_ns,
  rs_frame.id AS render_service_frame_id,
  rs_frame.itid AS render_service_itid,
  rs_frame.ipid AS render_service_ipid,
  rs_frame.ts AS render_start_ts,
  rs_frame.ts + COALESCE(rs_frame.dur, 0) AS render_end_ts,
  COALESCE(rs_frame.dur, 0) AS render_dur_ns,
  ROUND(COALESCE(rs_frame.dur, 0) / 1000000.0, 3) AS downstream_dur_ms
FROM app_frame
LEFT JOIN rs_frame ON 1 = 1;
