# 冷启动调度/内核 Topdown

冷启动分析先确认启动窗口和主线程，再判断主线程在窗口内是 runnable 等待、CPU 竞争、还是 blocked 时间占主导。

Topdown Brief 要先回答当前 trace 中实际存在什么信号：

- 目标进程是否能识别；
- 启动窗口是否可信；
- 主线程状态分布是否偏向 runnable 或 blocked；
- CPU 是否被其他线程或进程占用；
- 缺失哪些调度、线程状态或阻塞字段。

