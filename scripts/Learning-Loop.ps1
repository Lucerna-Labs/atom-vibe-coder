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
