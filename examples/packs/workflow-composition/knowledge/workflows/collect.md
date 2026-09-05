# 无输出编排结果

本 Workflow 的 `outputs` 为 `{}`，表示编排成功而未生成自己的表，不表示没有异常或没有分析结果。

从 Session inventory 找到当前 Run 的两个直接 `child_runs`：facts 和 summarize。分别读取对应 Workflow detail 的 Guide，并查询各自最少证据。需要时汇总它们的解释，明确区分 base=5 的独立事实与 base=20 的汇总样本；不要把 summarize 的后代再当作 collect 的直接结果。

缺省 Guide 可以不解释。证据不足时说明缺口，仅在输入已知、授权未扩大且会增加证据时执行下一个 Workflow；否则询问用户或停止。程序控制流不由 Guide 修改。
