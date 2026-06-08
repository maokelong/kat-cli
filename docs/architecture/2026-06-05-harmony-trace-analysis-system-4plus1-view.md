# Harmony Trace Analysis System 4+1 架构视图

日期：2026-06-05

来源：`doc/architecture/2026-06-05-harmony-trace-analysis-system-design.md`

用途：

- 为开发、评审、沟通和验收提供可渲染的架构图输入。
- 与 compact 规格保持一致；如有冲突，以 compact 规格中的“硬约束 / 禁止 / 必须”为准。

## 1. 整体分析流程图

```plantuml
@startuml
title 整体分析流程 - 单 trace 闭环与可选 Playbook 固化
skinparam shadowing false
autonumber

actor "User / UI / CLI" as User
participant "Codex Skill / LLM\n流程编排与语义解释" as Skill
database "Analysis Workspace Files\nChecklist / context / evidence-map\nartifacts / receipts" as Workspace
participant "htrace-service api" as Api
participant "capability gate\n配置加载 / 权限 / guard / refs" as Capability
participant "execution\natomic / composite / transform" as Execution
participant "trace adapter\nhtrace-service 内部 adapter" as TraceAdapter
participant "evidence / artifact store\nsummary / bounded read / receipt" as Store
participant "playbook\nvalidate / publish / evaluate" as Playbook

box "htrace-parser service" #LightBlue
  participant "htrace-parser" as Parser
  database "Trace Dataset" as Dataset
end box

User -> Skill : 提交 trace 分析请求\n问题描述 / trace 输入 / 约束
Skill -> Workspace : create_or_resume_workspace()
Skill -> Workspace : 写入 inputs.json / parser.json / analysis-context.json
Skill -> Skill : 识别用户问题是否明确\nroute_domain_or_problem

alt 问题明确
  Skill -> Api : resolve_topdown_strategy(problem_id)
  Api -> Capability : 从 config 加载并校验 problem_topdown
  Capability --> Api : topdown strategy contract
  Api --> Skill : problem_topdown contract
else 问题不明确
  Skill -> Skill : 根据用户身份和请求上下文\n确认分析领域
  Skill -> Api : resolve_topdown_strategy(domain_id)
  Api -> Capability : 从 config 加载并校验 domain_topdown
  Capability --> Api : topdown strategy contract
  Api --> Skill : domain_topdown contract
end

Skill -> Workspace : 根据 topdown strategy contract\n渲染 Checklist.md
Skill -> Api : register dataset / inspect / capability-probe
Api -> Capability : 校验 dataset 与 profile / service limits
Capability -> Execution : validated probe contract
Execution -> TraceAdapter : dataset inspect / capability probe
TraceAdapter -> Parser : 参数化查询 / schema inspect
Parser -> Dataset : 内部查询
Dataset --> Parser : rows / schema metadata
Parser --> TraceAdapter : parser response
TraceAdapter --> Execution : normalized response
Execution -> Store : 写 probe evidence / summary / receipt
Store --> Api : refs
Api --> Skill : dataset_ref / capability refs / receipt refs
Skill -> Workspace : 更新 analysis-context.json\nevidence-map.json / parser.json

loop Checklist step 闭环
  Skill -> Workspace : select_next_checklist_step\nstep -> in_progress
  Skill -> Api : execute atomic / composite\n或请求 bounded read / artifact validate
  Api -> Capability : validate capability / inputs / refs / guards

  alt capability blocked / config invalid / input missing
    Capability --> Api : blocked / error\nactor_required / recommended_action
    Api --> Skill : blocked receipt / diagnostic ref
    Skill -> Workspace : step -> blocked\n记录 cannot_conclude / next_actions
  else capability passed
    Capability -> Execution : validated execution contract
    Execution -> TraceAdapter : 执行 atomic / composite\ncomposite 内含 transform steps
    TraceAdapter -> Parser : 受控参数化查询
    Parser -> Dataset : 内部查询
    Dataset --> Parser : rows
    Parser --> TraceAdapter : parser result
    TraceAdapter --> Execution : normalized rows
    Execution -> Store : 写 AtomicEvidence / TransformedEvidence\nSummary / ExecutionReceipt
    Store --> Api : evidence_ref / summary_ref / receipt_ref
    Api --> Skill : ok / partial + refs
    Skill -> Workspace : step -> done / revisit\n更新 evidence-map.json
    Skill -> Api : 读取 summary；必要时 bounded read
    Api -> Store : read summary / bounded evidence
    Store --> Api : read_result + read_receipt
    Api --> Skill : summary / bounded rows / receipt_ref
  end

end

Skill -> Skill : 汇总 Facts / Inferences / Uncertainties\n判断是否需要 deep analysis

opt deep analysis
  Skill -> Workspace : 创建 user gate 或 deep steps
  User -> Skill : approve / skip

  loop deep analysis step 闭环
    Skill -> Workspace : select_next_deep_step\nstep -> in_progress
    Skill -> Api : 执行 deep composite / atomics
    Api -> Capability : 校验 deep capability / refs / guards
    Capability -> Execution : validated deep contract
    Execution -> TraceAdapter : 深入查询与 transform
    TraceAdapter -> Parser : 参数化查询
    Parser -> Dataset : 内部查询
    Dataset --> Parser : rows
    Parser --> TraceAdapter : parser result
    TraceAdapter --> Execution : normalized rows
    Execution -> Store : 写 deep evidence / summary / receipt
    Store --> Api : deep evidence / summary / receipt refs
    Api --> Skill : deep refs
    Skill -> Workspace : deep step -> done / revisit / blocked\n写 deep summary refs

    alt LLM 需要参与决策
      Skill -> Workspace : 读取 summaries / bounded read refs
      Skill -> Skill : LLM 辅助判断下一步\n保留 Facts / Inferences / Uncertainties
      Skill -> Workspace : 追加 deep steps / uncertainties / decision artifact refs
    else LLM 不需要参与
      Skill -> Workspace : 按既有 Checklist 继续
    end

    alt 用户需要改道
      User -> Skill : redirect / skip / approve new direction
      Skill -> Workspace : 写 plan-revisions.md\n旧 deep steps skipped/revisit/blocked\n追加新方向 Checklist steps
    else 用户不改道
      Skill -> Workspace : 继续当前 deep analysis plan
    end
  end
end

Skill -> Skill : 生成 problem report / hypothesis verdict draft
Skill -> Api : /artifacts/validate 或 /artifacts\n提交 evidence refs / conclusion draft
Api -> Capability : evidence sufficiency gate\nrequired evidence / counter evidence / truncation

alt evidence gate rejected
  Capability --> Api : rejected / blocked\nINSUFFICIENT_EVIDENCE_FOR_CLAIM
  Api --> Skill : blocked receipt
  Skill -> Workspace : step -> blocked/revisit\n新增补证据 steps 或降低置信度
else evidence gate passed
  Api -> Store : 注册 artifact_index / ArtifactReceipt
  Store --> Api : artifact_ref / receipt_ref
  Api --> Skill : artifact registered
  Skill -> Workspace : 写 final report refs\nChecklist 关键步骤完成
end

opt Phase 2: 从单 trace 分析固化 Playbook
  Skill -> Workspace : 从 Checklist + evidence-map + receipts\n编译 PlaybookVersion draft
  User -> Skill : 确认 scope / preconditions / evaluator rules
  Skill -> Api : playbook validate / publish / evaluate
  Api -> Playbook : validate / publish / evaluate request
  Playbook -> Capability : 校验 playbook refs / guards / evaluator rules
  Capability -> Execution : evaluate 时生成 validated execution contract
  Execution -> TraceAdapter : 执行声明的 atomic / composite
  TraceAdapter -> Parser : 参数化查询
  Parser -> Dataset : 内部查询
  Dataset --> Parser : rows
  Parser --> TraceAdapter : parser result
  TraceAdapter --> Execution : normalized rows
  Execution -> Store : 写 evaluation evidence / receipt
  Playbook -> Workspace : 固化 PlaybookVersion / evaluation refs
  Skill -> Workspace : batch workspace / review workspace\n由 Skill / CLI 维护 lifecycle
end
@enduml
```

说明：

- 这张图表达端到端时序，不替代前面的 4+1 视图。主线仍然是 Skill 编排，service 提供受控能力，htrace-parser service 负责 trace 查询。
- Checklist.md 来自 htrace-service capability gate 返回的 topdown strategy contract：问题明确时解析对应问题的 `problem_topdown`；问题不明确时 Skill 根据用户身份和请求上下文确定分析领域，再请求解析对应领域的 `domain_topdown`。htrace-service 参与 config 加载与策略契约校验，但不负责渲染 Checklist.md 或推进 workflow。
- 每个 Checklist step 都必须经历 `in_progress -> service 调用 -> evidence/summary/receipt 落盘 -> Checklist/evidence-map 更新` 的闭环。
- blocked、deep analysis 和证据充分性 gate 都是流程内的一等分支，不是异常旁路。
- deep analysis 是主要改道点；所有 decision point 都必须支持 redirect / skip / revisit。用户改道必须写 plan-revisions 并追加新方向 Checklist steps，历史 evidence、artifact、receipt 保留。
- deep analysis 本身是循环；每个 deep step 结束后都显式判断 LLM 是否需要参与决策，以及用户是否需要改道。
- Phase 2 的 Playbook 固化从 workspace 中的 Checklist、evidence-map、receipts 和 artifacts 产生；发布和评估仍需 capability gate 校验，evaluate 时由 capability 生成 execution contract。

## 2. Playbook 使用分析时序图

```plantuml
@startuml
title Playbook 使用分析时序 - 简化版
skinparam shadowing false
autonumber

actor "User / Batch Owner" as User
participant "Skill / CLI / LLM\n编排与受控 LLM 决策" as Skill
database "PlaybookVersion\n固化于 workspace" as Playbook
database "Batch Workspace\ncohort / progress / results" as BatchWorkspace
participant "htrace-service\nplaybook evaluate / aggregate" as Service

box "htrace-parser service" #LightBlue
  participant "htrace-parser" as Parser
  database "Trace Dataset(s)" as Dataset
end box

User -> Skill : 选择 PlaybookVersion
User -> Skill : 选择 trace 集合 / cohort
User -> Skill : 确认批量参数
Skill -> Playbook : 读取 Playbook 定义\n查询步骤 / 判定方式 / 聚合策略
Skill -> BatchWorkspace : 创建 batch workspace
Skill -> Service : validate(playbook, cohort)\n包含 batch preflight 输入
Service -> Service : 校验 playbook / cohort\nquota / concurrency / retention / retry budget
Service --> Skill : 可以执行 / 需要调整

alt 需要调整
  Skill --> User : 提示调整 playbook、cohort 或参数
else 可以执行
  Skill -> BatchWorkspace : batch -> running

  loop 遍历 cohort 中的每条 trace
    Skill -> BatchWorkspace : 取下一条 trace_ref
    Skill -> Service : evaluate(playbook_version, trace_ref)

    group 1. Playbook 数据查询 / 特征提取
      Service -> Service : capability -> execution -> trace_adapter\n生成参数化查询 contract
      Service -> Parser : trace adapter 参数化查询
      Parser -> Dataset : 内部查询 trace 数据
      Dataset --> Parser : rows / metadata
      Parser --> Service : parser result
      Service -> Service : transform / feature_extract\n写 evidence / feature refs / receipt
    end

    group 2. TraceVerdict 判定
      alt deterministic Playbook
        Service -> Service : evaluator rules 判定\nhit / partial_hit / miss / unknown / invalid
      else llm_assisted Playbook
        Service --> Skill : 固定 input_bundle\nfeature bundle / evidence refs / prompt schema
        Skill -> Skill : 批量执行声明的 llm_decision\n生成 LLMDecisionArtifact
        Skill -> Service : validate_llm_decision(artifact)
        Service -> Service : 校验后生成 TraceVerdict\nbasis = llm_assisted
      end
    end

    Service --> Skill : TraceVerdict + evidence refs
    Skill -> BatchWorkspace : 保存 BatchTraceResult
  end

  Skill -> Service : aggregate(BatchTraceResult refs)
  Service --> Skill : BatchAggregation
  Skill -> BatchWorkspace : 保存 aggregation / report refs
  opt review workspace 抽样复查
    Skill -> Service : prepare_review_sample(BatchTraceResult refs)
    Service --> Skill : review sample refs
    Skill -> BatchWorkspace : 保存 ReviewWorkspace refs\n不修改原始 verdict
  end
  Skill --> User : 展示批量分析报告
end
@enduml
```

说明：

- 这张图把 Playbook 应用到 trace 的过程简化成两步：先按 Playbook 查询并提取特征，再生成 `TraceVerdict`。
- 简化图中的 Service 内部执行仍必须经过 capability / execution / trace adapter，不能由 playbook 模块直接调用 htrace-parser。
- 批量进入 running 前必须完成 cohort / quota / concurrency / retention / retry budget preflight；该步骤只校验前置，不创建或推进 batch lifecycle。
- deterministic Playbook 的第二步由 evaluator rules 直接判定，不调用 LLM。
- llm_assisted Playbook 的第二步由 `Skill / CLI / LLM` 基于固定 input bundle 批量执行声明的 `llm_decision`，输出仍需回到 htrace-service 校验后才能形成 `TraceVerdict`。
- 批量分析只是把同一个单 trace evaluation 对 cohort 中多条 trace 循环执行，最后聚合 `BatchTraceResult` 并生成报告；可选 ReviewWorkspace 只能引用原始 batch result 和新证据，不修改原始 `TraceVerdict`。

## 3. 逻辑视图

```plantuml
@startuml
title 4+1 逻辑视图 - 基于整体分析与 Playbook 使用流程
top to bottom direction
skinparam componentStyle rectangle
skinparam packageStyle rectangle
skinparam shadowing false

actor "User / UI / CLI" as User
component "Skill / CLI / LLM\n流程编排、语义解释、受控 llm_decision" as Skill

folder "Workspace Files\nanalysis-runs\nPlaybookVersion\nbatch workspace / review workspace" as Workspace
folder "Config Files\nprofiles / guards / atomics / composites\nparser defaults / service limits" as ConfigFiles

package "Rust htrace-service\n无 workflow 状态的 evidence/capability service" as Service {
  component "api\n能力入口 / request_id / error envelope" as Api
  component "capability gate\n配置加载 / profile allowlist / guard / refs" as Capability
  component "execution\natomic / composite / transform\nfeature_extract / receipt" as Execution
  component "trace adapter\nschema inspect / parser adapter" as TraceAdapter
  component "evidence / artifact store\nsummary / bounded read\nartifact / receipt / index" as Store
  component "playbook module\nvalidate / publish / evaluate / aggregate" as Playbook
}

package "htrace-parser service\n查询服务边界" as ParserService {
  component "htrace-parser\n只负责查询" as Parser
  database "Trace Dataset(s)\ntrace file / trace DB" as Dataset
  Parser --> Dataset : 内部查询
}

User --> Skill : 提交分析请求 / 选择 Playbook / 审阅报告
Skill --> Workspace : 读写 Checklist / evidence-map\nPlaybookVersion / BatchTraceResult
Skill --> Api : 调用分析与 Playbook 能力

Api --> Capability : validate / resolve request
Capability --> Execution : validated execution contract
Execution --> TraceAdapter : 执行查询与 transform
TraceAdapter --> Parser : 参数化查询请求
Execution --> Store : 写 evidence / summary / receipt
Api --> Store : bounded read / artifact validate / artifact register
Store --> Workspace : 持久化 evidence / artifact / receipt refs

Api --> Playbook : validate / publish / evaluate / aggregate
Playbook --> Capability : 校验 playbook refs / evaluator rules / guards
Playbook --> Workspace : 固化 PlaybookVersion / evaluation refs
Capability ..> ConfigFiles : 唯一配置加载入口

Skill --> Skill : deep decision / llm_decision\n仅基于固定 evidence bundle

note right of Skill
Skill / CLI / LLM 维护 workflow 与 batch lifecycle；
LLM 只参与 deep decision 或声明的 llm_decision。
end note

note bottom of Service
htrace-service 执行确定性能力、校验、证据索引、
摘要和受限读取；不维护 Checklist / batch lifecycle。
end note

legend right
  |= 线型 |= 含义 |
  | 实线箭头 | 运行时调用 / 数据流 |
  | 虚线箭头 | 配置读取 |
endlegend
@enduml
```

说明：

- 逻辑视图以第 7、8 节的两条已确认流程为主线：交互式整体分析闭环，以及 Playbook 的“查询/特征提取 -> TraceVerdict 判定”。
- Config Files 位于 service 外部，只有 capability gate 加载和解析；execution、trace adapter、playbook 不直接读取 config。
- PlaybookVersion、BatchTraceResult、review findings 都固化在 Workspace Files 中，不挂在 Config Files 下。
- htrace-parser service 包含 htrace-parser 和 Trace Dataset，htrace-service 只通过 trace adapter 发送参数化查询。

## 4. 开发视图

```plantuml
@startuml
title 4+1 开发视图 - 模块、配置与工作区组织
left to right direction
skinparam componentStyle rectangle
skinparam packageStyle rectangle
skinparam shadowing false

package "htrace-service / src" as ServiceSrc {
  component "api" as DevApi
  component "capability\nConfigRegistry / CapabilityRegistry\nguards / refs / profile allowlist" as DevCapability
  component "execution\natomic / composite / transform\nfeature_extract" as DevExecution
  component "trace_adapter\nparser client / schema inspect" as DevTrace
  component "evidence_artifact_store\nindex / summary / bounded read\nartifact / receipts" as DevStore
  component "playbook\nvalidate / publish / evaluate / aggregate" as DevPlaybook
}

folder "config files" as ConfigDir {
  component "service.yaml" as ServiceYaml
  component "profiles/*.yaml" as Profiles
  component "guards/*.yaml" as Guards
  component "atomics/**/*.yaml" as Atomics
  component "composites/**/*.yaml" as Composites
  component "transforms/operators.yaml" as TransformOps
  component "topdown / strategies / templates / knowledge" as AnalysisConfigs
}

folder "workspace files" as Workspace {
  component "analysis-runs/{analysis_id}\nChecklist / context / evidence-map" as AnalysisRun
  component "evidence / summaries / artifacts / receipts" as EvidenceFiles
  component "playbooks/{playbook_id}/{version}\nPlaybookVersion / evaluator rules" as PlaybookVersions
  component "batch-runs/{batch_id}\ncohort / progress / BatchTraceResult\naggregation / report refs" as BatchRuns
}

package "htrace-parser service" as ParserSvc {
  component "htrace-parser" as Parser
  database "Trace Dataset(s)" as Dataset
}

DevApi --> DevCapability
DevCapability --> DevExecution : validated contract
DevExecution --> DevTrace
DevTrace --> Parser
Parser --> Dataset
DevExecution --> DevStore
DevApi --> DevStore
DevApi --> DevPlaybook
DevPlaybook --> DevCapability
DevPlaybook --> DevStore

DevCapability ..> ConfigDir : only config loader
DevStore --> EvidenceFiles
DevPlaybook --> PlaybookVersions
DevPlaybook --> BatchRuns
AnalysisRun --> EvidenceFiles : refs
BatchRuns --> PlaybookVersions : playbook_version_ref

note bottom of ConfigDir
Config files 表达基础能力、护栏和资源边界；
PlaybookVersion 不放在 config 中。
end note

note bottom of Workspace
workspace files 表达分析状态、证据、PlaybookVersion、
BatchTraceResult、聚合和报告引用。
end note
@enduml
```

说明：

- 开发视图把代码模块、配置文件和 workspace 文件分开，避免把 workflow/batch lifecycle 放进 service 代码或 config。
- `capability` 是唯一配置加载入口；它向 execution、playbook 提供已校验契约。
- PlaybookVersion 是工作区产物；批量分析引用 PlaybookVersion 并写入 BatchTraceResult 与 aggregation。

## 5. 进程视图

```plantuml
@startuml
title 4+1 进程视图 - 运行时协作主路径
skinparam shadowing false
autonumber

actor "User" as User
participant "Skill / CLI / LLM" as Skill
database "Workspace Files" as Workspace
participant "htrace-service api" as Api
participant "capability gate" as Capability
participant "execution" as Execution
participant "trace adapter" as TraceAdapter
participant "evidence / artifact store" as Store
participant "playbook module" as Playbook

box "htrace-parser service" #LightBlue
  participant "htrace-parser" as Parser
  database "Trace Dataset(s)" as Dataset
end box

User -> Skill : 提交 trace 分析请求\n或选择 PlaybookVersion + cohort
Skill -> Workspace : create_or_resume workspace\n读取 Checklist / PlaybookVersion / batch state

alt 整体分析流程
  Skill -> Api : execute Checklist step\natomic / composite / bounded read / artifact
  Api -> Capability : validate inputs / refs / guards
  Capability -> Execution : validated execution contract
  Execution -> TraceAdapter : 执行查询与 transform
  TraceAdapter -> Parser : 参数化查询
  Parser -> Dataset : 内部查询
  Dataset --> Parser : rows
  Parser --> TraceAdapter : parser result
  TraceAdapter --> Execution : normalized rows
  Execution -> Store : 写 evidence / summary / receipt
  Store --> Api : refs
  Api --> Skill : ok / partial / blocked + refs
  Skill -> Workspace : 更新 Checklist / evidence-map\n写 report / revision / deep step refs

  opt deep analysis decision
    Skill -> Workspace : 读取 summaries / bounded read refs
    Skill -> Skill : LLM 辅助判断下一步
    Skill -> Workspace : 追加 deep steps 或 plan-revisions
  end

else Playbook 使用分析
  Skill -> Api : validate(playbook, cohort)\n含 cohort / quota / concurrency preflight
  Api -> Playbook : validate request
  Playbook -> Capability : 校验 PlaybookVersion / cohort\nresource_policy / retention / retry budget
  Api --> Skill : 可以执行 / 需要调整

  loop 每条 trace
    Skill -> Api : evaluate(playbook_version, trace_ref)
    Api -> Playbook : evaluate request
    Playbook -> Capability : 校验 evaluator rules / refs
    Capability -> Execution : validated query contract
    Execution -> TraceAdapter : Playbook 查询 / 特征提取
    TraceAdapter -> Parser : 参数化查询
    Parser -> Dataset : 内部查询
    Dataset --> Parser : rows
    Parser --> TraceAdapter : parser result
    TraceAdapter --> Execution : normalized rows
    Execution -> Store : 写 evidence / feature refs

    alt deterministic Playbook
      Execution --> Api : evaluator rules -> TraceVerdict
    else llm_assisted Playbook
      Api --> Skill : fixed input_bundle
      Skill -> Skill : 执行声明的 llm_decision
      Skill -> Api : validate_llm_decision(artifact)
      Api -> Capability : 校验 artifact / citations / schema
      Capability --> Api : TraceVerdict basis=llm_assisted
    end

    Api --> Skill : TraceVerdict + evidence refs
    Skill -> Workspace : 写 BatchTraceResult
  end

  Skill -> Api : aggregate(BatchTraceResult refs)
  Api -> Playbook : aggregate
  Playbook -> Store : 写 aggregation artifact / receipt
  Api --> Skill : BatchAggregation
  Skill -> Workspace : 写 aggregation / report refs
end
@enduml
```

说明：

- 进程视图把第 7 节整体分析流程和第 8 节 Playbook 使用时序合并为两条运行时主路径。
- 整体分析路径以 Checklist step 为基本循环；Playbook 路径以单 trace evaluation 为基本循环。
- llm_assisted 的 LLM 调用发生在 Skill / CLI / LLM 内部，htrace-service 只校验 LLMDecisionArtifact 并生成可审计 verdict。

## 6. 物理视图

```plantuml
@startuml
title 4+1 物理视图 - 进程、文件与服务边界
left to right direction
skinparam shadowing false

node "分析主机 / CI Runner" as Host {
  artifact "Skill / CLI / LLM runtime" as SkillRuntime
  folder "Workspace Root\nanalysis-runs/\nplaybooks/\nbatch-runs/" as WorkspaceRoot
  folder "Config Files\nprofiles / guards / atomics / composites" as ConfigRoot
  node "Rust htrace-service process" as ServiceProcess
  database "Evidence / Artifact Index\nSQLite or local index" as Index
}

node "htrace-parser service" as ParserNode {
  node "htrace-parser process" as ParserProcess
  database "Trace Dataset(s)\ntrace file / trace DB" as TraceDataset
  ParserProcess --> TraceDataset : 内部查询
}

cloud "LLM Provider / Local Model\n可选：deep decision / llm_decision" as Model
storage "Archive / Artifact Storage\nPhase 2 TBD" as Archive

SkillRuntime --> WorkspaceRoot : Checklist / PlaybookVersion\nBatch lifecycle / reports
SkillRuntime --> ServiceProcess : HTTP / CLI API
SkillRuntime --> Model : 受控 LLM 调用
ServiceProcess --> ConfigRoot : capability gate 加载配置
ServiceProcess --> WorkspaceRoot : evidence / artifact / receipt refs
ServiceProcess --> Index : evidence / artifact index
ServiceProcess --> ParserProcess : trace adapter 参数化查询
WorkspaceRoot --> Archive : 可选归档
Index --> Archive : 可选备份

note right of ServiceProcess
htrace-service 可重启、无 workflow 状态；
恢复依赖 workspace files 与 index reconcile。
end note

note bottom of WorkspaceRoot
analysis-runs 保存交互式分析；
playbooks 保存 PlaybookVersion；
batch-runs 保存 cohort、BatchTraceResult、aggregation 和报告引用。
end note
@enduml
```

说明：

- 物理视图保持最小部署：Skill / CLI / LLM runtime、workspace、config、htrace-service 可以同机；htrace-parser service 与 Trace Dataset 作为查询服务边界。
- htrace-service 只通过 capability gate 加载 Config Files；PlaybookVersion 和 batch lifecycle 位于 Workspace Root。
- LLM Provider / Local Model 只由 Skill / CLI / LLM runtime 调用，不进入 htrace-service 的确定性执行链路。
