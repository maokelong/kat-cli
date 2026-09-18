# Python 依赖管理

仅在用户要求管理第三方库，或已授权的创作任务需要补装依赖时读取。KAT 自带 pip，可直接下载并安装库，无需另装 Python 或 uv。

## 选择当前部署的解释器

按 [公共命令合同](command-reference.md) 确认主机平台和 `<kat-root>`，把下列路径解析为绝对路径。使用与当前 CLI 相邻的 Python，不从 `PATH` 或其他部署选择解释器。

| 平台 | `<bundled-python>` |
|---|---|
| Linux x86_64 | `<kat-root>/scripts/targets/linux-x86_64/python/bin/python3` |
| Windows x86_64 | `<kat-root>/scripts/targets/windows-x86_64/python/python.exe` |

例如，在 Linux shell 中安装：

```bash
"<kat-root>/scripts/targets/linux-x86_64/python/bin/python3" -m pip install "<包名>"
```

在 Windows PowerShell 中安装：

```powershell
& "<kat-root>/scripts/targets/windows-x86_64/python/python.exe" -m pip install "<包名>"
```

以下命令中的 `<bundled-python>` 同样替换为对应解释器的绝对路径；PowerShell 使用上述 `&` 调用方式：

```text
<bundled-python> -m pip install "<包名或包名==版本>"
<bundled-python> -m pip install --upgrade "<包名>"
<bundled-python> -m pip uninstall "<包名>"
<bundled-python> -m pip show "<包名>"
<bundled-python> -m pip check
```

索引、版本约束、本地 wheel 等用法沿用 pip 原生参数，正常安装无需 `--break-system-packages`。库应安装到该解释器自身的 site-packages；不要用 `--user`、`--target` 或 `PYTHONPATH` 绕过它。KAT 以 isolated mode 启动 Python，系统 Python 和用户 site-packages 中的库不会因此可用。

## 共享环境与升级

同一部署的 Runtime 和全部 PACK 共用该环境。允许安装任意第三方库，也允许 pip 更新已有第三方依赖，包括 PyArrow、DataFusion；用户管理由此产生的版本冲突。这不承诺每个库都兼容当前 Python 或操作系统，源码包所需的编译器、系统库等由用户提供。安装目录需有写入权限，下载需有可用网络或包源。

整套 KAT Skills 通过目录替换升级时，新部署使用随包发布的干净 Python 环境，额外库需重新安装；不把旧 site-packages 合并到新版本。

## 验证与交付

pip 使用原生退出码及 stdout/stderr，不返回 KAT Response JSON。保留失败或冲突事实，不把 pip 的成功退出视为 PACK 已经验证。

安装或更新后，用 `pip show` 确认实际版本，运行 `pip check` 检查依赖声明，再用同一解释器的 `-I -B -c "import <模块名>"` 验证 KAT 的隔离导入条件；模块名可能与包名不同。卸载后确认目标包已移除，并检查剩余依赖。创作任务还需完成受影响 Provider/Workflow 的 inspection、PACK 测试及该任务需要的运行验证，按 KAT Response 判定结果。单独管理依赖时，说明尚未验证哪些 PACK 行为。

交付说明变更的包、实际版本及验证结果；出现冲突或失败时指出受影响能力和未完成验证。
