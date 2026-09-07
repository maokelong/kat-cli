# 公共文本 FtraceProvider 与独立来源发现

关联：[Issue #269](https://github.com/maokelong/kat-cli/issues/269)、[PR #270](https://github.com/maokelong/kat-cli/pull/270)、[ADR-0081](../adr/0081-platform-owns-public-provider-knowledge.md)。本文记录已确认的目标与交付规格；本规格作为 PR #270 的实现与验收依据，实际验证证据记录于关联 Issue 与 PR。

## 问题与最小切片

PR #270 已将文本 FtraceProvider 的实现上移到 `kat.dataprovider.ftrace`，但可发现声明与来源 guide 仍依附于 mem-pack。其他领域虽然可以直接 import 公共类，仍缺少无需选择 PACK 的来源知识入口。

首版把文本 FtraceProvider 的实现、`@kat.provider` 声明与公共来源 guide 一起交给平台，提供独立公共 inspection，并将 mem-pack 迁移为直接消费者。一个没有 `datasources/` 和 Provider guide 的新 PACK，也应能完成公共来源发现、读取使用合同、构造 Provider 和查询的完整链路。

## 作者接口与所有权

公共导入路径沿用 PR #270：

```python
from pathlib import Path

from kat.dataprovider.ftrace import FtraceProvider

provider = FtraceProvider(
    source=Path(trace_path),
    clock_domain=clock_domain,
    workspace_root=ctx.datasource_root,
)
result = provider.query("SELECT * FROM text_ftrace_header")
```

公共类自身使用现有 `@kat.provider(name=..., description=..., guide=...)` 声明，名称为 `ftrace-text`。装饰器继续只附加元数据，不改变类的构造与调用，不增加注册副作用。PACK 自有 Provider 继续使用同一个装饰器。

构造参数和 `tables`、`decode_report`、`query()` 合同沿用现有文本 FtraceProvider。来源路径、clock domain 和物化根目录仍由调用方明确提供；公共化不改变 Source stem 复用、物化失败处理、原生解码、关系结构或查询语义。

公共 guide 从现有 mem-pack Ftrace 来源合同迁移，涵盖构造与查询方式、实际关系及字段、关系生成条件、时钟语义、物化复用约束和诊断。它随 `kat-workflow` wheel 安装，由 Runtime 从安装包资源读取，不依赖源码 checkout、当前工作目录或某个 PACK 的文件。具体领域分析策略继续属于 Workflow guide。

## Provider inspection

| 命令 | 结果 |
| --- | --- |
| `kat inspect provider` | 公共 Provider 列表，首版只有 `ftrace-text` |
| `kat inspect provider --provider ftrace-text` | 公共声明详情与 guide |
| `kat inspect provider --pack <pack>` | 该 PACK 自有 Provider 列表 |
| `kat inspect provider --pack <pack> --provider <name>` | 该 PACK 自有目标声明详情与 guide |

列表沿用 `providers` 结果和 `name`、`description` 字段；详情沿用 `provider` 结果和 `name`、`description`、`module`、`qualname`、`guide` 字段。公共 FtraceProvider 的 `module` 为 `kat.dataprovider.ftrace`，`qualname` 为 `FtraceProvider`；guide 返回原始 Markdown 正文，不要求作者再定位文件。

`--pack` 选择唯一范围。公共与 PACK 自有 Provider 可以同名，同一范围内名称仍须唯一；两个范围不合并、不覆盖，未找到时不自动转向另一范围。使用公共类的 Workflow 不会让它自动成为该 PACK 的自有声明。`--pack-dir` 只用于 PACK 范围，未指定 `--pack` 时拒绝该参数。

公共 inspection 直接读取平台提供的声明，不先进行 PACK discovery，也不挂载或导入 PACK 代码；无 PACK 或无关 PACK 损坏都不能阻止公共发现。首版可显式列出唯一的公共 FtraceProvider，不引入通用 Provider registry、插件扫描或动态安装入口。inspection 可以加载声明模块，但不得构造 Provider、解码来源、建立来源连接或执行查询。

公共与 PACK inspection 使用各自所有者的 guide 资源根，并沿用声明合法性、名称唯一性、guide 存在且非空有效 UTF-8、公开导入位置可解析和失败时不返回部分列表的约束。公共资源缺失应明确报告平台安装资源问题，不提示用户修复某个 PACK。裸 `kat inspect` 继续只返回 PACK 索引。

## 迁移与交付范围

| 位置 | 所需交付 |
| --- | --- |
| `kat/platform/workflow/api/dataprovider/` | 公共 FtraceProvider、其声明与随包交付的公共 guide |
| `kat/platform/workflow/runtime/` | 区分公共与 PACK inspection 请求、声明读取和 guide 资源归属 |
| `kat/platform/cli/` | 允许不带 `--pack` 的 Provider inspection，并直接路由公共请求 |
| `kat/platform/workflow/pyproject.toml` 与相关构建验证 | 将公共 guide 纳入 wheel，验证安装后可读 |
| `examples/packs/mem-pack/` | Workflow 直接 import 公共类，移除本地 FtraceProvider 声明与重复 guide，更新 README 与测试 |
| `kat/skill/references/` 与领域文档 | 作者需要公共来源能力时直接 inspect 公共 Provider，更新命令说明与所有权表述 |

mem-pack 无其他自有 Provider 时，`kat inspect provider --pack mem-pack` 返回空列表；旧的 `--pack mem-pack --provider ftrace-text` 返回未找到。删除薄声明后不保留兼容别名，也不为维持旧入口复制公共 guide。

公共 Provider 的行为测试归入平台测试，mem-pack 保留真实 Workflow 与 Output 回归。使用现有原生 decoder 和 Python 标准库的包资源读取能力，两个私有 wheel 继续同版本原子装配，不增加彼此的 distribution metadata dependency。普通 Toolkit 导入不应因公共发现能力而预加载所有来源 Provider。

本次不收录 DataFusionProvider、Hitrace 或 Trace Streamer，不改造其他示例 Parser，不增加 Provider 基类、自动来源识别、隐式构造、生命周期托管、guide 合并或跨 PACK 依赖。

## 验收

| 场景 | 可验证结果 |
| --- | --- |
| 无可用 PACK 时执行公共 list/detail | 成功发现 `ftrace-text`，取得公共导入位置和完整 guide |
| 无关 PACK 的 manifest 或 Python 模块损坏 | 公共 inspection 成功，且不导入该 PACK 模块 |
| PACK 自有 Provider 与公共 Provider 同名 | 两个范围各自返回正确声明与 guide，互不遮蔽 |
| 请求当前范围不存在的名称 | 明确未找到，不回退到另一范围 |
| Provider 构造或来源操作被设置为一旦调用即失败 | list/detail 仍能成功，证明 inspection 只读取声明与知识 |
| 公共声明或 guide 缺失、非法 | 本次 inspection 失败，不返回部分列表；PACK 原有校验回归通过 |
| 新 PACK 无 `datasources/`、无 Provider guide | 真实 Workflow 可直接使用公共 FtraceProvider 并发布预期 Output |
| 迁移后的 mem-pack | 自有 Provider 列表为空，旧详情入口失败，Workflow 分析结果不变 |
| 从实际构建的 wheel/Payload 执行，不使用源码路径 | Windows 与 Linux 均能读取公共 guide，并完成文本 Ftrace 查询 smoke |

执行相关 CLI 参数与路由测试、Runtime inspection 进程测试、公共 Provider 行为测试、mem-pack 集成测试、wheel 资源与双 wheel 装配回归；PR 必须记录实际命令、结果和对应提交。本文的验收项是后续实现要求，不代表本次设计讨论已完成这些运行验证。
