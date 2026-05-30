# Skill 自进化闭环 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 `harmony-trace-analysis` 建立可重复执行的自动闭环自进化基础设施，支持多代理 E2E 验证、评审、版本记录和每轮 push。

**Architecture:** 仓库只沉淀可审计的机械部分：轮次目录创建、skill 安装、验证、报告汇总、agent 提示词生成和 `version.md` 记录模板。Codex 主控会话负责实际 subagent 调度；实现 worker 必须使用 Superpowers 工作流，E2E/评审代理使用轮次目录里的提示词和产物契约执行。

**Tech Stack:** PowerShell 5+ 脚本、Rust `htrace` CLI、Codex skills、Codex subagents、Git。

---

## 执行前约束

- 当前仓库可能存在未提交的 skill 适配改动。执行本计划时不得回滚这些改动。
- 除非用户另行要求，不提交 `D:\work\self_improved\test\evolution\` 下的大型测试产物。
- 每个任务完成后独立提交。提交前只 stage 本任务负责的文件。
- 如果本机没有 `cargo`，Rust 测试步骤记录为 blocked，但仍运行 PowerShell 和 skill 验证。

## 文件结构

- Create: `version.md`
  - 负责记录每轮自进化历史、分数、commit、push、残留风险和下一轮候选项。
- Create outside repo: `D:\work\self_improved\test\evolution-tools\install-round-skill.ps1`
  - 负责创建轮次目录，将当前 `skill/` 安装到隔离测试目录，并写入 skill 文件哈希。
- Create outside repo: `D:\work\self_improved\test\evolution-tools\verify-round.ps1`
  - 负责验证仓内 skill、轮次 skill、测试 trace、必需二进制和基础 smoke run。
- Create outside repo: `D:\work\self_improved\test\evolution-tools\collect-round-report.ps1`
  - 负责汇总 E2E 与多评审输出，生成 `round-report.md`、`handoff.md` 和 `version-entry.md`。
- Create outside repo: `D:\work\self_improved\test\evolution-tools\new-agent-prompts.ps1`
  - 负责为 Implementation Worker、E2E Runner、Flow Auditor、Report Reviewer、Performance Reviewer 生成中文提示词文件。
- Create outside repo: `D:\work\self_improved\test\evolution-tools\assert-round-submission.ps1`
  - 负责在每轮结束后强制确认 `version.md` 已记录、源码已提交、当前分支已 push，且远端分支指向本地 HEAD。
- Modify: `skill/SKILL.md`
  - 增加自进化资源导航，提醒 Codex 自进化时读取设计和轮次契约。
- Modify: `skill/MANIFEST.txt`
  - 纳入新增 skill reference 文件，如果后续任务创建该文件。
- Modify: `skill/SHA256SUMS.txt`
  - 同步新增或修改后的 skill 包文件哈希。

## Task 1: 初始化 version.md

**Files:**
- Create: `version.md`

- [ ] **Step 1: 创建 version.md**

使用 `apply_patch` 创建：

```markdown
# Harmony Trace Skill Evolution Versions

This file is the durable history for the `harmony-trace-analysis` self-evolution loop.
Each round appends one entry before the round commit is created.

## Round Entry Format

```markdown
## Round N - YYYY-MM-DD HH:mm

- Source commit before:
- Source commit after:
- Remote branch:
- Test trace:
- Installed skill path:
- E2E run dir:
- Final report:
- Scores:
  - UX:
  - Tool correctness:
  - Flow control:
  - Performance:
  - Evidence quality:
  - Recoverability:
- Fixed:
- Found:
- Deferred:
- Hard failures:
- Next round candidates:
- Context handoff:
```

## Baseline

- Design spec: `docs/superpowers/specs/2026-05-29-skill-self-evolution-design.md`
- Plan: `docs/superpowers/plans/2026-05-29-skill-self-evolution.md`
- Test trace: `D:\work\self_improved\test\test.htrace`
```

- [ ] **Step 2: 验证 version.md 可读**

Run:

```powershell
[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
Test-Path .\version.md
Select-String -LiteralPath .\version.md -Pattern "Round Entry Format|test.htrace"
```

Expected:

```text
True
version.md:...:## Round Entry Format
version.md:...:- Test trace: `D:\work\self_improved\test\test.htrace`
```

- [ ] **Step 3: 提交 Task 1**

```powershell
git add -- version.md
git commit -m "docs: add evolution version log"
```

Expected: commit succeeds. If git author identity is missing, use one-shot config:

```powershell
git -c user.name="Codex" -c user.email="codex@local" commit -m "docs: add evolution version log"
```

## Task 2: 创建轮次 skill 安装脚本

**Files:**
- Create outside repo: `D:\work\self_improved\test\evolution-tools\install-round-skill.ps1`

- [ ] **Step 1: 写入 install-round-skill.ps1**

使用 `apply_patch` 创建完整脚本：

```powershell
param(
    [Parameter(Mandatory = $true)]
    [int]$Round,

    [string]$RepoRoot = "D:\work\self_improved\kat-rs",
    [string]$TestRoot = "D:\work\self_improved\test"
)

$ErrorActionPreference = "Stop"

function Convert-ToRoundName {
    param([int]$Round)
    if ($Round -lt 0) {
        throw "Round must be >= 0"
    }
    if ($Round -eq 0) {
        return "round-000-baseline"
    }
    return ("round-{0:D3}" -f $Round)
}

function Assert-PathInside {
    param(
        [Parameter(Mandatory = $true)][string]$Child,
        [Parameter(Mandatory = $true)][string]$Parent
    )
    $childFull = [System.IO.Path]::GetFullPath($Child)
    $parentFull = [System.IO.Path]::GetFullPath($Parent)
    if (-not $childFull.StartsWith($parentFull, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to write outside parent. Child=$childFull Parent=$parentFull"
    }
}

$SkillSource = Join-Path $RepoRoot "skill"
if (-not (Test-Path (Join-Path $SkillSource "SKILL.md"))) {
    throw "Missing source skill: $SkillSource"
}
if (-not (Test-Path (Join-Path $SkillSource "agents\openai.yaml"))) {
    throw "Missing Codex metadata: $SkillSource\agents\openai.yaml"
}

$RoundName = Convert-ToRoundName -Round $Round
$EvolutionRoot = Join-Path $TestRoot "evolution"
$RoundRoot = Join-Path $EvolutionRoot $RoundName
$SkillDest = Join-Path $RoundRoot "skills\harmony-trace-analysis"

Assert-PathInside -Child $RoundRoot -Parent $EvolutionRoot
Assert-PathInside -Child $SkillDest -Parent $RoundRoot

New-Item -ItemType Directory -Force -Path $RoundRoot | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $RoundRoot "e2e") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $RoundRoot "reviews") | Out-Null

if (Test-Path $SkillDest) {
    Remove-Item -LiteralPath $SkillDest -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $SkillDest | Out-Null
Copy-Item -LiteralPath (Join-Path $SkillSource "*") -Destination $SkillDest -Recurse -Force

$files = Get-ChildItem -LiteralPath $SkillDest -Recurse -File |
    ForEach-Object { $_.FullName.Substring($SkillDest.Length + 1).Replace('\', '/') } |
    Sort-Object

$hashLines = foreach ($rel in $files) {
    $path = Join-Path $SkillDest ($rel -replace '/', '\')
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    "$hash  $rel"
}
$hashPath = Join-Path $RoundRoot "source-skill-sha256.txt"
[System.IO.File]::WriteAllLines($hashPath, [string[]]$hashLines, [System.Text.UTF8Encoding]::new($false))

$metadata = [ordered]@{
    round = $Round
    round_name = $RoundName
    repo_root = $RepoRoot
    test_root = $TestRoot
    skill_source = $SkillSource
    skill_dest = $SkillDest
    installed_at = (Get-Date).ToString("s")
    source_commit = (git -C $RepoRoot rev-parse HEAD)
    source_branch = (git -C $RepoRoot branch --show-current)
}
$metadataPath = Join-Path $RoundRoot "round-metadata.json"
$metadata | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $metadataPath -Encoding UTF8

[pscustomobject]@{
    round = $Round
    round_root = $RoundRoot
    skill_path = $SkillDest
    metadata = $metadataPath
    hashes = $hashPath
} | ConvertTo-Json -Depth 4
```

- [ ] **Step 2: 运行脚本创建 baseline 轮次目录**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\install-round-skill.ps1 -Round 0
```

Expected:

```json
{
  "round": 0,
  "round_root": "D:\\work\\self_improved\\test\\evolution\\round-000-baseline",
  "skill_path": "D:\\work\\self_improved\\test\\evolution\\round-000-baseline\\skills\\harmony-trace-analysis",
  "metadata": "...round-metadata.json",
  "hashes": "...source-skill-sha256.txt"
}
```

- [ ] **Step 3: 验证轮次 skill 文件存在**

Run:

```powershell
$round = "D:\work\self_improved\test\evolution\round-000-baseline"
Test-Path "$round\skills\harmony-trace-analysis\SKILL.md"
Test-Path "$round\skills\harmony-trace-analysis\agents\openai.yaml"
Test-Path "$round\source-skill-sha256.txt"
```

Expected:

```text
True
True
True
```

- [ ] **Step 4: 提交 Task 2**

```powershell
Do not commit this helper script. It lives under `D:\work\self_improved\test\evolution-tools`.
```

## Task 3: 创建轮次验证脚本

**Files:**
- Create outside repo: `D:\work\self_improved\test\evolution-tools\verify-round.ps1`

- [ ] **Step 1: 写入 verify-round.ps1**

使用 `apply_patch` 创建：

```powershell
param(
    [Parameter(Mandatory = $true)]
    [int]$Round,

    [string]$RepoRoot = "D:\work\self_improved\kat-rs",
    [string]$TestRoot = "D:\work\self_improved\test",
    [string]$QuickValidate = "$env:USERPROFILE\.codex\skills\.system\skill-creator\scripts\quick_validate.py"
)

$ErrorActionPreference = "Stop"

function Convert-ToRoundName {
    param([int]$Round)
    if ($Round -eq 0) { return "round-000-baseline" }
    return ("round-{0:D3}" -f $Round)
}

function Invoke-AndLog {
    param(
        [Parameter(Mandatory = $true)][string]$CommandLine,
        [Parameter(Mandatory = $true)][string]$LogPath
    )
    Add-Content -LiteralPath $LogPath -Encoding UTF8 -Value "`n>>> $CommandLine"
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command $CommandLine 2>&1
    $exit = $LASTEXITCODE
    $output | ForEach-Object { Add-Content -LiteralPath $LogPath -Encoding UTF8 -Value $_ }
    if ($exit -ne 0) {
        throw "Command failed ($exit): $CommandLine"
    }
}

$RoundName = Convert-ToRoundName -Round $Round
$RoundRoot = Join-Path (Join-Path $TestRoot "evolution") $RoundName
$RoundSkill = Join-Path $RoundRoot "skills\harmony-trace-analysis"
$LogPath = Join-Path $RoundRoot "verify-round.log"
$TracePath = Join-Path $TestRoot "test.htrace"

if (-not (Test-Path $RoundSkill)) {
    throw "Round skill not installed: $RoundSkill"
}
if (-not (Test-Path $TracePath)) {
    throw "Missing test trace: $TracePath"
}
if (-not (Test-Path $QuickValidate)) {
    throw "Missing quick_validate.py: $QuickValidate"
}

Set-Content -LiteralPath $LogPath -Encoding UTF8 -Value "# Verify Round $RoundName"

Invoke-AndLog -LogPath $LogPath -CommandLine "python -X utf8 '$QuickValidate' '$RepoRoot\skill'"
Invoke-AndLog -LogPath $LogPath -CommandLine "python -X utf8 '$QuickValidate' '$RoundSkill'"
Invoke-AndLog -LogPath $LogPath -CommandLine "powershell -ExecutionPolicy Bypass -File '$RepoRoot\skill\verify.ps1'"
Invoke-AndLog -LogPath $LogPath -CommandLine "powershell -ExecutionPolicy Bypass -File '$RoundSkill\verify.ps1'"

$htrace = Join-Path $RoundSkill "bin\windows-x64\htrace.exe"
$processor = Join-Path $RoundSkill "bin\windows-x64\trace_processor_shell.exe"
if (-not (Test-Path $htrace)) {
    throw "Missing htrace binary: $htrace"
}
if (-not (Test-Path $processor)) {
    throw "Missing trace processor: $processor"
}

$env:HTRACE_TRACE_PROCESSOR = $processor
Invoke-AndLog -LogPath $LogPath -CommandLine "& '$htrace' version"
Invoke-AndLog -LogPath $LogPath -CommandLine "& '$htrace' profile list --skill-root '$RoundSkill'"

[pscustomobject]@{
    round = $Round
    round_root = $RoundRoot
    skill_path = $RoundSkill
    trace = $TracePath
    log = $LogPath
    ok = $true
} | ConvertTo-Json -Depth 4
```

- [ ] **Step 2: 运行轮次验证**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\verify-round.ps1 -Round 0
```

Expected:

```json
{
  "round": 0,
  "ok": true
}
```

如果 `python` 或 `cargo` 不可用，不要改脚本绕过；记录实际错误，再由 Implementation Worker 修环境或补充检测。

- [ ] **Step 3: 提交 Task 3**

```powershell
Do not commit this helper script. It lives under `D:\work\self_improved\test\evolution-tools`.
```

## Task 4: 创建 agent 提示词生成脚本

**Files:**
- Create outside repo: `D:\work\self_improved\test\evolution-tools\new-agent-prompts.ps1`

- [ ] **Step 1: 写入 new-agent-prompts.ps1**

使用 `apply_patch` 创建：

```powershell
param(
    [Parameter(Mandatory = $true)]
    [int]$Round,

    [string]$RepoRoot = "D:\work\self_improved\kat-rs",
    [string]$TestRoot = "D:\work\self_improved\test"
)

$ErrorActionPreference = "Stop"

function Convert-ToRoundName {
    param([int]$Round)
    if ($Round -eq 0) { return "round-000-baseline" }
    return ("round-{0:D3}" -f $Round)
}

$RoundName = Convert-ToRoundName -Round $Round
$RoundRoot = Join-Path (Join-Path $TestRoot "evolution") $RoundName
$PromptDir = Join-Path $RoundRoot "prompts"
$RoundSkillPath = Join-Path $RoundRoot "skills\harmony-trace-analysis"
$E2eDir = Join-Path $RoundRoot "e2e"
$ReviewDir = Join-Path $RoundRoot "reviews"
$TracePath = Join-Path $TestRoot "test.htrace"

New-Item -ItemType Directory -Force -Path $PromptDir | Out-Null
New-Item -ItemType Directory -Force -Path $E2eDir | Out-Null
New-Item -ItemType Directory -Force -Path $ReviewDir | Out-Null

$implementation = @"
请使用 Superpowers 工作流完成本轮实现修改。

仓库：$RepoRoot
轮次：$RoundName
所有权范围由主控消息指定。不得回滚或覆盖其他人的改动。

要求：
1. 根据任务选择 superpowers:systematic-debugging、superpowers:test-driven-development、superpowers:executing-plans 或 superpowers:verification-before-completion。
2. 修改完成后运行相关验证命令。
3. 返回修改文件、使用的 Superpowers workflow、验证命令、验证结果和残留风险。
"@

$e2e = @"
请使用当前轮安装的 skill 完成端到端冷启动分析。

Skill path: $RoundSkillPath
Trace path: $TracePath
Output dir: $E2eDir

硬性要求：
1. 不读取其他轮次的报告或结论。
2. 不直接调用 trace_processor_shell 或 trace_processor 执行 SQL。
3. 必须按顺序走完 8 个阶段：collect_input、load_profile、overview_atomics、topdown_brief、strategy_selection、deep_analysis、replay_generation、final_report。
4. 每次定位 run 后第一条流程命令必须是 `htrace run go $RunDir --json`。
5. 写入阶段产物后必须运行 `htrace run validate $RunDir --json`。
6. 阶段推进必须使用 htrace run advance。
7. 最终输出 stage completion status、artifact paths、final report path、failures、observations。
"@

$flowAudit = @"
请审计本轮 E2E 目录的流程合规性。

E2E dir: $E2eDir
Review output: $ReviewDir\flow-audit.md

重点检查：
1. htrace run go 是否是定位 run 后第一条流程命令。
2. 是否使用 run guard 或当前 stage metadata 判定允许动作。
3. 写入阶段产物后是否运行 run validate。
4. 阶段推进是否使用 run advance。
5. 是否出现 trace_processor_shell -Q、trace_processor_shell -q 或临时 SQL 绕过。
6. run-state.yaml 是否被手写篡改。

先列 hard violations，再列 warnings，最后给 0-5 的 Flow control 分数。
"@

$reportReview = @"
请评审本轮最终报告质量。

E2E dir: $E2eDir
Review output: $ReviewDir\report-review.md

重点检查：
1. 每个主要结论是否引用 atomic 输出字段或 artifact 路径。
2. 是否区分事实、推断和不确定性。
3. trace 缺失字段和回填方法是否说明。
4. 中文表达是否清楚，用户是否能知道下一步。
5. 是否有证据不足的过度结论。

给 UX、Evidence quality、Recoverability 三项 0-5 分，并列出具体改进建议。
"@

$performanceReview = @"
请评审本轮性能和可观测性。

E2E dir: $E2eDir
Review output: $ReviewDir\performance-review.md

重点检查：
1. E2E 总耗时。
2. 每个 atomic 或关键命令耗时，若日志没有耗时则指出观测缺口。
3. trace processor 启动次数。
4. 最大 artifact 和输出规模。
5. 重复执行或可缓存的步骤。
6. 16G 内存机器上的风险。

给 Performance 0-5 分，并列出可执行优化建议。
"@

$implementation | Set-Content -LiteralPath (Join-Path $PromptDir "implementation-worker.md") -Encoding UTF8
$e2e | Set-Content -LiteralPath (Join-Path $PromptDir "e2e-runner.md") -Encoding UTF8
$flowAudit | Set-Content -LiteralPath (Join-Path $PromptDir "flow-auditor.md") -Encoding UTF8
$reportReview | Set-Content -LiteralPath (Join-Path $PromptDir "report-reviewer.md") -Encoding UTF8
$performanceReview | Set-Content -LiteralPath (Join-Path $PromptDir "performance-reviewer.md") -Encoding UTF8

[pscustomobject]@{
    round = $Round
    prompt_dir = $PromptDir
    prompts = @(
        "implementation-worker.md",
        "e2e-runner.md",
        "flow-auditor.md",
        "report-reviewer.md",
        "performance-reviewer.md"
    )
} | ConvertTo-Json -Depth 4
```

- [ ] **Step 2: 生成 baseline 提示词**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\new-agent-prompts.ps1 -Round 0
```

Expected:

```json
{
  "round": 0,
  "prompt_dir": "D:\\work\\self_improved\\test\\evolution\\round-000-baseline\\prompts",
  "prompts": [
    "implementation-worker.md",
    "e2e-runner.md",
    "flow-auditor.md",
    "report-reviewer.md",
    "performance-reviewer.md"
  ]
}
```

- [ ] **Step 3: 验证提示词包含硬约束**

Run:

```powershell
$prompt = "D:\work\self_improved\test\evolution\round-000-baseline\prompts\e2e-runner.md"
Select-String -LiteralPath $prompt -Pattern "8 个阶段|run go|run validate|run advance|不直接调用"
```

Expected: every pattern appears at least once.

- [ ] **Step 4: 提交 Task 4**

```powershell
Do not commit this helper script. It lives under `D:\work\self_improved\test\evolution-tools`.
```

## Task 5: 创建 round report 汇总脚本

**Files:**
- Create outside repo: `D:\work\self_improved\test\evolution-tools\collect-round-report.ps1`

- [ ] **Step 1: 写入 collect-round-report.ps1**

使用 `apply_patch` 创建：

```powershell
param(
    [Parameter(Mandatory = $true)]
    [int]$Round,

    [string]$RepoRoot = "D:\work\self_improved\kat-rs",
    [string]$TestRoot = "D:\work\self_improved\test",
    [string]$RemoteBranch = ""
)

$ErrorActionPreference = "Stop"

function Convert-ToRoundName {
    param([int]$Round)
    if ($Round -eq 0) { return "round-000-baseline" }
    return ("round-{0:D3}" -f $Round)
}

function Read-OptionalFile {
    param([string]$Path)
    if (Test-Path $Path) {
        return (Get-Content -LiteralPath $Path -Encoding UTF8 -Raw).Trim()
    }
    return "未生成：$Path"
}

$RoundName = Convert-ToRoundName -Round $Round
$RoundRoot = Join-Path (Join-Path $TestRoot "evolution") $RoundName
$E2eDir = Join-Path $RoundRoot "e2e"
$ReviewDir = Join-Path $RoundRoot "reviews"
$RoundReport = Join-Path $RoundRoot "round-report.md"
$Handoff = Join-Path $RoundRoot "handoff.md"
$VersionEntry = Join-Path $RoundRoot "version-entry.md"
$MetadataPath = Join-Path $RoundRoot "round-metadata.json"

if (-not (Test-Path $RoundRoot)) {
    throw "Round root missing: $RoundRoot"
}

$metadata = @{}
if (Test-Path $MetadataPath) {
    $metadata = Get-Content -LiteralPath $MetadataPath -Encoding UTF8 -Raw | ConvertFrom-Json
}

$flow = Read-OptionalFile (Join-Path $ReviewDir "flow-audit.md")
$report = Read-OptionalFile (Join-Path $ReviewDir "report-review.md")
$performance = Read-OptionalFile (Join-Path $ReviewDir "performance-review.md")
$e2eSummary = Read-OptionalFile (Join-Path $E2eDir "summary.md")

$commitBefore = if ($metadata.source_commit) { $metadata.source_commit } else { git -C $RepoRoot rev-parse HEAD }
$branch = if ($RemoteBranch -ne "") { $RemoteBranch } else { git -C $RepoRoot branch --show-current }
$trace = Join-Path $TestRoot "test.htrace"
$finalReport = Join-Path $E2eDir "artifacts\final-report.md"
if (-not (Test-Path $finalReport)) {
    $finalReport = Join-Path $E2eDir "final-report.md"
}

$roundReportText = @"
# $RoundName Report

## Metadata

- Source commit before: $commitBefore
- Current source commit: $(git -C $RepoRoot rev-parse HEAD)
- Branch: $branch
- Test trace: $trace
- Round root: $RoundRoot
- E2E dir: $E2eDir
- Final report: $finalReport

## E2E Summary

$e2eSummary

## Flow Audit

$flow

## Report Review

$report

## Performance Review

$performance

## Synthesis

- Hard failures: inspect Flow Audit and E2E Summary.
- Fixed this round: 主控在提交前从 implementation worker final messages 复制具体修复条目。
- Deferred: inspect review recommendations.
- Next round candidates: select from hard failures first, then repeated warnings.
"@

$handoffText = @"
# $RoundName Handoff

Start the next round by reading:

1. $RepoRoot\version.md
2. $RoundReport
3. $MetadataPath
4. Git status in $RepoRoot

Do not rely on chat history for current round status. If this handoff follows a
hard failure, fix the hard failure before starting the next E2E run.
"@

$versionEntryText = @"
## Round $Round - $(Get-Date -Format "yyyy-MM-dd HH:mm")

- Source commit before: $commitBefore
- Source commit after: $(git -C $RepoRoot rev-parse HEAD)
- Remote branch: $branch
- Test trace: $trace
- Installed skill path: $RoundRoot\skills\harmony-trace-analysis
- E2E run dir: $E2eDir
- Final report: $finalReport
- Scores:
  - UX: see $ReviewDir\report-review.md
  - Tool correctness: see $RoundReport
  - Flow control: see $ReviewDir\flow-audit.md
  - Performance: see $ReviewDir\performance-review.md
  - Evidence quality: see $ReviewDir\report-review.md
  - Recoverability: see $Handoff
- Fixed: see $RoundReport
- Found: see $RoundReport
- Deferred: see $RoundReport
- Hard failures: see $RoundReport
- Next round candidates: see $RoundReport
- Context handoff: $Handoff
"@

$roundReportText | Set-Content -LiteralPath $RoundReport -Encoding UTF8
$handoffText | Set-Content -LiteralPath $Handoff -Encoding UTF8
$versionEntryText | Set-Content -LiteralPath $VersionEntry -Encoding UTF8

[pscustomobject]@{
    round = $Round
    round_report = $RoundReport
    handoff = $Handoff
    version_entry = $VersionEntry
} | ConvertTo-Json -Depth 4
```

- [ ] **Step 2: 创建最小 review fixture 并运行汇总**

Run:

```powershell
$root = "D:\work\self_improved\test\evolution\round-000-baseline"
New-Item -ItemType Directory -Force -Path "$root\e2e","$root\reviews" | Out-Null
"E2E fixture summary" | Set-Content -LiteralPath "$root\e2e\summary.md" -Encoding UTF8
"Flow control: 5/5" | Set-Content -LiteralPath "$root\reviews\flow-audit.md" -Encoding UTF8
"UX: 4/5`nEvidence quality: 4/5`nRecoverability: 4/5" | Set-Content -LiteralPath "$root\reviews\report-review.md" -Encoding UTF8
"Performance: 3/5" | Set-Content -LiteralPath "$root\reviews\performance-review.md" -Encoding UTF8
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\collect-round-report.ps1 -Round 0
```

Expected:

```json
{
  "round": 0,
  "round_report": "...round-report.md",
  "handoff": "...handoff.md",
  "version_entry": "...version-entry.md"
}
```

- [ ] **Step 3: 验证汇总文件存在**

Run:

```powershell
$root = "D:\work\self_improved\test\evolution\round-000-baseline"
Test-Path "$root\round-report.md"
Test-Path "$root\handoff.md"
Test-Path "$root\version-entry.md"
Select-String -LiteralPath "$root\round-report.md" -Pattern "Flow Audit|Report Review|Performance Review"
```

Expected:

```text
True
True
True
round-report.md:...:## Flow Audit
round-report.md:...:## Report Review
round-report.md:...:## Performance Review
```

- [ ] **Step 4: 提交 Task 5**

```powershell
Do not commit this helper script. It lives under `D:\work\self_improved\test\evolution-tools`.
```

## Task 6: 将自进化入口写入 skill

**Files:**
- Create: `skill/references/evolution-loop.md`
- Modify: `skill/SKILL.md`
- Modify: `skill/MANIFEST.txt`
- Modify: `skill/SHA256SUMS.txt`

- [ ] **Step 1: 创建 skill/references/evolution-loop.md**

使用 `apply_patch` 创建：

```markdown
# Skill 自进化循环

当用户要求改进、进化、评测或多轮优化本 skill 时，优先读取：

1. `docs/superpowers/specs/2026-05-29-skill-self-evolution-design.md`
2. `docs/superpowers/plans/2026-05-29-skill-self-evolution.md`
3. 仓库根目录的 `version.md`
4. 最近一轮 `D:\work\self_improved\test\evolution\round-*\handoff.md`

执行原则：

- 每轮使用 `D:\work\self_improved\test\evolution-tools\install-round-skill.ps1` 安装当前 skill 到隔离 round 目录。
- 每轮使用 `D:\work\self_improved\test\evolution-tools\verify-round.ps1` 做包和环境验证。
- E2E 验证必须使用当前轮安装的 skill 和 `D:\work\self_improved\test\test.htrace`。
- E2E 代理必须走完 `collect_input` 到 `final_report` 的 8 阶段。
- 多评审代理至少包括 flow audit、report review 和 performance review。
- 实现修改应由使用 Superpowers 的 implementation worker 完成。
- 每轮结束前更新 `version.md`，提交并 push 当前分支。
- 提交/push 是强制出口门禁：push 后必须运行 `D:\work\self_improved\test\evolution-tools\assert-round-submission.ps1 -Round <N>` 并得到 `ok=true`，否则本轮不得标记完成，也不得启动下一轮。
- 若出现流程违规、E2E 未完成、验证失败、报告缺证据、commit 失败、push 失败或提交门禁失败，停止自动闭环并向用户报告。
```

- [ ] **Step 2: 修改 skill/SKILL.md 的资源导航**

在 `## 资源导航` 列表中追加一行：

```markdown
- 自进化流程：用户要求进化、改进或多轮评测本 skill 时，读取 `references/evolution-loop.md`。
```

- [ ] **Step 3: 重建 MANIFEST 和 SHA256SUMS**

Run:

```powershell
$root = Resolve-Path -LiteralPath 'skill'
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$files = Get-ChildItem -LiteralPath $root -Recurse -File |
    Where-Object { $_.Name -ne 'SHA256SUMS.txt' } |
    ForEach-Object { $_.FullName.Substring($root.Path.Length + 1).Replace('\','/') } |
    Sort-Object
[System.IO.File]::WriteAllLines((Resolve-Path -LiteralPath 'skill\MANIFEST.txt').Path, [string[]]$files, $utf8NoBom)
$hashes = foreach ($rel in $files) {
    $path = Join-Path $root.Path ($rel -replace '/', '\')
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    "$hash  $rel"
}
[System.IO.File]::WriteAllLines((Resolve-Path -LiteralPath 'skill\SHA256SUMS.txt').Path, [string[]]$hashes, $utf8NoBom)
```

- [ ] **Step 4: 验证 skill 包**

Run:

```powershell
python -X utf8 "$env:USERPROFILE\.codex\skills\.system\skill-creator\scripts\quick_validate.py" "D:\work\self_improved\kat-rs\skill"
powershell -ExecutionPolicy Bypass -File .\skill\verify.ps1
```

Expected:

```text
Skill is valid!
Skill package verification passed.
```

- [ ] **Step 5: 提交 Task 6**

```powershell
git add -- skill/SKILL.md skill/references/evolution-loop.md skill/MANIFEST.txt skill/SHA256SUMS.txt
git commit -m "docs: add skill evolution reference"
```

## Task 7: 端到端脚手架验收

**Files:**
- No source file creation required.
- Reads: `D:\work\self_improved\test\evolution-tools\*.ps1`, `version.md`, `skill/`

- [ ] **Step 1: 从干净轮次目录重新安装 round 1 skill**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\install-round-skill.ps1 -Round 1
```

Expected: JSON 输出包含：

```text
"round": 1
"round-001"
"skills\\harmony-trace-analysis"
```

- [ ] **Step 2: 生成 round 1 agent prompts**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\new-agent-prompts.ps1 -Round 1
```

Expected: JSON 输出包含 `implementation-worker.md`、`e2e-runner.md`、`flow-auditor.md`、`report-reviewer.md`、`performance-reviewer.md`。

- [ ] **Step 3: 验证 round 1 skill**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\verify-round.ps1 -Round 1
```

Expected: JSON 输出包含：

```text
"round": 1
"ok": true
```

- [ ] **Step 4: 汇总 fixture 报告**

Run:

```powershell
$root = "D:\work\self_improved\test\evolution\round-001"
New-Item -ItemType Directory -Force -Path "$root\e2e","$root\reviews" | Out-Null
"Round 1 E2E fixture summary" | Set-Content -LiteralPath "$root\e2e\summary.md" -Encoding UTF8
"Flow control: 5/5" | Set-Content -LiteralPath "$root\reviews\flow-audit.md" -Encoding UTF8
"UX: 4/5`nEvidence quality: 4/5`nRecoverability: 4/5" | Set-Content -LiteralPath "$root\reviews\report-review.md" -Encoding UTF8
"Performance: 3/5" | Set-Content -LiteralPath "$root\reviews\performance-review.md" -Encoding UTF8
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\collect-round-report.ps1 -Round 1
```

Expected: JSON 输出包含 `round-report.md`、`handoff.md`、`version-entry.md`。

- [ ] **Step 5: 检查 git 状态，确认没有测试产物被纳入仓库**

Run:

```powershell
git status --short
```

Expected: only source files under `kat-rs/` appear. No `D:\work\self_improved\test\evolution\` path appears because it is outside the repo.

- [ ] **Step 6: 提交 Task 7 验收记录**

如果 Task 7 只运行命令、不修改源文件，不需要提交。若你把验收结果写入 `version.md`，则提交：

```powershell
git add -- version.md
git commit -m "docs: record evolution scaffold validation"
```

## Task 8: 第一轮真实自动闭环试运行

**Files:**
- Modify: `version.md`
- Potentially modify: files selected by Implementation Worker after review findings

- [ ] **Step 1: 启动 E2E Runner subagent**

主控会话读取：

```powershell
Get-Content -LiteralPath "D:\work\self_improved\test\evolution\round-001\prompts\e2e-runner.md" -Encoding UTF8
```

然后用该提示词启动 fresh subagent。要求它只读取当前轮 skill、`test.htrace` 和 `round-001` 输出目录。

- [ ] **Step 2: 启动三个独立评审 subagent**

E2E Runner 完成后，分别读取并分派：

```powershell
Get-Content -LiteralPath "D:\work\self_improved\test\evolution\round-001\prompts\flow-auditor.md" -Encoding UTF8
Get-Content -LiteralPath "D:\work\self_improved\test\evolution\round-001\prompts\report-reviewer.md" -Encoding UTF8
Get-Content -LiteralPath "D:\work\self_improved\test\evolution\round-001\prompts\performance-reviewer.md" -Encoding UTF8
```

Expected: 每个评审写入对应 review 文件，且主控关闭已完成 subagent。

- [ ] **Step 3: 汇总真实 round report**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\collect-round-report.ps1 -Round 1
```

Expected: `D:\work\self_improved\test\evolution\round-001\round-report.md` 和 `handoff.md` 被刷新。

- [ ] **Step 4: 如果存在可修复问题，启动 Implementation Worker**

主控给 worker 的任务必须包含：

```text
请使用 Superpowers 工作流完成本轮实现修改。
仓库：D:\work\self_improved\kat-rs
所有权范围：由本轮问题决定，必须是互不重叠的文件或目录。
不得回滚或覆盖其他人的改动。
返回修改文件、workflow、验证命令、验证结果和残留风险。
```

Expected: worker 修改限定范围内文件，主控 review diff 后运行相关验证。

- [ ] **Step 5: 更新 version.md**

将 `D:\work\self_improved\test\evolution\round-001\version-entry.md` 的内容追加到 `version.md`，并手动补齐 `Fixed`、`Found`、`Deferred`、`Hard failures`、`Next round candidates` 的具体条目。

- [ ] **Step 6: 提交、push 并执行强制提交门禁**

Run:

```powershell
git status --short
git add -- version.md skill docs README.md cli
git commit -m "evolution: round 1 scaffold and findings"
git push origin codex-skill
powershell -ExecutionPolicy Bypass -File D:\work\self_improved\test\evolution-tools\assert-round-submission.ps1 -Round 1
```

Expected:

```text
To https://github.com/ohfei/kat-rs.git
   codex-skill -> codex-skill
```

如果 git author identity 缺失，使用 one-shot commit config。若 commit、push 或 `assert-round-submission.ps1` 失败，停止自动闭环并向用户报告；不要启动下一轮。

## Self-Review Checklist

- Spec coverage:
  - 自动闭环：Task 2-8 覆盖。
  - 多评审代理：Task 4 和 Task 8 覆盖。
  - Superpowers Implementation Worker：Task 4 和 Task 8 覆盖。
  - 八阶段 E2E：Task 4 的 E2E prompt 和 Task 8 覆盖。
  - `version.md`：Task 1、Task 5、Task 8 覆盖。
  - commit/push 强制门禁：Task 8 覆盖。
  - context compaction handoff：Task 5 覆盖。
- Placeholder scan:
  - 本计划没有未落地的占位步骤。
  - 代码里的参数是 PowerShell 参数、运行时变量或示例输出省略号，不是未定义占位。
- Type and name consistency:
  - Round naming is consistently `round-000-baseline`, `round-001`, `round-{N:D3}`.
  - Script names match the design spec.
  - Review filenames are consistently `flow-audit.md`, `report-review.md`, `performance-review.md`.
