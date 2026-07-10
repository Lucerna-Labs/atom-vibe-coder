$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
. (Join-Path $PSScriptRoot "Learning-Loop.ps1")
$LearningIntent = if ([string]::IsNullOrWhiteSpace($env:MATH_ATOMS_PROVIDER_PROBE_INTENT)) { "Run the configured provider model against wiki graph RAG evidence on the Spiderweb Bus." } else { $env:MATH_ATOMS_PROVIDER_PROBE_INTENT }
$DurableCorrection = Get-AtomLearningContext -Intent $LearningIntent -Atoms "measure,compose,flow,preserve" -Limit 4
if ($DurableCorrection -match 'hits=0') { $DurableCorrection = "" }

Push-Location $Engine
try {
    $env:RUSTFLAGS = "-D warnings"
    cargo run -p math-atoms-core --example provider_probe --release
    if ($LASTEXITCODE -ne 0) { throw "provider execution gate failed with exit code $LASTEXITCODE" }
    Write-AtomLearningRecord -Source "provider-execution" -Intent $LearningIntent -Recipe "provider-model-loop" -Atoms "measure,compose,flow,preserve" -Gate "provider-execution" -Attempt 1 -Outcome "succeeded" -Correction $DurableCorrection -ProviderModel $env:MATH_ATOMS_PROVIDER_MODEL
}
catch {
    $failure = $_.Exception.Message
    Write-AtomLearningRecord -Source "provider-execution" -Intent $LearningIntent -Recipe "provider-model-loop" -Atoms "measure,compose,flow,preserve" -Gate "provider-execution" -Attempt 1 -Outcome "failed" -Failure $failure -ProviderModel $env:MATH_ATOMS_PROVIDER_MODEL
    throw
}
finally {
    Pop-Location
}
