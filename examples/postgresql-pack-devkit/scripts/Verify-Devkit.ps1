[CmdletBinding()]
param(
    [string]$DevkitRoot = ([IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..")))
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion -lt [Version]"7.3") {
    throw "Verify-Devkit.ps1 requires PowerShell 7.3 or newer"
}
$PSNativeCommandArgumentPassing = "Standard"

# 保持脚本自包含：逐文件 hash 通过前不能加载 devkit 中的其他 PowerShell 代码。
$PackName = "postgresql-query"
$WorkflowName = "query-postgresql"
$ExpectedPythonVersion = "3.14.6"
$ExpectedPsycopgVersion = "3.3.4"
$ExpectedLibpqVersion = 180003

function Clear-PostgreSqlEnvironment {
    Get-ChildItem -LiteralPath Env: |
        Where-Object { $_.Name.StartsWith("PG", [StringComparison]::OrdinalIgnoreCase) } |
        ForEach-Object {
            Remove-Item -LiteralPath ("Env:" + $_.Name) -ErrorAction SilentlyContinue
        }
}

function Restore-ProcessEnvironment {
    param(
        [Parameter(Mandatory)]
        [string]$Name,
        [AllowNull()]
        [string]$Value
    )

    if ($null -eq $Value) {
        Remove-Item -LiteralPath ("Env:" + $Name) -ErrorAction SilentlyContinue
    }
    else {
        [Environment]::SetEnvironmentVariable($Name, $Value, "Process")
    }
}

function Resolve-ExistingDirectory {
    param(
        [Parameter(Mandatory)]
        [string]$Path,
        [Parameter(Mandatory)]
        [string]$Label
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "$Label directory is missing: $Path"
    }
    $item = Get-Item -LiteralPath $Path -Force
    return $item.FullName
}

function Resolve-ExistingFile {
    param(
        [Parameter(Mandatory)]
        [string]$Path,
        [Parameter(Mandatory)]
        [string]$Label
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is missing: $Path"
    }
    $item = Get-Item -LiteralPath $Path -Force
    return $item.FullName
}

function Get-RelativeKey {
    param(
        [Parameter(Mandatory)]
        [string]$Root,
        [Parameter(Mandatory)]
        [string]$Path
    )

    return [IO.Path]::GetRelativePath($Root, $Path).Replace("\", "/")
}

function Assert-PathInsideRoot {
    param(
        [Parameter(Mandatory)]
        [string]$Root,
        [Parameter(Mandatory)]
        [string]$Path,
        [Parameter(Mandatory)]
        [string]$Label
    )

    $rootPrefix = $Root.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) +
        [IO.Path]::DirectorySeparatorChar
    if (-not $Path.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label escapes the devkit root: $Path"
    }
}

function Test-DevkitHashes {
    param(
        [Parameter(Mandatory)]
        [string]$Root,
        [Parameter(Mandatory)]
        [string]$ManifestPath
    )

    $recordedPaths = [Collections.Generic.HashSet[string]]::new(
        [StringComparer]::OrdinalIgnoreCase
    )
    $entryCount = 0

    foreach ($line in Get-Content -LiteralPath $ManifestPath -Encoding UTF8) {
        $trimmed = $line.Trim()
        if ($trimmed.Length -eq 0 -or $trimmed.StartsWith("#")) {
            continue
        }
        if ($trimmed -notmatch "^(?<hash>[0-9A-Fa-f]{64})\s+\*?(?<path>.+)$") {
            throw "SHA256SUMS contains an invalid line: $line"
        }

        $expectedHash = $Matches.hash.ToLowerInvariant()
        $relativePath = $Matches.path.Trim()
        while ($relativePath.StartsWith("./", [StringComparison]::Ordinal)) {
            $relativePath = $relativePath.Substring(2)
        }
        if ($relativePath.Length -eq 0 -or [IO.Path]::IsPathRooted($relativePath)) {
            throw "SHA256SUMS contains a non-relative path: $relativePath"
        }

        $candidatePath = [IO.Path]::GetFullPath(
            (Join-Path $Root $relativePath.Replace("/", [IO.Path]::DirectorySeparatorChar))
        )
        Assert-PathInsideRoot -Root $Root -Path $candidatePath -Label "SHA256SUMS entry"
        $verifiedPath = Resolve-ExistingFile -Path $candidatePath -Label "SHA256SUMS entry"
        $relativeKey = Get-RelativeKey -Root $Root -Path $verifiedPath
        if (-not $recordedPaths.Add($relativeKey)) {
            throw "SHA256SUMS contains a duplicate path: $relativeKey"
        }

        $actualHash = (Get-FileHash -LiteralPath $verifiedPath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualHash -ne $expectedHash) {
            throw "SHA-256 mismatch: $relativeKey"
        }
        $entryCount++
    }

    if ($entryCount -eq 0) {
        throw "SHA256SUMS contains no file entries"
    }

    foreach ($file in Get-ChildItem -LiteralPath $Root -File -Recurse -Force) {
        if ($file.FullName.Equals($ManifestPath, [StringComparison]::OrdinalIgnoreCase)) {
            continue
        }
        $relativeKey = Get-RelativeKey -Root $Root -Path $file.FullName
        if ($relativeKey.StartsWith("data-home/", [StringComparison]::OrdinalIgnoreCase)) {
            continue
        }
        if (-not $recordedPaths.Contains($relativeKey)) {
            throw "File is missing from SHA256SUMS: $relativeKey"
        }
    }

    return $entryCount
}

function Invoke-NativeJson {
    param(
        [Parameter(Mandatory)]
        [string]$CommandPath,
        [Parameter(Mandatory)]
        [string[]]$CommandArguments,
        [Parameter(Mandatory)]
        [string]$Label
    )

    $stdoutLines = @(& $CommandPath @CommandArguments)
    $exitCode = $LASTEXITCODE
    $stdout = [string]::Join([Environment]::NewLine, [string[]]$stdoutLines)
    if ([string]::IsNullOrWhiteSpace($stdout)) {
        throw "$Label returned empty stdout (exit code $exitCode)"
    }
    try {
        $response = ConvertFrom-Json -InputObject $stdout -ErrorAction Stop
    }
    catch {
        throw "$Label did not return valid JSON (exit code $exitCode)"
    }

    return [pscustomobject]@{
        ExitCode = $exitCode
        Json = $response
    }
}

function Get-JsonProperty {
    param(
        [Parameter(Mandatory)]
        [object]$Object,
        [Parameter(Mandatory)]
        [string]$Name,
        [Parameter(Mandatory)]
        [string]$Label
    )

    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        throw "$Label is missing property '$Name'"
    }
    return $property.Value
}

function Assert-SuccessfulPackInspection {
    param(
        [Parameter(Mandatory)]
        [object]$Invocation
    )

    $status = Get-JsonProperty -Object $Invocation.Json -Name "status" -Label "KAT Response"
    if ($Invocation.ExitCode -ne 0 -or $status -ne "success") {
        throw "PACK inspection failed (exit code $($Invocation.ExitCode), status '$status')"
    }
    $result = Get-JsonProperty -Object $Invocation.Json -Name "result" -Label "KAT Response"
    if ((Get-JsonProperty -Object $result -Name "name" -Label "inspection result") -ne $PackName) {
        throw "PACK inspection returned an unexpected PACK name"
    }

    $workflows = @(Get-JsonProperty -Object $result -Name "workflows" -Label "inspection result")
    $matchingWorkflows = @($workflows | Where-Object { $_.name -eq $WorkflowName })
    if ($matchingWorkflows.Count -ne 1) {
        throw "PACK inspection must return exactly one '$WorkflowName' Workflow"
    }
    $workflow = $matchingWorkflows[0]
    if (@(Get-JsonProperty -Object $workflow -Name "required_tables" -Label "Workflow").Count -ne 0) {
        throw "Workflow required_tables must be empty"
    }

    $parameters = @(Get-JsonProperty -Object $workflow -Name "parameters" -Label "Workflow")
    $sqlParameters = @($parameters | Where-Object { $_.name -eq "sql" })
    if ($sqlParameters.Count -ne 1) {
        throw "Workflow must expose exactly one 'sql' parameter"
    }
    $sqlParameter = $sqlParameters[0]
    if (
        (Get-JsonProperty -Object $sqlParameter -Name "type" -Label "sql parameter") -ne "string" -or
        (Get-JsonProperty -Object $sqlParameter -Name "option" -Label "sql parameter") -ne "--sql" -or
        (Get-JsonProperty -Object $sqlParameter -Name "required" -Label "sql parameter") -ne $true
    ) {
        throw "Workflow 'sql' parameter must be a required string exposed as --sql"
    }
}

$previousDataHome = [Environment]::GetEnvironmentVariable("KAT_DATA_HOME", "Process")
$previousPsycopgImpl = [Environment]::GetEnvironmentVariable("PSYCOPG_IMPL", "Process")

Clear-PostgreSqlEnvironment
try {
    $root = Resolve-ExistingDirectory -Path $DevkitRoot -Label "Devkit root"
    $manifest = Resolve-ExistingFile -Path (Join-Path $root "SHA256SUMS") -Label "SHA256SUMS"
    $hashCount = Test-DevkitHashes -Root $root -ManifestPath $manifest

    $skillRoot = Resolve-ExistingDirectory -Path (Join-Path $root "skill") -Label "Skill root"
    $packRoot = Resolve-ExistingDirectory -Path (Join-Path $root "pack") -Label "PACK root"
    $dataHome = Resolve-ExistingDirectory -Path (Join-Path $root "data-home") -Label "KAT Data Home"
    [void](Resolve-ExistingFile -Path (Join-Path $root "README.md") -Label "README")
    [void](Resolve-ExistingFile -Path (Join-Path $root "ENVIRONMENT.md") -Label "Environment guide")
    [void](Resolve-ExistingFile -Path (Join-Path $root "DEVKIT-MANIFEST.json") -Label "Devkit manifest")
    [void](Resolve-ExistingFile -Path (
        Join-Path $root "scripts/Invoke-LiveValidation.ps1"
    ) -Label "Live validation script")
    [void](Resolve-ExistingFile -Path (
        Join-Path $skillRoot "SKILL.md"
    ) -Label "KAT Skill entry")
    $katExecutable = Resolve-ExistingFile -Path (
        Join-Path $skillRoot "scripts/targets/windows-x86_64/kat.exe"
    ) -Label "KAT executable"
    $pythonExecutable = Resolve-ExistingFile -Path (
        Join-Path $skillRoot "scripts/targets/windows-x86_64/python/python.exe"
    ) -Label "Bundled Python"
    [void](Resolve-ExistingFile -Path (Join-Path $packRoot "pack.toml") -Label "PACK manifest")
    [void](Resolve-ExistingFile -Path (Join-Path $root "queries/smoke.sql") -Label "Smoke SQL")

    [Environment]::SetEnvironmentVariable("KAT_DATA_HOME", $dataHome, "Process")
    [Environment]::SetEnvironmentVariable("PSYCOPG_IMPL", "binary", "Process")

    $pythonProbe = @'
import json
import platform
import psycopg
import sys
from psycopg import pq

print(json.dumps({
    "architecture": platform.machine(),
    "python": ".".join(str(part) for part in sys.version_info[:3]),
    "psycopg": psycopg.__version__,
    "pq_impl": pq.__impl__,
    "libpq": pq.version(),
}))
'@
    $hostInvocation = Invoke-NativeJson `
        -CommandPath $pythonExecutable `
        -CommandArguments @("-I", "-B", "-X", "utf8", "-c", $pythonProbe) `
        -Label "Bundled Python probe"
    if ($hostInvocation.ExitCode -ne 0) {
        throw "Bundled Python probe failed (exit code $($hostInvocation.ExitCode))"
    }
    $hostInfo = $hostInvocation.Json
    $architecture = Get-JsonProperty -Object $hostInfo -Name "architecture" -Label "host probe"
    if (
        $architecture -notin @("AMD64", "x86_64") -or
        (Get-JsonProperty -Object $hostInfo -Name "python" -Label "host probe") -ne $ExpectedPythonVersion -or
        (Get-JsonProperty -Object $hostInfo -Name "psycopg" -Label "host probe") -ne $ExpectedPsycopgVersion -or
        (Get-JsonProperty -Object $hostInfo -Name "pq_impl" -Label "host probe") -ne "binary" -or
        (Get-JsonProperty -Object $hostInfo -Name "libpq" -Label "host probe") -ne $ExpectedLibpqVersion
    ) {
        throw "Bundled Python/Psycopg versions do not match the devkit lock"
    }

    $inspection = Invoke-NativeJson `
        -CommandPath $katExecutable `
        -CommandArguments @(
            "inspect", "--pack", $PackName, "--pack-dir", $packRoot
        ) `
        -Label "PACK inspection"
    Assert-SuccessfulPackInspection -Invocation $inspection

    [pscustomobject]@{
        status = "success"
        result = [ordered]@{
            verified_files = $hashCount
            python = $hostInfo.python
            psycopg = $hostInfo.psycopg
            psycopg_implementation = $hostInfo.pq_impl
            libpq = $hostInfo.libpq
            architecture = $architecture
            pack = $PackName
            workflow = $WorkflowName
            password_present_during_inspection = $false
        }
    } | ConvertTo-Json -Depth 5
}
finally {
    Clear-PostgreSqlEnvironment
    Restore-ProcessEnvironment -Name "KAT_DATA_HOME" -Value $previousDataHome
    Restore-ProcessEnvironment -Name "PSYCOPG_IMPL" -Value $previousPsycopgImpl
}
