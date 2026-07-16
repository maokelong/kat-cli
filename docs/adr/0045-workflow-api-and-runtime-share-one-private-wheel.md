---
status: accepted
---

# Workflow API 与 Runtime 共用一个私有 wheel

> ADR-0047 进一步明确：该 wheel 提供顶层 `kat` Pack Authoring API，但不提供静态 `kat.pack`；`kat.pack` 由 Runtime 为当前 PACK 动态挂载。本文其余构建与发布决定继续有效。

`kat/platform/workflow` 是 Pack Authoring API 与 Workflow Runtime 的单一源码构建单元。构建期使用成熟 PEP 517 backend 生成一个纯 Python Workflow Host wheel；它同时包含 PACK 使用的 `kat` package，以及 KAT CLI 以 `python -I -B -X utf8 -u -m <private-runtime-module>` 启动的私有 Runtime module。源码职责目录、Python import namespace 与 wheel 内文件布局由标准 backend 映射，不要求三者目录同构，也不通过脚本手工复制 site-packages。

Linux 与 Windows Platform Payload Builder 使用固定版本的 `uv`，把同一个 KAT wheel 和各自锁定的第三方预编译 wheels 安装进 `python-build-standalone` Host。Skill Assembly Adapter 只复制已经完整的 Platform Payload，不读取 wheel metadata、不枚举 Python packages，也不再次装配 API 或 Runtime。这使 Source view 到两个 Host 之间只有一个标准、可独立验证的构建 seam，同时把平台原生依赖继续留给各自 Builder。

该 wheel 只是 KAT 原子发布过程中的私有中间产物：不发布到 PyPI 或其他包索引，不供系统 Python 安装，不加入用户 `PATH`，不提供 console script，也不让 distribution name 或 version 成为产品 Interface。KAT 不恢复 `kat-python-sdk` 与 `kat-python-runtime` 两个 distribution，不为二者建立依赖关系、兼容层或独立升级路径；Skill、CLI、Host、API、Runtime 与 PACK 仍按同一个 KAT 版本发布和验证。
