param(
    [int]$MaxAttempts = 3
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$OutDir = Join-Path $Engine "target\deepseek-pro-work-test"
$GeneratedRoot = Join-Path $Engine "target\provider-built-apps"
$Expected = "MATH_ATOMS_APP_OK counter total=4 stack=canonical"
$Saved = @{}
$Names = @(
    "MATH_ATOMS_PROVIDER_KIND",
    "MATH_ATOMS_PROVIDER_FORMAT",
    "MATH_ATOMS_PROVIDER_MODEL",
    "MATH_ATOMS_PROVIDER_URL",
    "MATH_ATOMS_PROVIDER_KEY_ENV",
    "MATH_ATOMS_PROVIDER_BODY_TEMPLATE",
    "MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS",
    "MATH_ATOMS_LEARNING_STORE",
    "MATH_ATOMS_WORK_DIR",
    "DEEPSEEK_API_KEY"
)
foreach ($name in $Names) {
    $Saved[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}

try {
    $key = [Environment]::GetEnvironmentVariable("DEEPSEEK_API_KEY", "Process")
    if ([string]::IsNullOrWhiteSpace($key)) {
        $key = [Environment]::GetEnvironmentVariable("DEEPSEEK_API_KEY", "User")
    }
    if ([string]::IsNullOrWhiteSpace($key)) {
        $key = [Environment]::GetEnvironmentVariable("DEEPSEEK_API_KEY", "Machine")
    }
    if ([string]::IsNullOrWhiteSpace($key)) {
        throw "Missing DEEPSEEK_API_KEY"
    }

    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
    $env:DEEPSEEK_API_KEY = $key.Trim()
    $env:MATH_ATOMS_PROVIDER_KIND = "deepseek"
    $env:MATH_ATOMS_PROVIDER_FORMAT = "chat"
    $env:MATH_ATOMS_PROVIDER_MODEL = "deepseek-v4-pro"
    $env:MATH_ATOMS_PROVIDER_URL = "https://api.deepseek.com/chat/completions"
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = "DEEPSEEK_API_KEY"
    $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = ""
    $env:MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS = "900"
    $env:MATH_ATOMS_LEARNING_STORE = Join-Path $OutDir ("learning-" + [Guid]::NewGuid().ToString("N") + ".jsonl")
    $env:MATH_ATOMS_WORK_DIR = Join-Path $OutDir ("work-packets-" + [Guid]::NewGuid().ToString("N"))

    powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderBuildSeveralApps.ps1") -AppsRequired 1 -MaxAttempts $MaxAttempts
    if ($LASTEXITCODE -ne 0) {
        throw "DeepSeek Pro meticulous app build failed with exit code $LASTEXITCODE"
    }

    $attempts = @(Get-ChildItem -LiteralPath $GeneratedRoot -Directory -Filter "counter-attempt-*" | Sort-Object Name)
    if ($attempts.Count -eq 0) {
        throw "DeepSeek Pro gate did not produce a counter attempt directory"
    }
    $passed = $null
    foreach ($attempt in $attempts) {
        $source = Join-Path $attempt.FullName "main.rs"
        $exe = Join-Path $attempt.FullName "counter.exe"
        if (-not (Test-Path -LiteralPath $source) -or -not (Test-Path -LiteralPath $exe)) {
            continue
        }
        $actual = ((& $exe) -join "`n").Trim()
        if ($actual -eq $Expected) {
            $passed = [pscustomobject]@{ Source = $source; Exe = $exe; Output = $actual }
            break
        }
    }
    if ($null -eq $passed) {
        throw "DeepSeek Pro generated artifacts did not rerun with the expected output"
    }

    if (-not (Test-Path -LiteralPath $env:MATH_ATOMS_LEARNING_STORE)) {
        throw "DeepSeek Pro gate did not persist schema-v3 learning evidence"
    }
    $learning = @([System.IO.File]::ReadAllLines($env:MATH_ATOMS_LEARNING_STORE) | ForEach-Object { $_ | ConvertFrom-Json })
    $success = @($learning | Where-Object { $_.source -eq "provider-multi-app" -and $_.gate -eq "app-counter" -and $_.outcome -eq "succeeded" }) | Select-Object -Last 1
    if ($null -eq $success) {
        throw "DeepSeek Pro gate did not persist a successful counter learning record"
    }
    $manifestPath = [string]$success.work_plan_manifest
    $plan = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ([int]$plan.packet_count -lt 13 -or -not [bool]$plan.expanded -or $plan.plan_id -ne $success.work_plan_id) {
        throw "DeepSeek Pro work plan was not meticulously expanded: $manifestPath"
    }
    $packetRecords = @(Get-ChildItem -LiteralPath (Split-Path -Parent $manifestPath) -Filter "*.json" | Where-Object Name -NotLike "plan-*.json")
    if ($packetRecords.Count -ne [int]$plan.packet_count) {
        throw "DeepSeek Pro work evidence count mismatch: plan=$($plan.packet_count) records=$($packetRecords.Count)"
    }

    Write-Host "deepseek pro work test ok: model=deepseek-v4-pro packets=$($plan.packet_count) generated, compiled, reran: $($passed.Output)"
}
finally {
    foreach ($name in $Names) {
        [Environment]::SetEnvironmentVariable($name, $Saved[$name], "Process")
    }
}
