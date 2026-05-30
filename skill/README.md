# 鸿蒙 Trace Codex Skill 包

这是可直接安装给 Codex 使用的 skill 目录。包根目录就是 skill root，包含：

- `SKILL.md`
- `agents/openai.yaml`
- `config/`
- `atomics/`
- `knowledge/`
- `references/`
- `strategies/`
- `bin/windows-x64/htrace.exe`
- `bin/windows-x64/trace_processor_shell.exe`

## Windows 快速安装

在包根目录运行：

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

默认会复制到：

```text
%USERPROFILE%\.codex\skills\harmony-trace-analysis
```

并把包内 `bin/windows-x64` 加入用户 PATH。新开终端后可直接使用 `htrace`。
脚本也会把用户环境变量 `HTRACE_TRACE_PROCESSOR` 设置为包内 `trace_processor_shell.exe`。
安装后请重启 Codex 或开启新的 Codex 会话，让 skill 列表重新发现该目录。

## Codex 调用

安装后，Codex 可以在用户提到 `$harmony-trace-analysis`、鸿蒙 trace、HarmonyOS trace、Perfetto trace、`.htrace` 或 `.pftrace` 分析时调用本 skill。`agents/openai.yaml` 已允许隐式触发，`SKILL.md` 的 description 是主要触发依据。

## 手动使用

如果不安装，也可以在包根目录直接调用：

```powershell
.\bin\windows-x64\htrace.exe profile list --skill-root .
.\bin\windows-x64\htrace.exe run init --out runs --trace sample.htrace --question "冷启动为什么慢" --json
```

分析真实 Perfetto-compatible trace 时，需要设置 trace processor：

```powershell
$env:HTRACE_TRACE_PROCESSOR=".\bin\windows-x64\trace_processor_shell.exe"
```

本包已内置 Windows x64 的 `trace_processor_shell.exe`。使用 skill 时应先检查：

```powershell
.\bin\windows-x64\htrace.exe version
Test-Path .\bin\windows-x64\trace_processor_shell.exe
```

## 验证

```powershell
powershell -ExecutionPolicy Bypass -File .\verify.ps1
```

验证内容：

- `SKILL.md` 存在
- `htrace.exe` 可运行
- `trace_processor_shell.exe` 可运行
- profile 配置可加载
- run 状态命令可创建临时 run

## Linux/macOS

本包内只附带 Windows x64 的 `htrace.exe` 和 `trace_processor_shell.exe`。Linux/macOS 环境可直接使用本 skill 目录，但需要自行提供对应平台的 `htrace` 与 `trace_processor_shell` 可执行文件并放入 PATH，或从项目源码构建后复制到 `bin/`。
