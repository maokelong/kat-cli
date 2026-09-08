# 定位首个实际帧

本 Guide 是当前 PACK 版本的分析策略，不是 Output Schema、可执行计划或历史 Run
快照。先以成功 `kat run` Response 的 `result.outputs` 为准；只有 Run ID 时，先查询
`information_schema` 发现实际 Output 和 columns。若实际 inventory 与下文不同，停止
使用示例 SQL，以实际 inventory 为权威。

## 查询最少证据

当前 Workflow 预期产生单行 `frame_window`。如果只需定位首帧，查询回答该问题所需的
身份和窗口字段：

```sql
SELECT frame_id, start_ts, end_ts, duration_ns,
       process_name, thread_name, clock_domain
FROM output.frame_window
LIMIT 1
```

`start_ts`、`end_ts` 和 `duration_ns` 是 `clock_domain` 所指时钟域中的纳秒值，不能
直接当作墙上时间或与其他时钟域比较。报告查询到的帧、线程和窗口后即可停止。

如果用户还要求解释该窗口中的调度耗时来源，只查询传给后续 Workflow 所需的字段：

```sql
SELECT root_itid, start_ts, end_ts
FROM output.frame_window
LIMIT 1
```

不要为了链式执行提前读取 `callstack_id` 或整张 Output。

## 受控继续分析

把 `extract-critical-path` 视为候选 Workflow，而不是自动跳转。执行前必须重新调用
`kat inspect workflow --pack kat-openharmony-critical-path --workflow
extract-critical-path`，并以本次 inspection 的参数合同为准；这里的 `kat` 代表 KAT
Skill 已选择的绝对载荷路径。

参数来源必须逐项成立：

- `root_itid`、`start_ts` 和 `end_ts` 来自上述同一条实际 Query Result，不能来自
  Guide、Run ID 或历史分析猜测。
- `sqlite_path` 不属于 `frame_window`，只能沿用用户提供或当前执行上下文中已知且仍获
  授权的同一绝对 SQLite 路径。只有旧 Run ID 而没有该路径时，请求用户补充。
- `max_depth` 和 `min_segment_ms` 使用本次 inspection 展示的默认值时应省略；只有用户
  明确给出其他值时才传入。

确认继续分析符合用户目标、没有扩大 Datasource 授权且会增加证据后，显式执行另一个
Run：

```text
kat run --pack kat-openharmony-critical-path \
  --workflow extract-critical-path -- \
  --sqlite-path <同一已授权绝对路径> \
  --root-itid <查询值> --start-ts <查询值> --end-ts <查询值>
```

前后两个 Workflow 始终产生两个独立 Run，前一个 Output 不会自动成为后一个输入。
Output inventory、Query、候选 inspection、参数来源或授权任一项不能确认时停止，并说明
缺少的最小信息；不要扫描 Run 文件、重复同一执行或自行扩大访问范围。
