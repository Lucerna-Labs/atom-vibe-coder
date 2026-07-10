$script:AtomLearningRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$script:AtomLearningEngine = Join-Path $script:AtomLearningRoot "atom-rendering-engine-main"
$script:AtomLearningBinary = Join-Path $script:AtomLearningEngine "target\debug\learning_probe.exe"
$script:AtomLearningReady = $false

function Initialize-AtomLearningProbe {
    if ($script:AtomLearningReady -and (Test-Path -LiteralPath $script:AtomLearningBinary)) {
        return
    }
    Push-Location $script:AtomLearningEngine
    try {
        cargo build --quiet -p math-atoms-learning --bin learning_probe
        if ($LASTEXITCODE -ne 0) {
            throw "learning probe build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
    if (-not (Test-Path -LiteralPath $script:AtomLearningBinary)) {
        throw "learning probe binary was not built: $script:AtomLearningBinary"
    }
    $script:AtomLearningReady = $true
}

function Invoke-AtomLearningProbe {
    param([Parameter(Mandatory = $true)][string[]]$Arguments)

    Initialize-AtomLearningProbe
    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $output = & $script:AtomLearningBinary @Arguments 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $oldErrorActionPreference
    }
    $text = ($output | Out-String).Trim()
    if ($exitCode -ne 0) {
        throw "learning probe failed with exit code ${exitCode}: $text"
    }
    return $text
}

function Write-AtomLearningRecord {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Intent,
        [Parameter(Mandatory = $true)][string]$Recipe,
        [Parameter(Mandatory = $true)][string]$Atoms,
        [Parameter(Mandatory = $true)][string]$Gate,
        [Parameter(Mandatory = $true)][int]$Attempt,
        [Parameter(Mandatory = $true)][ValidateSet("failed", "succeeded")][string]$Outcome,
        [string]$Failure = "",
        [string]$Correction = "",
        [string]$Artifact = "",
        [string]$ArtifactHash = "",
        [string]$ProviderModel = "",
        [string]$WorkPlanId = "",
        [string]$WorkPlanManifest = "",
        [int]$WorkPacketCount = 0,
        [string]$HarnessAttestation = "",
        [string]$HarnessAttestationHash = "",
        [int]$RouteLen = 4
    )

    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-learning-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    try {
        $intentFile = Join-Path $tempDir "intent.txt"
        [System.IO.File]::WriteAllText($intentFile, $Intent)
        $arguments = @(
            "record", "--source", $Source,
            "--intent-file", $intentFile,
            "--recipe", $Recipe,
            "--atoms", $Atoms,
            "--gate", $Gate,
            "--attempt", $Attempt.ToString(),
            "--outcome", $Outcome,
            "--route-len", $RouteLen.ToString()
        )
        if (-not [string]::IsNullOrWhiteSpace($Failure)) {
            $failureFile = Join-Path $tempDir "failure.txt"
            [System.IO.File]::WriteAllText($failureFile, $Failure)
            $arguments += @("--failure-file", $failureFile)
        }
        if (-not [string]::IsNullOrWhiteSpace($Correction)) {
            $correctionFile = Join-Path $tempDir "correction.txt"
            [System.IO.File]::WriteAllText($correctionFile, $Correction)
            $arguments += @("--correction-file", $correctionFile)
        }
        if (-not [string]::IsNullOrWhiteSpace($Artifact)) {
            $arguments += @("--artifact", $Artifact)
        }
        if (-not [string]::IsNullOrWhiteSpace($ArtifactHash)) {
            $arguments += @("--artifact-hash", $ArtifactHash)
        }
        if (-not [string]::IsNullOrWhiteSpace($ProviderModel)) {
            $arguments += @("--provider-model", $ProviderModel)
        }
        if (-not [string]::IsNullOrWhiteSpace($WorkPlanId)) {
            $arguments += @("--work-plan-id", $WorkPlanId)
        }
        if (-not [string]::IsNullOrWhiteSpace($WorkPlanManifest)) {
            $arguments += @("--work-plan-manifest", $WorkPlanManifest)
        }
        if ($WorkPacketCount -gt 0) {
            $arguments += @("--work-packet-count", $WorkPacketCount.ToString())
        }
        if (-not [string]::IsNullOrWhiteSpace($HarnessAttestation)) {
            $arguments += @("--harness-attestation", $HarnessAttestation)
        }
        if (-not [string]::IsNullOrWhiteSpace($HarnessAttestationHash)) {
            $arguments += @("--harness-attestation-hash", $HarnessAttestationHash)
        }
        $result = Invoke-AtomLearningProbe -Arguments $arguments
        if ($result -notmatch '^MATH_ATOMS_LEARNING_OK ') {
            throw "learning probe returned an unexpected result: $result"
        }
        Write-Host $result
    }
    finally {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function New-AtomHarnessAttestation {
    param(
        [Parameter(Mandatory = $true)][ValidateSet("rust-console-exact-v1", "native-pmre-functional-v1", "design-upload-functional-v1", "provider-transport-functional-v1", "self-learning-restart-v1")][string]$HarnessId,
        [Parameter(Mandatory = $true)][string]$Gate,
        [Parameter(Mandatory = $true)][string]$Artifact,
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$ExpectedOutput,
        [Parameter(Mandatory = $true)][string]$AttestationPath,
        [string]$WorkingDirectory = "",
        [string]$WorkPlanId = "",
        [string]$ProviderModel = "",
        [int]$TimeoutSeconds = 120,
        [ValidateSet("", "MATH_ATOMS_REAL_APP_BMP", "MATH_ATOMS_DESIGN_APP_BMP", "MATH_ATOMS_PROVIDER_OUTPUT")][string]$ArtifactEnv = ""
    )

    if ([string]::IsNullOrWhiteSpace($WorkingDirectory)) {
        $WorkingDirectory = Split-Path -Parent $Executable
    }
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-attestation-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    try {
        $expectedFile = Join-Path $tempDir "expected-output.txt"
        [System.IO.File]::WriteAllText($expectedFile, $ExpectedOutput)
        $arguments = @(
            "attest",
            "--harness-id", $HarnessId,
            "--gate", $Gate,
            "--artifact", $Artifact,
            "--executable", $Executable,
            "--working-directory", $WorkingDirectory,
            "--expected-output-file", $expectedFile,
            "--attestation", $AttestationPath,
            "--timeout-seconds", $TimeoutSeconds.ToString()
        )
        if (-not [string]::IsNullOrWhiteSpace($WorkPlanId)) {
            $arguments += @("--work-plan-id", $WorkPlanId)
        }
        if (-not [string]::IsNullOrWhiteSpace($ProviderModel)) {
            $arguments += @("--provider-model", $ProviderModel)
        }
        if (-not [string]::IsNullOrWhiteSpace($ArtifactEnv)) {
            $arguments += @("--artifact-env", $ArtifactEnv)
        }
        $result = Invoke-AtomLearningProbe -Arguments $arguments
        if ($result -notmatch '^MATH_ATOMS_ATTESTATION_OK path=(?<path>.+) hash=(?<hash>sha256:[0-9a-f]{64})$') {
            throw "attestation probe returned an unexpected result: $result"
        }
        return [pscustomobject]@{
            Path = $Matches.path.Trim()
            Hash = $Matches.hash
        }
    }
    finally {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Get-AtomWorkEvidence {
    param([Parameter(Mandatory = $true)][string]$ProviderText)

    $machineText = [regex]::Replace($ProviderText, '\r?\n[ \t]+', '')
    if ($machineText -notmatch '(?m)^provider execution ok: .* model=(?<model>\S+) work_plan=(?<id>work-[0-9a-f]{24}) packets=(?<count>\d+) executed=\d+ resumed=\d+') {
        throw "provider output is missing meticulous work-plan execution evidence"
    }
    $planId = $Matches.id
    $model = $Matches.model
    $packetCount = [int]$Matches.count
    if ($packetCount -lt 19) {
        throw "provider work plan is too coarse: $packetCount packets"
    }
    if ($machineText -notmatch '(?m)^provider work manifest: (?<manifest>.+)$') {
        throw "provider output is missing the expanded work manifest path"
    }
    $manifest = $Matches.manifest.Trim()
    if (-not (Test-Path -LiteralPath $manifest)) {
        throw "provider work manifest does not exist: $manifest"
    }
    return [pscustomobject]@{
        PlanId = $planId
        Model = $model
        Manifest = $manifest
        PacketCount = $packetCount
    }
}

function Get-AtomLearningContext {
    param(
        [Parameter(Mandatory = $true)][string]$Intent,
        [Parameter(Mandatory = $true)][string]$Atoms,
        [int]$Limit = 6
    )

    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-context-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    try {
        $intentFile = Join-Path $tempDir "intent.txt"
        [System.IO.File]::WriteAllText($intentFile, $Intent)
        $result = Invoke-AtomLearningProbe -Arguments @(
            "context", "--intent-file", $intentFile,
            "--atoms", $Atoms,
            "--limit", $Limit.ToString()
        )
        return $result
    }
    finally {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
