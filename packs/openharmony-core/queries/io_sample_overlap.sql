WITH window AS (
  SELECT start_ts, end_ts FROM first_draw_window LIMIT 1
),
fs AS (
  SELECT 'file_system_sample' AS source, ts, dur, name FROM file_system_sample, window
  WHERE ts < window.end_ts AND ts + COALESCE(dur, 0) > window.start_ts
),
bio AS (
  SELECT 'bio_latency_sample' AS source, ts, dur, name FROM bio_latency_sample, window
  WHERE ts < window.end_ts AND ts + COALESCE(dur, 0) > window.start_ts
),
disk AS (
  SELECT 'diskio' AS source, ts, dur, name FROM diskio, window
  WHERE ts < window.end_ts AND ts + COALESCE(dur, 0) > window.start_ts
),
sys AS (
  SELECT 'syscall' AS source, ts, dur, name FROM syscall, window
  WHERE ts < window.end_ts AND ts + COALESCE(dur, 0) > window.start_ts
)
SELECT * FROM fs
UNION ALL SELECT * FROM bio
UNION ALL SELECT * FROM disk
UNION ALL SELECT * FROM sys;
