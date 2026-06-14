SELECT
  c.id,
  c.ts AS marker_ts,
  c.dur AS marker_dur,
  c.callid AS itid,
  t.tid,
  t.name AS thread_name,
  t.ipid,
  p.pid,
  p.name AS process_name,
  c.name AS marker_payload
FROM callstack c
LEFT JOIN thread t ON c.callid = t.itid
LEFT JOIN process p ON t.ipid = p.ipid
WHERE LOWER(COALESCE(c.name, '')) LIKE '%firstdrawframe:1%'
  AND (
    LOWER(COALESCE(p.name, '')) LIKE LOWER('%${process_query}%')
    OR CAST(p.pid AS TEXT) = '${process_query}'
    OR CAST(t.tid AS TEXT) = '${process_query}'
  )
ORDER BY c.ts
LIMIT ${max_rows}
