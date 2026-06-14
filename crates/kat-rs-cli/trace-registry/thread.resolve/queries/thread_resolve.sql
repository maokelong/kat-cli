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
WHERE CAST(t.itid AS TEXT) = '${thread_query}'
   OR CAST(t.tid AS TEXT) = '${thread_query}'
   OR LOWER(COALESCE(t.name, '')) LIKE LOWER('%${thread_query}%')
   OR LOWER(COALESCE(p.name, '')) LIKE LOWER('%${thread_query}%')
ORDER BY
  CASE WHEN CAST(t.itid AS TEXT) = '${thread_query}' THEN 0 ELSE 1 END,
  CASE WHEN CAST(t.tid AS TEXT) = '${thread_query}' THEN 0 ELSE 1 END,
  t.is_main_thread DESC,
  t.switch_count DESC,
  t.itid
LIMIT ${max_rows}
