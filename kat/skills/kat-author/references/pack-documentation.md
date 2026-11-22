# PACK 使用文档

在创建或更新 PACK 能力后自动执行本流程；也支持用户单独要求为已有 PACK 生成、补齐或更新 README。单独文档请求只授权文档维护，不修改程序、不运行 Workflow、不安装依赖。脚本与资源从本文件解析，PACK 使用用户已明确的精确目录。

## 编写说明

先用共享合同选定同套部署的 CLI，按实际对象 inspection 核对已交付能力；源码、测试与真实消费者用于解释场景和限制，不用源码扫描伪造 inspection。已有 README 保留有效的作者内容，只改本次能力变化相关部分。没有 README 时创建 PACK 根的 `README.md`，包含：

- 定位、适用范围与维护方。
- 可以直接提出的自然语言问题，以及适用的知识入口/Workflow。
- 必需材料、格式、输入准备与执行前提。
- 预期结果、不能据此判断的事项及证据限制。
- Provider/普通函数的开发者复用导航（确实存在才写），链接已有文档，不复制 API 合同。

没有事实依据时明确未知，不编造阈值、能力或示例结果；骨架明确尚无业务能力。知识和 Workflow 都要覆盖，Provider/helper 不冒充用户可直接运行的分析入口。先选择再执行的交互由 kat-analyze 负责，README 示例不代替用户选择。

文档内相对链接应指向随 PACK 交付的已有文件；开发仓库资料用明确的网页链接，避免安装后失效。README 是使用导航；执行参数以 Workflow detail 为准，执行后解释以 Run Guide 为准，SDK Python API 仍查经审核的作者文档。

## 同步与检查

脚本使用 Python 3.11+；链接校验需要该解释器已具备 `markdown-it-py==4.2.0`。它不使用私有 Runtime 的 Python 代替文档工具环境。缺失依赖时报告精确缺项，单独文档任务不自动安装。

```text
python <author-root>/scripts/pack_readme.py --kat <同套CLI绝对路径> --pack-dir <PACK绝对目录>
python <author-root>/scripts/pack_readme.py --kat <同套CLI绝对路径> --pack-dir <PACK绝对目录> --check
```

脚本只更新 `<!-- kat:capabilities:start -->` 与 `<!-- kat:capabilities:end -->` 之间的事实；没有标记时追加，标记残缺或重复则停止。元数据、知识入口与调用标志、Workflow 用途和全部参数来自公开 inspection。只读浏览不会采纳知识或执行 Workflow。无 Python Workflow 文件的 PACK 不需要 Host；有程序时需要完整部署及可导入依赖，失败不写残缺事实。SDK 源码与已安装 SDK 同名时按既有 discovery 报冲突；使用明确的隔离部署维护源码，不能隐藏或卸载用户的已安装 SDK 来绕过。

同步后阅读完整 README，更新与变更有关的场景、输入、示例和限制，再运行 `--check`。它只验证事实区同步及本地链接有效，不证明叙述语义或示例已运行；不为文档检查自动执行程序。

## 交付

说明 README 路径、覆盖的实际能力、更新及 `--check` 结果、已做的示例验证和限制。README 必须进入 PACK 发布包；SDK wheel 也携带它，但源码 API 文档仍不入 wheel。只读安装目录先请求 owner 的源码位置，不能把修改安装副本当作正式交付。
