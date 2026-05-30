param(
    [string]$Destination = "$env:USERPROFILE\.codex\skills\harmony-trace-analysis",
    [switch]$SkipPathUpdate
)

$ErrorActionPreference = "Stop"
$Source = Split-Path -Parent $MyInvocation.MyCommand.Path

if (-not (Test-Path (Join-Path $Source "SKILL.md"))) {
    throw "请在 skill 包根目录运行 install.ps1"
}

if (-not (Test-Path (Join-Path $Source "agents\openai.yaml"))) {
    throw "Codex 元数据不存在：agents\openai.yaml"
}

New-Item -ItemType Directory -Force -Path $Destination | Out-Null
Copy-Item -Path (Join-Path $Source "*") -Destination $Destination -Recurse -Force

$BinDir = Join-Path $Destination "bin\windows-x64"
$TraceProcessor = Join-Path $BinDir "trace_processor_shell.exe"
if (-not $SkipPathUpdate) {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @($userPath -split ";" | Where-Object { $_ -ne "" })
    if ($entries -notcontains $BinDir) {
        $entries += $BinDir
        [Environment]::SetEnvironmentVariable("Path", ($entries -join ";"), "User")
    }
}

if (Test-Path $TraceProcessor) {
    [Environment]::SetEnvironmentVariable("HTRACE_TRACE_PROCESSOR", $TraceProcessor, "User")
    $env:HTRACE_TRACE_PROCESSOR = $TraceProcessor
}

Write-Host "Installed harmony-trace-analysis skill to: $Destination"
Write-Host 'Codex invocation: $harmony-trace-analysis'
Write-Host "htrace binary: $BinDir\htrace.exe"
if (Test-Path $TraceProcessor) {
    Write-Host "HTRACE_TRACE_PROCESSOR: $TraceProcessor"
}
if (-not $SkipPathUpdate) {
    Write-Host "User PATH updated. Open a new terminal before using htrace by name."
}
Write-Host "Restart Codex or open a new Codex session so the skill list can refresh."
