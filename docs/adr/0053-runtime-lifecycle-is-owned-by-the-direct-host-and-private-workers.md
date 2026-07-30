---
status: accepted
---

# Runtime 生命周期由直接 Host 与私有 worker 分层拥有

本决定部分取代 ADR-0010 关于 OS 进程容器、全部后代及“只等待 leader 不足”的生命周期决定，因为当前生产路径没有等待任意后台后代的需求；其文件 IPC、退出码、严格 Runtime Response、Operation log 与 Run Manifest 发布门禁继续有效。CLI 只管理直接启动的 Python Runtime，须等待并回收它后才接受 Response 或发布 Manifest；Runtime 则须在写 Response 前回收自己显式创建的私有 worker。PACK 留下的未等待线程或进程不受支持，CLI 不提供任意后代清理、sandbox、timeout、取消或信号转发，除非新的产品需求先定义相应跨平台行为。
