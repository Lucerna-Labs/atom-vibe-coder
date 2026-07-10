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
    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    if ($PSVersionTable.PSVersion.Major -ge 7) {
        $oldNativeErrorPreference = $PSNativeCommandUseErrorActionPreference
        $PSNativeCommandUseErrorActionPreference = $false
    }
    try {
        $providerOutput = & cargo run --quiet -p math-atoms-core --example provider_probe --release 2>&1
        $providerExit = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $oldErrorActionPreference
        if ($PSVersionTable.PSVersion.Major -ge 7) {
            $PSNativeCommandUseErrorActionPreference = $oldNativeErrorPreference
        }
    }
    $providerText = ($providerOutput | Out-String).Trim()
    Write-Host $providerText
    if ($providerExit -ne 0) { throw "provider execution gate failed with exit code $providerExit" }
    $work = Get-AtomWorkEvidence -ProviderText $providerText
    if ($providerText -notmatch '(?m)^provider output artifact: (.+)$') {
        throw "provider execution gate did not return an output artifact path"
    }
    $providerArtifact = $Matches[1].Trim()
    if (-not (Test-Path -LiteralPath $providerArtifact)) {
        throw "provider execution artifact does not exist: $providerArtifact"
    }
    if ($providerText -notmatch 'provider output hash: (sha256:[0-9a-f]{64})') {
        throw "provider execution gate did not return an audited output hash"
    }
    $providerHash = $Matches[1]
    $actualHash = "sha256:" + (Get-FileHash -LiteralPath $providerArtifact -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $providerHash) {
        throw "provider execution artifact hash mismatch"
    }
    $harnessId = [Guid]::NewGuid().ToString("N")
    $harnessDir = Split-Path -Parent $providerArtifact
    $harnessSource = Join-Path $harnessDir "provider-transport-harness-$harnessId.rs"
    $harnessExe = Join-Path $harnessDir "provider-transport-harness-$harnessId.exe"
    $providerLength = (Get-Item -LiteralPath $providerArtifact).Length
    $harnessExpected = "MATH_ATOMS_PROVIDER_HARNESS_OK bytes=$providerLength"
    $harnessCode = @'
fn main() {
    let path = std::env::var("MATH_ATOMS_PROVIDER_OUTPUT").expect("provider output path");
    let bytes = std::fs::read(&path).expect("read provider output");
    assert!(!bytes.is_empty(), "provider output must not be empty");
    println!("MATH_ATOMS_PROVIDER_HARNESS_OK bytes={}", bytes.len());
}
'@
    [System.IO.File]::WriteAllText($harnessSource, $harnessCode)
    rustc --edition=2021 -D warnings $harnessSource -o $harnessExe
    if ($LASTEXITCODE -ne 0) {
        throw "provider transport harness compilation failed with exit code $LASTEXITCODE"
    }
    $attestation = New-AtomHarnessAttestation -HarnessId "provider-transport-functional-v1" -Gate "provider-execution" -Artifact $providerArtifact -Executable $harnessExe -ExpectedOutput $harnessExpected -AttestationPath (Join-Path $harnessDir "provider-transport-attestation-$harnessId.json") -WorkingDirectory $harnessDir -WorkPlanId $work.PlanId -ProviderModel $work.Model -ArtifactEnv "MATH_ATOMS_PROVIDER_OUTPUT"
    Write-AtomLearningRecord -Source "provider-execution" -Intent $LearningIntent -Recipe "provider-model-loop" -Atoms "measure,compose,flow,preserve" -Gate "provider-execution" -Attempt 1 -Outcome "succeeded" -Correction $DurableCorrection -Artifact $providerArtifact -ArtifactHash $providerHash -ProviderModel $work.Model -WorkPlanId $work.PlanId -WorkPlanManifest $work.Manifest -WorkPacketCount $work.PacketCount -CandidateVerificationManifest $work.CandidateManifest -CandidateVerificationHash $work.CandidateHash -CandidateBundleHash $work.CandidateBundleHash -CandidateAttempts $work.CandidateAttempts -CandidateRepairs $work.CandidateRepairs -HarnessAttestation $attestation.Path -HarnessAttestationHash $attestation.Hash
}
catch {
    $failure = $_.Exception.Message
    Write-AtomLearningRecord -Source "provider-execution" -Intent $LearningIntent -Recipe "provider-model-loop" -Atoms "measure,compose,flow,preserve" -Gate "provider-execution" -Attempt 1 -Outcome "failed" -Failure $failure -ProviderModel $env:MATH_ATOMS_PROVIDER_MODEL
    throw
}
finally {
    Pop-Location
}
