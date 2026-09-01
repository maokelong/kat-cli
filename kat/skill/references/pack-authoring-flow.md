# PACK 创作与维护流程

## 1. 先定位 PACK，再按对象检查知识

用户指定已有 PACK 时，先调用裸 `kat inspect` 和需要时的精确 `--pack-dir`，从 manifest 概要定位它。裸 inspection 不加载 PACK Python，也不包含 Workflow 或 Provider 声明。

根据开发目标分别调用：

- `kat inspect workflow --pack <名称>`：了解 PACK 暴露的 Workflow；选中一个后追加 `--workflow <名称>` 读取参数合同和 analysis guide。
- `kat inspect provider --pack <名称>`：了解 PACK 已有的 Provider；选中一个后追加 `--provider <名称>` 读取代码位置和数据库、SQL、Schema 或接入 guide。

Workflow 和 Provider 是两个独立知识入口。开发 Workflow 时只在需要复用或修改数据能力时 inspect Provider；分析问题时不 inspect Provider。inspection 失败时按 Diagnostic 停止，不用静态源码扫描伪造公开声明。

新建 PACK 没有 KAT 任务契约内的 Issue 或 SDD 前置门；执行所在仓库的协作规范仍独立适用。

## 2. 声明可发现知识

Workflow 与 Provider 都用显式元数据供 Agent 索引：

```python
import kat


@kat.workflow(
    name="memory-summary",
    description="汇总进程内存变化并定位异常区间。",
    parameters={"input_path": "待分析输入的路径。"},
    guide="workflows/memory-summary.md",
)
def memory_summary(ctx: kat.Context, *, input_path: str):
    ...


@kat.provider(
    name="postgresql",
    description="查询远端 PostgreSQL 并返回可物化表。",
    guide="providers/postgresql.md",
)
class PostgreSQLProvider:
    ...
```

- `name` 是稳定索引；`description` 是列表筛选所需的明确用途。两者都必须显式声明，不使用 `title`，也不从 docstring 推导 description。
- Workflow `parameters` 是运行输入合同；`guide` 可选，内容指导结果解释、分析发散与下一步方向。
- Provider decorator 只附加 `name`、`description`、`guide` 元数据。它不要求基类、Protocol、注册表、固定方法或生命周期；Provider 类可以按数据源需要定义 `decode`、`query` 或其他能力，Workflow 显式调用它们。
- Provider `guide` 必填，用于说明数据库、可用 SQL、表与关系、Schema、解析或物化方式。它是作者知识，不是分析策略。
- Provider detail 的 `module` 与 `qualname` 由声明类机械取得，不能在 decorator 中覆盖。

## 3. 组织和引用 guide

一个 PACK 的作者知识统一放在顶层 `knowledge/`：Workflow guide 位于
`knowledge/workflows/`，Provider guide 位于 `knowledge/providers/`。框架不限制
Markdown 的章节和写法。

Decorator 中的 `guide` 是相对 `knowledge/` 的路径，例如 `providers/postgresql.md`。它必须指向 `knowledge/` 内已有、非空、有效 UTF-8 的普通 `.md` 文件；绝对路径、路径穿越和解析后逃逸 `knowledge/` 都会被拒绝。

List inspection 会校验全部声明及 guide，但只返回 `name`、`description`，不会把所有 Markdown 放进上下文。选中 detail 后，Runtime 才把对应文件按原样读成 Response 的 `guide` 字符串；Agent 直接使用该字段，不自行组合路径或实现 include。Workflow 未声明 guide 时 detail 返回 `null`；Provider guide 始终返回字符串。

## 4. Provider inspection 的执行边界

Provider inspection 会递归导入所选 PACK 顶层 `datasources/` 下的普通 Python 模块，并收集由各模块自身定义且经过 `@kat.provider` 装饰的类。一个模块可以声明零个、一个或多个 Provider；从其他模块 import 的声明不会重复计数。

因此 `datasources/` 必须 import-safe：模块导入可以定义类和纯元数据，但不应建立数据库连接、读取凭据、解析输入、启动进程或执行查询。KAT inspection 也不会实例化 Provider 或调用其业务方法。任一导入错误、非法声明、重名或 guide 错误会使本次 inspection 原子失败，不返回部分 Provider 列表。

这是运行时 Python 发现，不是静态 AST 扫描。只有 Provider inspection 扫描完整 `datasources/`；Workflow inspection 不触发它。

## 5. 实施并验证已授权变更

理解、检查和测试默认只读。只有用户明确要求创建、修改或修复时才写入指定 PACK，并保持最小切片。编写 Provider 时优先复用 KAT 已公开的数据表、物化和查询能力；具体用法以已选 Provider guide、公共库接口和随 Skill 发布的
[可执行 Data Provider reference PACK](examples/dataprovider-pack/README.md) 为准，不发明框架约束。

写入后按变更面验证：

1. 重新执行对应 Workflow 或 Provider list inspection，确认所有声明与 guide 都能完整校验。
2. 对新增或修改的声明执行 detail inspection，核对精确公开字段和 guide 内容。
3. 运行适用的 `kat test --pack-dir ...`；成功 `result.summary` 是测试结论，失败时使用 Response、报告和日志定位。
4. 交付变更摘要、受影响文件、inspection/test 证据和仍存限制。

“诊断失败”本身不授权修复。无法在已有授权和事实下继续时，按 [result-contract.md](result-contract.md) 交付最小下一步。
