# 分析特殊情况处理

正常任务及读取已有记录直接按 [分析入口](../SKILL.md) 执行。只在修订冲突、响应不确定、记录格式问题、多快照歧义、显式换方法或删除 Session 时读取本文件。

## 修订冲突与响应不确定

每次 save 使用最近已核对 revision。材料操作只有 previous_revision 与该值一致时才能直接采用回执 revision；否则先 `kat analysis show --session S` 核对并发修改。冲突不能仅换数字重提，须结合当前目标、正文和材料判断自己的修改是否仍适用。

保存、采纳、归档或 run 的响应不确定时，先读取已知 Session 的记录或 Run inventory 确认实际事实，不盲目重发。重复执行可能产生新 Run 或外部副作用。失败不能覆盖上次完整内容；已确认的部分判断能保存时及时保存，交付区分判断与保存状态。

明确没有记录才允许以现存事实新建；损坏或未知格式不是“没有记录”。`analysis show` 不升级旧格式；新写入为 schema 3，兼容读取 schema 2 的 Query 材料与报告，不接受未知版本或 schema 2 的知识材料。不能自行修补私有文件或把当前 Guide 当历史依据。

## 多快照与显式换方法

按入口或路径普通读取优先复用唯一旧快照，不因 PACK 更新而改变方法；同键多快照时错误列出候选 ID。按保存正文的材料 ID 恢复，仍无法确定时向用户澄清，不猜“最新”。

只有用户明确要求采用当前方法，才使用原入口或引用选择器并加配对的更新参数：

```text
kat knowledge read --session S --pack P --entry E --update --expected-revision R
kat knowledge read --session S --pack P --path F --update --expected-revision R
```

需要外部 PACK 时附 `--pack-dir D`。更新在锁内比对 revision、追加不可变材料 ID，保留旧 ID；失败或回执不确定按上一节处理。`--material M` 只读取知识材料，与 pack、entry、path、pack-dir、update、expected-revision 互斥。Query 材料用 `analysis show --material M`。

换方法后核对受影响判断并保存修订；新方法不能暗示事实已更新。旧入口中未采纳的引用不保证历史可恢复，当前文件不能冒充历史快照；必需引用缺失时说明方法缺口。方法恢复不等于外部输入可重现。

## 删除已知 Session

仅在用户明确要求永久删除这个已知 Session 时调用：

```text
kat session delete --session S
```

成功 result 只有 session_id。它永久删除 Session 的 Run、Output、物化、scratch 和 Analysis Record（含知识快照与报告），不删除 Session 外既有日志或查询结果，不创建 Operation log。活跃操作占用时立即失败且不修改；失败已进入内部 tombstone 时可用同一命令续删。

没有单 Run 删除、TTL 或自动 GC。分析结束、换目标或可能缺磁盘空间都不是删除授权，不自行扫描或清理目录。
