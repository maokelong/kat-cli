$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Htrace = Join-Path $Root "bin\windows-x64\htrace.exe"
$TraceProcessor = Join-Path $Root "bin\windows-x64\trace_processor_shell.exe"

if (-not (Test-Path (Join-Path $Root "SKILL.md"))) {
    throw "SKILL.md 不存在：$Root"
}

$SkillMd = Get-Content -LiteralPath (Join-Path $Root "SKILL.md") -Encoding UTF8 -Raw
if ($SkillMd -notmatch "(?m)^name:\s*harmony-trace-analysis\s*$") {
    throw "SKILL.md frontmatter 缺少 name: harmony-trace-analysis"
}

if ($SkillMd -notmatch "(?m)^description:.*Codex") {
    throw "SKILL.md description 必须明确面向 Codex 触发"
}

$OpenAiYaml = Join-Path $Root "agents\openai.yaml"
if (-not (Test-Path $OpenAiYaml)) {
    throw "Codex 元数据不存在：$OpenAiYaml"
}

$OpenAiYamlContent = Get-Content -LiteralPath $OpenAiYaml -Encoding UTF8 -Raw
if ($OpenAiYamlContent -notmatch 'display_name:\s*"Harmony Trace Analysis"') {
    throw "agents/openai.yaml 缺少 Codex UI display_name"
}

if ($OpenAiYamlContent -notmatch 'default_prompt:\s*".*\$harmony-trace-analysis') {
    throw 'agents/openai.yaml default_prompt 必须显式提到 $harmony-trace-analysis'
}

if ($OpenAiYamlContent -notmatch "allow_implicit_invocation:\s*true") {
    throw "agents/openai.yaml 必须允许 Codex 隐式触发"
}

if (-not (Test-Path $Htrace)) {
    throw "htrace.exe 不存在：$Htrace"
}

if (-not (Test-Path $TraceProcessor)) {
    throw "trace_processor_shell.exe 不存在：$TraceProcessor"
}

$env:HTRACE_TRACE_PROCESSOR = $TraceProcessor

& $Htrace version | Out-Host
& $TraceProcessor --version | Out-Host
& $Htrace profile list --skill-root $Root | Out-Host

$SmokeRoot = Join-Path $env:TEMP "harmony-trace-skill-smoke"
Remove-Item -LiteralPath $SmokeRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $SmokeRoot | Out-Null

$Runs = Join-Path $SmokeRoot "runs"
& $Htrace run init --out $Runs --trace sample.htrace --question "冷启动为什么慢" --json | Out-Host

Write-Host "Skill package verification passed."
