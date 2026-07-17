# KAT 双平台 Payload 与 Skill 装配设计

Issue：#143。本文只补充 accepted ADR 尚未固定的本切片构建输入和验证 seam；产品边界仍以 ADR 为准。

## 目标与非目标

交付同一份 Workflow Host wheel、Linux x86_64 与 Windows x86_64 两个完整黑盒 Payload，以及只做标准路径映射的原子 Skill Assembly。生成的 wheel、Python Host、Payload、assembly 和归档均进入 `target/` 或 `dist/`，不提交版本库。

本切片不删除旧产品面，不引入 `dist` 发布生命周期，不发布 Python distribution，不增加通用 launcher、系统 Python fallback、运行时下载或其他平台；这些边界分别属于 #144 或已被 ADR 排除。

## 锁定输入与构建 DAG

`build/runtime-inputs.json` 固定 CPython 3.14.6 standard-GIL、PBS 20260623、uv 0.11.28、两平台 archive SHA-256，以及 Microsoft VC143 CRT Redist VSIX。两份 requirements lock 固定完整平台 wheel 闭包的版本和 SHA-256，并强制 binary-only/hash install。PEP 517 backend 固定为 setuptools 80.9.0，Rust 固定为 1.95.0，Cargo 始终使用 `--locked`。

构建顺序只有一条：

```text
一次 PEP 517 Workflow Host wheel + SHA-256
                  │
        ┌─────────┴─────────┐
 Linux glibc 2.28 Builder   Windows MSVC Builder
        └─────────┬─────────┘
             Skill Assembly
```

两个 Builder 必须校验并安装同一个 `kat_workflow-0.1.0-py3-none-any.whl`，不得从源码手工复制 package。Linux Builder 在固定 manylinux_2_28 环境中检查全部 ELF 的架构和 GLIBC symbol baseline；Windows Builder 从最终 `.exe`、`.pyd`、`.dll` 递归导入关系及锁定 Microsoft 可再分发集合计算 app-local DLL 闭包，不维护固定 DLL 名单。

Assembly 只映射 `kat/skill`、`kat/packs` 与两个完整 Payload；它不检查 Payload 内的 wheel、Python、site-packages 或 DLL。Bundled PACK 只复制一次到 `assets/packs`。

## 验证

构建单测覆盖锁文件、hash、归档防穿越、payload root、ELF/PE 闭包与 Assembly 原子失败。最终 smoke 将整份 Skill 移到带空格的任意目录，从无关 cwd 执行，污染 `PATH` 与 `PYTHON*`，完成 Trace Streamer Import → Run → bounded Query，并对两个 Bundled PACK 执行 `kat test`；前后比较 Skill 定义与 PACK 源码摘要。

Linux smoke 在 glibc 2.28、无网络容器中执行。GitHub Windows Server 只作为 Builder 闭包 smoke；Windows 10/11 x86_64 Client 的最终证据必须来自 `ProductType=1` 主机，不能由 Server 结果冒充。
