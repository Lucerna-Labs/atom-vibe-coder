$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$TestDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-learning-functional-" + [Guid]::NewGuid().ToString("N"))
$Store = Join-Path $TestDir "learning.jsonl"
$Artifact = Join-Path $TestDir "bluetooth-driver.rs"
$OriginalStore = $env:MATH_ATOMS_LEARNING_STORE
$Intent = "Build a Bluetooth driver with connection validation"
$Atoms = "scan,project,compose,measure,preserve,order"
$Failure = "compiler rejected missing connect transition; provider token sk-functional-secret-123456"
. (Join-Path $PSScriptRoot "Learning-Loop.ps1")

try {
    New-Item -ItemType Directory -Path $TestDir -Force | Out-Null
    [System.IO.File]::WriteAllText($Artifact, 'fn main() { println!("bluetooth corrected"); }')
    $env:MATH_ATOMS_LEARNING_STORE = $Store

    Write-AtomLearningRecord -Source "self-learning-functional" -Intent $Intent -Recipe "provider-model-loop" -Atoms $Atoms -Gate "bluetooth-driver" -Attempt 1 -Outcome "failed" -Failure $Failure
    Write-AtomLearningRecord -Source "self-learning-functional" -Intent $Intent -Recipe "provider-model-loop" -Atoms $Atoms -Gate "bluetooth-driver" -Attempt 2 -Outcome "succeeded" -Correction $Failure -Artifact $Artifact

    $lines = @([System.IO.File]::ReadAllLines($Store))
    if ($lines.Count -ne 2) {
        throw "self-learning ledger expected 2 records, found $($lines.Count)"
    }
    $text = $lines -join "`n"
    if ($text -notmatch '"outcome":"failed"' -or $text -notmatch '"outcome":"succeeded"') {
        throw "self-learning ledger is missing a terminal outcome: $text"
    }
    if ($text -notmatch '"artifact_hash":"sha256:[0-9a-f]{64}"') {
        throw "self-learning success is missing an artifact hash: $text"
    }
    if ($text -match 'sk-functional-secret') {
        throw "self-learning ledger leaked token-like secret material"
    }
    if ($lines[-1] -notmatch '\[REDACTED\]' -or $lines[-1] -notmatch 'missing connect transition') {
        throw "self-learning success did not retain redacted correction evidence: $($lines[-1])"
    }

    Push-Location $Engine
    try {
        $probe = & cargo run --quiet -p math-atoms-core --example learning_context_probe -- $Intent $Store 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "restart graph retrieval probe failed with exit code $LASTEXITCODE`: $($probe | Out-String)"
        }
    }
    finally {
        Pop-Location
    }
    $probeText = ($probe | Out-String).Trim()
    if ($probeText -notmatch '^MATH_ATOMS_LEARNING_RETRIEVED ') {
        throw "restart graph retrieval did not return durable learning: $probeText"
    }
    Write-Host "self-learning functional ok: failed=1 succeeded=1 restart-retrieved=1 secrets=redacted artifact=hashed"
}
finally {
    $env:MATH_ATOMS_LEARNING_STORE = $OriginalStore
    Remove-Item -LiteralPath $TestDir -Recurse -Force -ErrorAction SilentlyContinue
}
