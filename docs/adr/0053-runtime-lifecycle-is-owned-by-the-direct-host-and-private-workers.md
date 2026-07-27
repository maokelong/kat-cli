---
status: accepted
---

# Runtime 生命周期由直接 Host 与私有 worker 分层拥有

ADR-0010 最初要求 CLI 通过 Unix process group 或 Windows Job Object 等待 Runtime
及其后代。PR #151 的实际生产路径没有调用者依赖“直接 Python Runtime 已退出，但
未等待的普通后台进程仍属于本次操作”；为该假设增加跨平台进程容器、无限等待与
平台 `unsafe` 不符合当前最小交付。

CLI 只拥有并管理它直接启动的 Python Runtime：

- 使用标准子进程能力启动、等待并回收直接 Runtime。
- 直接 Runtime 退出前，不读取或接受 Runtime Response，也不发布 Run Manifest。
- 等待失败时尽力终止并回收直接 Runtime，然后令当前操作失败。

Workflow Runtime 拥有自己显式创建的私有 worker。当前唯一生产 worker 是 PACK
inspection 使用的 `multiprocessing spawn` worker；正常、失败和异常路径都必须在
Runtime 写 Response 前完成 `join`，仍存活时先 `terminate`，最后关闭进程资源。
Workflow 本身同步执行并物化 Outputs；Workflow 返回即声明本次受支持工作已经完成。

PACK 自行留下未等待的线程或进程不属于受支持的 Pack Authoring 行为。CLI 不发现、
等待或清理任意 PACK 后代，也不提供进程 sandbox、timeout、取消或信号转发合同。
若以后需要管理不协作的普通后台进程，应先由独立产品需求定义这些行为和真实
Linux/Windows 验收，再选择相应 OS primitive。

本决定部分取代 ADR-0010 关于 OS 进程容器、全部后代及“只等待 leader 不足”的
生命周期决定；文件 IPC、退出码、严格 Runtime Response、Operation log 与
Run Manifest 发布门禁继续有效。
