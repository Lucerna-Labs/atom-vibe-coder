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
    "MATH_ATOMS_PROVIDER_PLAN_TIMEOUT_SECONDS",
    "MATH_ATOMS_STORE_DIR",
    "MATH_ATOMS_WIKI_DIR",
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
    $isolatedId = [Guid]::NewGuid().ToString("N")
    $isolatedStore = Join-Path $OutDir ("cloud-store-" + $isolatedId)
    $isolatedWiki = Join-Path $OutDir ("cloud-wiki-empty-" + $isolatedId)
    $isolatedGenerated = Join-Path $isolatedStore ("generated-apps-" + $isolatedId)
    New-Item -ItemType Directory -Path $isolatedStore -Force | Out-Null
    New-Item -ItemType Directory -Path $isolatedWiki -Force | Out-Null
    $env:DEEPSEEK_API_KEY = $key.Trim()
    $env:MATH_ATOMS_PROVIDER_KIND = "deepseek"
    $env:MATH_ATOMS_PROVIDER_FORMAT = "chat"
    $env:MATH_ATOMS_PROVIDER_MODEL = "deepseek-v4-pro"
    $env:MATH_ATOMS_PROVIDER_URL = "https://api.deepseek.com/chat/completions"
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = "DEEPSEEK_API_KEY"
    $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = ""
    $env:MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS = "900"
    $env:MATH_ATOMS_PROVIDER_PLAN_TIMEOUT_SECONDS = "21600"
    $env:MATH_ATOMS_STORE_DIR = $isolatedStore
    $env:MATH_ATOMS_WIKI_DIR = $isolatedWiki
    $env:MATH_ATOMS_LEARNING_STORE = Join-Path $isolatedStore "learning.jsonl"
    $env:MATH_ATOMS_WORK_DIR = Join-Path $isolatedStore "work-packets"

    powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderBuildSeveralApps.ps1") -AppsRequired 1 -MaxAttempts $MaxAttempts -OutputRoot $isolatedGenerated
    if ($LASTEXITCODE -ne 0) {
        throw "DeepSeek Pro meticulous app build failed with exit code $LASTEXITCODE"
    }

    if (-not (Test-Path -LiteralPath $env:MATH_ATOMS_LEARNING_STORE)) {
        throw "DeepSeek Pro gate did not persist schema-v5 attested candidate learning evidence"
    }
    $learning = @([System.IO.File]::ReadAllLines($env:MATH_ATOMS_LEARNING_STORE) | ForEach-Object { $_ | ConvertFrom-Json })
    $success = @($learning | Where-Object { $_.source -eq "provider-multi-app" -and $_.gate -eq "app-counter" -and $_.outcome -eq "succeeded" }) | Select-Object -Last 1
    if ($null -eq $success) {
        throw "DeepSeek Pro gate did not persist a successful counter learning record"
    }
    if ([int]$success.schema_version -ne 5 -or $null -eq $success.candidate_verification) {
        throw "DeepSeek Pro learning record is missing schema-v5 candidate verification evidence"
    }
    if ([string]$success.candidate_verification.bundle_hash -ne [string]$success.artifact_hash) {
        throw "DeepSeek Pro candidate bundle is not the learned source artifact"
    }
    $source = [string]$success.artifact_path
    if (-not (Test-Path -LiteralPath $source)) {
        throw "DeepSeek Pro learning record source is missing: $source"
    }
    $generatedRootFull = [System.IO.Path]::GetFullPath($isolatedGenerated).TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    $sourceFull = [System.IO.Path]::GetFullPath($source)
    if (-not $sourceFull.StartsWith($generatedRootFull, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "DeepSeek learning artifact escaped the isolated generated root: $sourceFull"
    }
    $sourceHash = "sha256:" + (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($sourceHash -ne [string]$success.artifact_hash) {
        throw "DeepSeek Pro learning source hash does not recompute: expected=$($success.artifact_hash) actual=$sourceHash"
    }
    $exe = Join-Path (Split-Path -Parent $source) "counter.exe"
    if (-not (Test-Path -LiteralPath $exe)) {
        throw "DeepSeek Pro compiled executable is missing: $exe"
    }
    $exeFull = [System.IO.Path]::GetFullPath($exe)
    if (-not $exeFull.StartsWith($generatedRootFull, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "DeepSeek executable escaped the isolated generated root: $exeFull"
    }
    $attestationPath = [string]$success.harness_attestation_path
    if (-not (Test-Path -LiteralPath $attestationPath)) {
        throw "DeepSeek harness attestation is missing: $attestationPath"
    }
    $attestationHash = "sha256:" + (Get-FileHash -LiteralPath $attestationPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($attestationHash -ne [string]$success.harness_attestation_hash) {
        throw "DeepSeek harness attestation hash does not recompute"
    }
    $attestation = Get-Content -LiteralPath $attestationPath -Raw | ConvertFrom-Json
    $exeHash = "sha256:" + (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ([string]$attestation.artifact_path -ne $sourceFull -or [string]$attestation.executable_path -ne $exeFull -or [string]$attestation.executable_hash -ne $exeHash) {
        throw "DeepSeek harness attestation does not bind the isolated source and executable"
    }
    $actual = ((& $exe) -join "`n").Trim()
    if ($actual -ne $Expected) {
        throw "DeepSeek Pro exact learned artifact rerun failed: $actual"
    }
    $passed = [pscustomobject]@{ Source = $source; Exe = $exe; Output = $actual }
    $manifestPath = [string]$success.work_plan_manifest
    $plan = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ([int]$plan.packet_count -lt 19 -or -not [bool]$plan.expanded -or $plan.plan_id -ne $success.work_plan_id) {
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
