[CmdletBinding()]
param(
    [string]$DevkitRoot = ([IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))),
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$DatabaseHost,
    [string]$HostAddress,
    [ValidateRange(1, 65535)]
    [int]$DatabasePort = 5432,
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$DatabaseName,
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$DatabaseUser,
    [ValidateSet("verify-full", "verify-ca", "require", "disable")]
    [string]$SslMode = "verify-full",
    [string]$CaCertificate,
    [ValidateRange(1, 600)]
    [int]$ConnectTimeoutSeconds = 10,
    [string]$SqlFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion -lt [Version]"7.3") {
    throw "Invoke-LiveValidation.ps1 requires PowerShell 7.3 or newer"
}
$PSNativeCommandArgumentPassing = "Standard"

# 与 Verify 脚本重复边界代码，使 Verify 能在加载任何其他代码前完成 hash 校验。
$PackName = "postgresql-query"
$TextWorkflowName = "query-postgresql"
$FileWorkflowName = "query-postgresql-file"
$OutputName = "postgresql_result"

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

function Assert-KatSuccess {
    param(
        [Parameter(Mandatory)]
        [object]$Invocation,
        [Parameter(Mandatory)]
        [string]$Label
    )

    $status = Get-JsonProperty -Object $Invocation.Json -Name "status" -Label "$Label Response"
    if ($Invocation.ExitCode -ne 0 -or $status -ne "success") {
        $evidencePaths = @("log_path", "test_report_path") | ForEach-Object {
            $property = $Invocation.Json.PSObject.Properties[$_]
            if ($null -ne $property -and -not [string]::IsNullOrWhiteSpace([string]$property.Value)) {
                "$_=$($property.Value)"
            }
        }
        $evidence = if (@($evidencePaths).Count -eq 0) {
            ""
        }
        else {
            "; " + [string]::Join(", ", [string[]]$evidencePaths)
        }
        throw "$Label failed (exit code $($Invocation.ExitCode), status '$status'$evidence)"
    }
    return Get-JsonProperty -Object $Invocation.Json -Name "result" -Label "$Label Response"
}

function Assert-SuccessfulPackInspection {
    param(
        [Parameter(Mandatory)]
        [object]$Invocation
    )

    $result = Assert-KatSuccess -Invocation $Invocation -Label "PACK inspection"
    if ((Get-JsonProperty -Object $result -Name "name" -Label "inspection result") -ne $PackName) {
        throw "PACK inspection returned an unexpected PACK name"
    }
    $workflows = @(Get-JsonProperty -Object $result -Name "workflows" -Label "inspection result")
    $textWorkflows = @($workflows | Where-Object { $_.name -eq $TextWorkflowName })
    $fileWorkflows = @($workflows | Where-Object { $_.name -eq $FileWorkflowName })
    if ($textWorkflows.Count -ne 1 -or $fileWorkflows.Count -ne 1) {
        throw "PACK inspection must return '$TextWorkflowName' and '$FileWorkflowName'"
    }
    $textWorkflow = $textWorkflows[0]
    $fileWorkflow = $fileWorkflows[0]
    foreach ($workflow in @($textWorkflow, $fileWorkflow)) {
        if (@(Get-JsonProperty -Object $workflow -Name "required_tables" -Label "Workflow").Count -ne 0) {
            throw "Workflow required_tables must be empty"
        }
    }
    $parameters = @(Get-JsonProperty -Object $textWorkflow -Name "parameters" -Label "Workflow")
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
    if (@(Get-JsonProperty -Object $fileWorkflow -Name "parameters" -Label "Workflow").Count -ne 0) {
        throw "Workflow '$FileWorkflowName' must not expose parameters"
    }
}

$previousDataHome = [Environment]::GetEnvironmentVariable("KAT_DATA_HOME", "Process")
$previousPsycopgImpl = [Environment]::GetEnvironmentVariable("PSYCOPG_IMPL", "Process")
$securePassword = $null
$passwordPointer = [IntPtr]::Zero

Clear-PostgreSqlEnvironment
try {
    $root = Resolve-ExistingDirectory -Path $DevkitRoot -Label "Devkit root"
    $skillRoot = Resolve-ExistingDirectory -Path (Join-Path $root "skill") -Label "Skill root"
    $packRoot = Resolve-ExistingDirectory -Path (Join-Path $root "pack") -Label "PACK root"
    $dataHome = Resolve-ExistingDirectory -Path (Join-Path $root "data-home") -Label "KAT Data Home"
    $katExecutable = Resolve-ExistingFile -Path (
        Join-Path $skillRoot "scripts/targets/windows-x86_64/kat.exe"
    ) -Label "KAT executable"
    [void](Resolve-ExistingFile -Path (Join-Path $packRoot "pack.toml") -Label "PACK manifest")
    [void](Resolve-ExistingFile -Path (
        Join-Path $packRoot "queries/smoke.sql"
    ) -Label "PACK fixed SQL")
    $selectedWorkflow = $FileWorkflowName
    $sql = $null
    if ([string]::IsNullOrWhiteSpace($SqlFile)) {
        [Console]::Error.WriteLine(
            "No -SqlFile was provided; the PACK fixed SQL Workflow will be used."
        )
    }
    else {
        if (-not [IO.Path]::IsPathFullyQualified($SqlFile)) {
            throw "-SqlFile must be an absolute path: $SqlFile"
        }
        $resolvedSqlFile = Resolve-ExistingFile -Path $SqlFile -Label "SQL file"
        $strictUtf8 = [Text.UTF8Encoding]::new($true, $true)
        $sql = [IO.File]::ReadAllText($resolvedSqlFile, $strictUtf8)
        if ([string]::IsNullOrWhiteSpace($sql)) {
            throw "SQL file is empty: $resolvedSqlFile"
        }
        $selectedWorkflow = $TextWorkflowName
    }

    $resolvedCaCertificate = $null
    if ($SslMode -in @("verify-full", "verify-ca")) {
        if ([string]::IsNullOrWhiteSpace($CaCertificate)) {
            throw "-CaCertificate is required when -SslMode is '$SslMode'"
        }
        $resolvedCaCertificate = Resolve-ExistingFile -Path $CaCertificate -Label "PostgreSQL CA certificate"
    }
    elseif (-not [string]::IsNullOrWhiteSpace($CaCertificate)) {
        $resolvedCaCertificate = Resolve-ExistingFile -Path $CaCertificate -Label "PostgreSQL CA certificate"
    }

    [Environment]::SetEnvironmentVariable("KAT_DATA_HOME", $dataHome, "Process")
    [Environment]::SetEnvironmentVariable("PSYCOPG_IMPL", "binary", "Process")

    # Inspection imports trusted PACK source. It deliberately runs with every PG* variable removed.
    $inspection = Invoke-NativeJson `
        -CommandPath $katExecutable `
        -CommandArguments @(
            "inspect", "--pack", $PackName, "--pack-dir", $packRoot
        ) `
        -Label "PACK inspection"
    Assert-SuccessfulPackInspection -Invocation $inspection
    [Console]::Error.WriteLine(
        "PACK inspection succeeded before PostgreSQL credentials were requested."
    )

    [Console]::Error.Write("PostgreSQL password (kept only in this process): ")
    $securePassword = $Host.UI.ReadLineAsSecureString()
    [Console]::Error.WriteLine()
    if ($securePassword.Length -eq 0) {
        throw "PostgreSQL password cannot be empty"
    }
    $passwordPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($securePassword)
    $plainPassword = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($passwordPointer)

    [Environment]::SetEnvironmentVariable("PGHOST", $DatabaseHost, "Process")
    if (-not [string]::IsNullOrWhiteSpace($HostAddress)) {
        [Environment]::SetEnvironmentVariable("PGHOSTADDR", $HostAddress, "Process")
    }
    [Environment]::SetEnvironmentVariable("PGPORT", $DatabasePort.ToString(), "Process")
    [Environment]::SetEnvironmentVariable("PGDATABASE", $DatabaseName, "Process")
    [Environment]::SetEnvironmentVariable("PGUSER", $DatabaseUser, "Process")
    [Environment]::SetEnvironmentVariable("PGPASSWORD", $plainPassword, "Process")
    [Environment]::SetEnvironmentVariable("PGSSLMODE", $SslMode, "Process")
    if ($null -ne $resolvedCaCertificate) {
        [Environment]::SetEnvironmentVariable("PGSSLROOTCERT", $resolvedCaCertificate, "Process")
    }
    [Environment]::SetEnvironmentVariable("PGSSLCERTMODE", "disable", "Process")
    [Environment]::SetEnvironmentVariable("PGSSLMINPROTOCOLVERSION", "TLSv1.2", "Process")
    [Environment]::SetEnvironmentVariable("PGCONNECT_TIMEOUT", $ConnectTimeoutSeconds.ToString(), "Process")
    [Environment]::SetEnvironmentVariable("PGAPPNAME", "kat-postgresql-pack-devkit", "Process")
    [Environment]::SetEnvironmentVariable("PGCLIENTENCODING", "UTF8", "Process")
    $plainPassword = $null

    [Console]::Error.WriteLine("Running live PACK tests against PostgreSQL...")
    $testInvocation = Invoke-NativeJson `
        -CommandPath $katExecutable `
        -CommandArguments @("test", "--pack-dir", $packRoot) `
        -Label "PACK test"
    $testResult = Assert-KatSuccess -Invocation $testInvocation -Label "PACK test"

    [Console]::Error.WriteLine(
        "Publishing a Run without a Dataset through '$selectedWorkflow'..."
    )
    $runArguments = @(
        "run", "--pack", $PackName,
        "--workflow", $selectedWorkflow,
        "--pack-dir", $packRoot
    )
    if ($selectedWorkflow -eq $TextWorkflowName) {
        $runArguments += @("--", "--sql", $sql)
    }
    $runInvocation = Invoke-NativeJson `
        -CommandPath $katExecutable `
        -CommandArguments $runArguments `
        -Label "Workflow run"
    $runResult = Assert-KatSuccess -Invocation $runInvocation -Label "Workflow run"
    $runId = Get-JsonProperty -Object $runResult -Name "run_id" -Label "run result"
    if ([string]::IsNullOrWhiteSpace([string]$runId)) {
        throw "Workflow run returned an empty run_id"
    }
    $outputs = Get-JsonProperty -Object $runResult -Name "outputs" -Label "run result"
    if ($null -eq $outputs.PSObject.Properties[$OutputName]) {
        throw "Workflow run did not publish output '$OutputName'"
    }

    # Output Query is local. Drop database credentials immediately after the remote Run.
    Clear-PostgreSqlEnvironment
    if ($passwordPointer -ne [IntPtr]::Zero) {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordPointer)
        $passwordPointer = [IntPtr]::Zero
    }
    if ($null -ne $securePassword) {
        $securePassword.Dispose()
        $securePassword = $null
    }

    [Console]::Error.WriteLine("Querying the published Run Output...")
    # COUNT(*) avoids projecting temporal Arrow scalars that the current JSON
    # query response does not yet encode directly.
    $outputQuery = "SELECT COUNT(*) AS row_count FROM output.$OutputName"
    $queryInvocation = Invoke-NativeJson `
        -CommandPath $katExecutable `
        -CommandArguments @("query", "--run", [string]$runId, "--sql", $outputQuery) `
        -Label "Output Query"
    $queryResult = Assert-KatSuccess -Invocation $queryInvocation -Label "Output Query"

    [pscustomobject]@{
        status = "success"
        result = [ordered]@{
            pack_test = Get-JsonProperty -Object $testResult -Name "summary" -Label "PACK test result"
            workflow = $selectedWorkflow
            sql_source = if ($selectedWorkflow -eq $FileWorkflowName) {
                "pack-fixed-file"
            }
            else {
                "external-file-as-text"
            }
            run_id = $runId
            output = $OutputName
            output_metadata = $outputs.PSObject.Properties[$OutputName].Value
            query = $queryResult
        }
    } | ConvertTo-Json -Depth 12
}
finally {
    Clear-PostgreSqlEnvironment
    if ($passwordPointer -ne [IntPtr]::Zero) {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordPointer)
    }
    if ($null -ne $securePassword) {
        $securePassword.Dispose()
    }
    Restore-ProcessEnvironment -Name "KAT_DATA_HOME" -Value $previousDataHome
    Restore-ProcessEnvironment -Name "PSYCOPG_IMPL" -Value $previousPsycopgImpl
}
