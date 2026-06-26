WITH window AS (
  SELECT itid, start_ts, end_ts, dur_ns FROM first_draw_window LIMIT 1
),
segments AS (
  SELECT
    ts.state,
    MAX(ts.ts, window.start_ts) AS overlap_start,
    MIN(ts.ts + COALESCE(ts.dur, 0), window.end_ts) AS overlap_end,
    window.dur_ns AS window_dur_ns
  FROM thread_state ts
  JOIN window ON ts.itid = window.itid
  WHERE ts.ts < window.end_ts
    AND ts.ts + COALESCE(ts.dur, 0) > window.start_ts
),
summary AS (
  SELECT
    state,
    SUM(CASE WHEN overlap_end > overlap_start THEN overlap_end - overlap_start ELSE 0 END) AS dur_ns,
    MAX(window_dur_ns) AS window_dur_ns
  FROM segments
  GROUP BY state
),
ranked AS (
  SELECT
    state,
    dur_ns,
    window_dur_ns,
    ROUND(dur_ns * 100.0 / NULLIF(window_dur_ns, 0), 3) AS percent,
    ROW_NUMBER() OVER (ORDER BY dur_ns DESC, state ASC) AS rank
  FROM summary
)
SELECT
  (SELECT itid FROM window) AS itid,
  (SELECT start_ts FROM window) AS start_ts,
  (SELECT end_ts FROM window) AS end_ts,
  (SELECT dur_ns FROM window) AS window_dur_ns,
  state AS dominant_state,
  dur_ns AS dominant_dur_ns,
  percent AS dominant_percent,
  (SELECT COALESCE(SUM(dur_ns), 0) FROM summary WHERE state = 'Running') AS running_dur_ns,
  (SELECT COALESCE(SUM(dur_ns), 0) FROM summary WHERE state IN ('R', 'R+')) AS runnable_dur_ns,
  (SELECT COALESCE(SUM(dur_ns), 0) FROM summary WHERE state = 'S') AS sleeping_dur_ns,
  (SELECT COALESCE(SUM(dur_ns), 0) FROM summary WHERE state IN ('D', 'D-IO')) AS blocked_dur_ns
FROM ranked
WHERE rank = 1;
