---
name: kat
description: 使用随附的 KAT PACK 分析本地 Hitrace、Trace Streamer SQLite、KAT Dataset 或既有 Run，并检查和测试 KAT PACK。适用于平台性能、整机和应用调度分析；不用于普通 SQLite 查询或其他 trace 格式。
---

# KAT

KAT 是唯一面向用户的产品入口。用户只需描述分析目标；在内部编排 `import`、`inspect`、`run`、`query` 和 `test`，不要要求用户选择这些内部步骤。

## 每次操作前选择平台载荷

先把本 `SKILL.md` 的父目录解析为绝对 `<skill-root>`，再重新检查当前主机：

- Linux：读取 `uname -m` 与 `getconf GNU_LIBC_VERSION`。仅支持 glibc 2.28 或更高版本的 x86_64，执行 `<skill-root>/scripts/targets/linux-x86_64/kat`。
- Windows：读取原生架构、系统版本与 `Win32_OperatingSystem.ProductType`。仅支持 Windows 10/11 x86_64 客户端（`ProductType=1`），执行 `<skill-root>/scripts/targets/windows-x86_64/kat.exe`；明确拒绝 Windows Server、Windows 7/8.1。

拒绝其他系统、架构、libc 或版本；所选文件缺失时也拒绝，Linux 还需确认可执行位。始终使用上述绝对路径，不持久化选择，不搜索 `PATH`，不回退到系统 `kat` 或 Python，不调用载荷内的私有 Python launcher，也不传入 Skill root 参数。

## 形成分析结果

1. 用户提供既有 Dataset 时直接 `inspect --dataset`，不要重新导入。用户提供原始输入时必须先明确其类型：Hitrace 使用 `hitrace`，Trace Streamer SQLite 使用 `trace-streamer`；来源不明确时先询问，不自动探测。
2. 导入后只使用 KAT Response 返回的 canonical Dataset path。检查 Dataset 的实际表和 schema；失败时停止。
3. 使用无目标 `inspect` 取得可发现 PACK，不扫描 manifest、PACK 源码或 Dataset 文件来替代 KAT 的结果。
4. 只对可能匹配的 PACK 执行 `inspect --pack`，根据 Workflow 描述、参数和 Required tables 排除不兼容项。只剩一个明确匹配项时自动选择；存在多个实质不同的分析时再询问用户。
5. 按 inspection 返回的 Workflow Interface 生成参数，并把全部 Workflow 参数放在 `--` 后。保存成功 Response 中的 Run ID。
6. 对已发布 Run 执行少量、有界、只读的 Query；取得足够事实后停止查询，由模型形成 Analysis Result。不得读取 Run 内部文件，不得制造未发布结果，也不得把模型结论写回 Runtime、Run Manifest 或 Dataset。

命令形状如下，其中 `<kat>` 必须替换为当前平台载荷的绝对路径：

```text
<kat> import [--dataset <directory>] [--overwrite-dataset] hitrace --trace <file>
<kat> import [--dataset <directory>] [--overwrite-dataset] trace-streamer --database <file>
<kat> inspect --dataset <dataset-directory>
<kat> inspect [--pack-dir <exact-pack-directory> ...]
<kat> inspect --pack <pack-name> [--pack-dir <exact-pack-directory> ...]
<kat> run --pack <pack-name> --workflow <workflow-name> [--dataset <dataset-directory>] [--pack-dir <exact-pack-directory> ...] [-- <workflow-argument> ...]
<kat> query --run <run-id> --sql <single-read-only-statement>
```

每个 `--pack-dir` 都必须直接包含该 PACK 的 `pack.toml`。KAT stdout 是一个 compact JSON Response；只根据 `status=success` 的 result 连接下一步。失败时向用户报告 Diagnostic、可执行帮助与 Operation log 路径，不把失败或未发布候选描述为成功。

## 验证 PACK

先 inspection 生产 Interface，再在同一真实 Runtime 中运行测试：

```text
<kat> inspect --pack <pack-name> [--pack-dir <exact-pack-directory> ...]
<kat> test --pack <pack-name> [--pack-dir <exact-pack-directory> ...]
<kat> test --pack <pack-name> [--pack-dir <exact-pack-directory> ...] --test <node-id> [--test <node-id> ...]
```

只复用 KAT 报告的 pytest node ID，不转发任意 pytest 参数。除非用户明确要求编辑 External PACK，否则把 Skill、两个 Platform Payload 和所有 PACK 源码视为只读输入；可写 Dataset、Run、日志与测试报告由 KAT 写入平台 Data Home 或用户明确选择的 Dataset 目录。
