SELECT
  c.id,
  c.callid AS itid,
  t.tid,
  t.name AS thread_name,
  p.name AS process_name,
  c.ts,
  c.dur,
  c.name,
  c.cat,
  c.depth,
  c.parent_id
FROM callstack c
LEFT JOIN thread t ON c.callid = t.itid
LEFT JOIN process p ON t.ipid = p.ipid
WHERE c.ts + COALESCE(c.dur, 0) > ${start_ts}
  AND c.ts < ${end_ts}
  AND COALESCE(${itid}, c.callid) = c.callid
ORDER BY c.ts, c.depth
LIMIT ${max_rows}
