param(
    [string]$OriginalResourceDir = "",
    [string]$OutputDir = "target",
    [switch]$CopyLocalResources,
    [switch]$SkipCompare
)

$ErrorActionPreference = "Stop"
$workspace = Split-Path -Parent $PSScriptRoot
$resourceDir = Join-Path $workspace "test\resource"
$localResourceDir = Join-Path $workspace "test\local_resource"
$outputPath = Join-Path $workspace $OutputDir
New-Item -ItemType Directory -Force -Path $outputPath | Out-Null

if ([string]::IsNullOrWhiteSpace($OriginalResourceDir)) {
    $defaultOriginalResourceDir = Join-Path $workspace "..\..\..\trace_streamer\test\resource"
    if (Test-Path -LiteralPath $defaultOriginalResourceDir) {
        $OriginalResourceDir = (Resolve-Path -LiteralPath $defaultOriginalResourceDir).Path
    }
}

if ($CopyLocalResources) {
    if ([string]::IsNullOrWhiteSpace($OriginalResourceDir)) {
        throw "Original resource dir was not provided and the default location was not found."
    }
    if (-not (Test-Path -LiteralPath $OriginalResourceDir)) {
        throw "Original resource dir not found: $OriginalResourceDir"
    }
    New-Item -ItemType Directory -Force -Path $localResourceDir | Out-Null
    Copy-Item -Path (Join-Path $OriginalResourceDir "*") -Destination $localResourceDir -Recurse -Force
}

Push-Location $workspace
try {
    cargo test --workspace
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    $traceFiles = @()
    foreach ($dir in @($resourceDir, $localResourceDir)) {
        if (Test-Path -LiteralPath $dir) {
            $traceFiles += Get-ChildItem -LiteralPath $dir -File -Recurse |
                Where-Object { $_.Extension -in @(".htrace", ".bin", ".data", ".txt", ".systrace", ".zip") }
        }
    }

    $inspectReport = @()
    foreach ($file in $traceFiles | Sort-Object FullName) {
        $inspectJson = Join-Path $outputPath ("inspect_" + ($file.BaseName -replace '[^A-Za-z0-9_.-]', '_') + ".json")
        $stderrPath = Join-Path $outputPath ("inspect_" + ($file.BaseName -replace '[^A-Za-z0-9_.-]', '_') + ".err.log")
        $previousErrorActionPreference = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        $output = & cargo run -q -p htrace-engine-cli --bin htrace-engine -- inspect --trace $file.FullName --json 2>$stderrPath
        $exitCode = $LASTEXITCODE
        $ErrorActionPreference = $previousErrorActionPreference
        if ($exitCode -eq 0) {
            $output | Set-Content -Encoding utf8 -LiteralPath $inspectJson
            $parsed = $output | ConvertFrom-Json
            $tableCount = ($parsed.tables.PSObject.Properties | Measure-Object).Count
            $nonEmpty = @($parsed.tables.PSObject.Properties | Where-Object { $_.Value.row_count -gt 0 } | ForEach-Object {
                [pscustomobject]@{
                    table = $_.Name
                    rows = $_.Value.row_count
                }
            })
            $inspectReport += [pscustomobject]@{
                file = $file.FullName
                status = "ok"
                trace_id = $parsed.trace_id
                start_ts = $parsed.start_ts
                end_ts = $parsed.end_ts
                table_count = $tableCount
                non_empty_tables = $nonEmpty
                report = $inspectJson
            }
        } else {
            $inspectReport += [pscustomobject]@{
                file = $file.FullName
                status = "failed"
                error_log = $stderrPath
            }
        }
    }

    $inspectSummary = Join-Path $outputPath "local_fixture_inspect_report.json"
    $inspectReport | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 -LiteralPath $inspectSummary

    if (-not $SkipCompare) {
        $compareOutput = & cargo run -q -p htrace-engine-cli --bin compare-cpp-sqlite -- `
            --html-output (Join-Path $outputPath "compare_validation_report.html") `
            --json
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
        $compareOutput |
            Where-Object { $_ -notmatch '^wrote ' } |
            Set-Content -Encoding utf8 -LiteralPath (Join-Path $outputPath "compare_validation_report.json")
    }

    Write-Output "inspect_report=$inspectSummary"
    if (-not $SkipCompare) {
        Write-Output ("compare_html=" + (Join-Path $outputPath "compare_validation_report.html"))
        Write-Output ("compare_json=" + (Join-Path $outputPath "compare_validation_report.json"))
    }
} finally {
    Pop-Location
}
