SELECT
  t.itid,
  t.tid,
  t.name AS thread_name,
  t.is_main_thread,
  t.ipid,
  p.pid,
  p.name AS process_name
FROM thread t
LEFT JOIN process p ON t.ipid = p.ipid
WHERE t.itid IN (${itids})
ORDER BY t.itid
LIMIT 1000
