# PACK 使用文档与分析前候选选择

跟踪：[Issue #334](https://github.com/maokelong/kat-cli/issues/334)。

## 问题与范围

使用者需要知道其他团队已交付哪些能力、如何使用，并在执行前选择适用方案。
最小切片复用五个 Skill 和现有 inspection：PACK 自带 README，作者流程维护它，分析入口展示候选后等待选择。
不增加能力市场、自动安装、集中索引、新 CLI 命令、选择数据库或第六个顶层 Skill。

## README 所有权与生成

PACK 根的 `README.md` 随所属 PACK 发布。事实区用唯一的 `<!-- kat:capabilities:start -->` / `<!-- kat:capabilities:end -->` 标记围住，由作者脚本维护；标记外的场景、输入准备、自然语言示例、结果限制和开发复用说明由 owner 与 Skill 维护。
已有 README 没有标记时追加事实区；重复、缺失或顺序错误的标记拒绝写入，不能猜测替换范围。

`kat-author/scripts/pack_readme.py --kat K --pack-dir D [--check]` 读取指定源码 manifest 的 name 作为选择器，以公开 `kat inspect`、knowledge list、Workflow list/detail 取得规范化事实。脚本仅检查源码 `workflows/` 是否存在 Python 文件来决定是否需要 Host；无程序的骨架与纯知识 PACK 不启动 Host。它不解析 Python 声明，也不读取执行后 Guide。Provider/helper 的使用导航由说明正文维护，沿用既有作者文档与 Provider inspection。

事实区包含 PACK 元数据、全部知识入口及调用标志、Workflow 用途与参数（实际 option、negative_option、类型、必填、默认、choices）。它不藏起禁用入口，但明确标记其不可选状态。README 不替代执行前 detail 或历史 Guide。

脚本先完成全部 inspection、渲染与链接检查，再写 README；任一失败保持旧文档。`--check` 只验证事实区同步和本地链接，不写文件，不宣称叙述语义或参数示例已验证。使用标准库编排现有 CLI；Markdown 链接交给已有构建依赖 markdown-it-py，不自建解析器。本地链接必须指向 PACK 内已有文件；外部文档使用 URL，源码私有文件不作为安装后导航。

创建骨架直接生成空事实区与明确的无能力说明；完成能力后，kat-author/kat-dev-sdk 自动刷新事实区，并按声明、测试和消费者同步正文。单独文档请求不授权运行 Workflow、修改程序或安装依赖。SDK wheel 包含 README，继续排除开发 API 文档；构建检查包内链接，真实 SDK 安装验收执行 --check 并通过公开 knowledge reference 读取安装说明。

## 候选交互

目标未知时，枚举正常发现范围内每个 PACK 的 knowledge 和 Workflow 摘要，不以 PACK 描述或先找到知识为由跳过另一类。按问题筛选全部相关且允许用户选用的入口；数量多时分组完整展示，不静默截断。按需读 README、知识正文和 Workflow detail；不扫描 Provider 或读取执行后 Guide。

候选展示用途、匹配原因、所属 PACK/入口、必需材料、预期结果、限制及 README 入口。缺输入可列为待补充；发现失败报告覆盖不完整；空列表与失败分开。无匹配不自动创作或安装。文档缺失不抹去现有可发现能力。

普通新任务即使唯一候选也等用户选择，推荐/默认选中不等于选择。明确目标或用户明确授权自动选择视为已有选择，不重复询问。只浏览不创建 Session、采纳知识或运行 Workflow；选择后复用既有初始化、采纳与执行流程。已有目标的恢复和只读交付不重复全量选择；已选方案内的查询与程序子调用不逐步确认，新增实质不同方向再选。

保留 `user-invocable: false` 的不可展示为调用选项和 `disable-model-invocation: true` 的不可由模型自动选用语义；后者可由用户主动选择。作者不再强制两者同时禁用，而按 owner 的发布意图填写；存量显式禁用不自动修改。

## 验证

脚本测试覆盖空/纯知识/混合、增删与参数变更、幂等、正文保留、坏标记、Markdown 本地链接、inspection 失败不写入和 --check 不写入。使用真实隔离 CLI/Host 验证公开 inspection、已有 PACK 生成及 SDK 安装后读取与事实同步。现有装配与构建测试验证 README 交付且 API 文档仍排除。

Skill 静态验证只检查可安装性；另用独立 AI 的现实问题与隔离材料验证候选选择、唯一候选等待、已有选择、缺输入/失败/调用标志/恢复边界，记录实际观察，不用文案匹配测试替代行为验证。

## 已执行证据（2026-11-22）

- Windows x64、Python 3.14.6：`python -I -B -X utf8 -m unittest discover -s build/tests` 共 104 项，99 通过、5 项因既有平台输入缺失跳过；其中 README 脚本 12 项覆盖事实更新、失败不写入及 LF/CRLF。
- 四个修改后的 Skill 均通过 `quick_validate.py`；SDK 与五个实际示例 PACK 经真实 CLI/Host 的 `--check` 全部通过。无 Host 的独立部署也完成真实空骨架和纯知识 PACK 的同步检查。
- `build/build_sdk_wheel.py` 实际构建 `kat_sdk-0.1.1-py3-none-any.whl`；安装到隔离 Host 后，公开 README reference 与安装文件原文字节一致，事实区检查通过，API 文档仍排除在 wheel 外。
- 两个独立 AI 执行者共完成九个行为场景：多候选等待、选择后执行并保存、唯一候选等待、精确指定直接执行、已有 PACK 文档生成、仅 Session ID 只读恢复、发现失败与缺输入、无匹配、明确授权自动选择。观察到内部入口未展示、手动入口不被自动推荐；浏览阶段没有 Session 或 Run。
- 混合任务实际形成 Session `01a20219-0438-79c2-b2d3-61e11d799ba6`、Run `01a20219-7bdf-7be1-bd48-0c65a9c2efcd`；维护者另经公开 query 核对三项观测、两项超标、最大 22 毫秒；独立恢复只调用 analysis show，revision 保持 4。
- 原始本地日志与验证摘要留在工作区 `target/issue334-*`，不作为交付源码。已将文档检查接入 Full CI；本次未运行 GitHub 双平台 Full CI、原生 Datasource 完整安装升级验收或 Release Payload 验收，不将 GNU CLI 与隔离 Host 的文档验证描述为这些门禁通过。
