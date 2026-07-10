param(
    [int]$AppsRequired = 3,
    [int]$MaxAttempts = 6,
    [string]$OutputRoot = ""
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$SharedOutDir = [System.IO.Path]::GetFullPath((Join-Path $Engine "target\provider-built-apps"))
$IsolatedOutput = -not [string]::IsNullOrWhiteSpace($OutputRoot)
if ($IsolatedOutput) {
    $OutDir = [System.IO.Path]::GetFullPath($OutputRoot)
    if ($OutDir -eq [System.IO.Path]::GetPathRoot($OutDir)) {
        throw "OutputRoot cannot be a filesystem root"
    }
    if ((Test-Path -LiteralPath $OutDir) -and @(Get-ChildItem -LiteralPath $OutDir -Force).Count -gt 0) {
        throw "OutputRoot must be new or empty: $OutDir"
    }
    $Manifest = Join-Path $OutDir "artifact-window.tsv"
}
else {
    $OutDir = Join-Path $SharedOutDir ("runs\run-" + [Guid]::NewGuid().ToString("N"))
    $Manifest = Join-Path $SharedOutDir "artifact-window.tsv"
}
$OriginalProbeIntent = $env:MATH_ATOMS_PROVIDER_PROBE_INTENT
$OriginalTemplate = $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE
. (Join-Path $PSScriptRoot "Learning-Loop.ps1")
. (Join-Path $PSScriptRoot "Artifact-Manifest.ps1")

$ProviderKind = if ([string]::IsNullOrWhiteSpace($env:MATH_ATOMS_PROVIDER_KIND)) { "openai" } else { $env:MATH_ATOMS_PROVIDER_KIND }
$ProviderModel = $env:MATH_ATOMS_PROVIDER_MODEL

$Specs = @(
    @{
        Name = "counter"
        Struct = "CounterApp"
        Expected = "MATH_ATOMS_APP_OK counter total=4 stack=canonical"
        Requirements = @(
            "store a vector of exactly four atom names",
            "compute the total through a method on CounterApp"
        )
    },
    @{
        Name = "todo"
        Struct = "TodoApp"
        Expected = "MATH_ATOMS_APP_OK todo open=2 done=1 stack=canonical"
        Requirements = @(
            "store three task records with done flags",
            "the task record must contain only one field named done with type bool",
            "do not include description, name, title, label, text, or String fields",
            "compute open and done counts through methods on TodoApp"
        )
    },
    @{
        Name = "router"
        Struct = "RouterApp"
        Expected = "MATH_ATOMS_APP_OK router health=200 atoms=3 stack=canonical"
        Requirements = @(
            "route /health to status 200",
            "define an Atom record with id: u32 and name: &'static str",
            "route /atoms by returning a collection with exactly three atoms",
            "compute the atom count through a RouterApp method that iterates the atoms and reads both Atom.id and Atom.name",
            "do not compute the atom count with atoms.len() alone"
        )
    }
)

if ($AppsRequired -lt 1 -or $AppsRequired -gt $Specs.Count) {
    throw "AppsRequired must be between 1 and $($Specs.Count)"
}
if ($MaxAttempts -lt 1) {
    throw "MaxAttempts must be at least 1"
}

function New-AppIntent($Spec, [string]$FailureEvidence) {
    $requirements = ($Spec.Requirements | ForEach-Object { "- $_" }) -join "`n"
    $intent = @"
provider model build a complete tiny dependency-free Rust console app through Atom Vibe Coder.
Return exactly one fenced rust code block and no prose.
The generated source must be one file, Rust standard library only, deterministic, and compile with:
rustc --edition=2021 -D warnings
Do not use external crates, files, network, stdin, timers, unsafe, or platform APIs.
No compiler warnings are allowed: every field, method, import, variable, and struct must be used.
Constructing a struct or deriving Debug is not enough to count as field usage; executable logic must read every field.
Define a struct named $($Spec.Struct).
Define a const ATOM_STACK with this exact ordered stack: scan -> project -> compose -> measure -> preserve -> order.
Before printing, validate the stack in executable logic; a shuffled or missing atom stack must not pass.
fn main must print exactly:
$($Spec.Expected)
Implementation requirements:
$requirements
"@
    if (-not [string]::IsNullOrWhiteSpace($FailureEvidence)) {
        $intent += @"

Previous attempt failed. Correct the app and return a fresh complete fenced rust code block.
Failure evidence:
$FailureEvidence
"@
    }
    return $intent
}

function Invoke-ProviderProbe($Intent, $AppDir) {
    $env:MATH_ATOMS_PROVIDER_PROBE_INTENT = $Intent
    Push-Location $Engine
    try {
        $oldErrorActionPreference = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            $output = & cargo run --quiet -p math-atoms-core --example provider_probe --release 2>&1
            $exit = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $oldErrorActionPreference
        }
    }
    finally {
        Pop-Location
    }
    $text = ($output | Out-String)
    [System.IO.File]::WriteAllText((Join-Path $AppDir "provider-output.txt"), $text)
    if ($exit -ne 0) {
        throw "provider probe failed with exit code $exit. Output: $text"
    }
    return $text
}

function Get-RustCode($ProviderText) {
    $artifactText = Get-AtomProviderArtifactText -ProviderText $ProviderText
    if (-not [string]::IsNullOrWhiteSpace($artifactText)) {
        return $artifactText
    }
    $matches = [regex]::Matches(
        $ProviderText,
        '```(?:rust)?\s*(?<code>[\s\S]*?)```',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    if ($matches.Count -eq 0) {
        throw "provider output did not contain a fenced Rust code block"
    }
    return $matches[$matches.Count - 1].Groups["code"].Value.Trim()
}

function Assert-CanonicalStackCode([string]$Code, [string]$Name) {
    $stackStart = $Code.IndexOf("ATOM_STACK", [System.StringComparison]::OrdinalIgnoreCase)
    if ($stackStart -lt 0) {
        throw "$Name app is missing ATOM_STACK"
    }
    $required = @("scan", "project", "compose", "measure", "preserve", "order")
    $last = $stackStart
    foreach ($atom in $required) {
        $idx = $Code.IndexOf($atom, $stackStart, [System.StringComparison]::OrdinalIgnoreCase)
        if ($idx -lt 0) {
            throw "$Name app ATOM_STACK is missing $atom"
        }
        if ($idx -le $last) {
            throw "$Name app ATOM_STACK is not canonical order"
        }
        $last = $idx
    }
}

function Invoke-Rustc($Source, $Exe, $AppDir) {
    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $output = & rustc --edition=2021 -D warnings $Source -o $Exe 2>&1
        $exit = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $oldErrorActionPreference
    }
    $text = ($output | Out-String)
    [System.IO.File]::WriteAllText((Join-Path $AppDir "rustc-output.txt"), $text)
    if ($exit -ne 0) {
        throw "rustc exit $exit`n$text"
    }
}

try {
    if ($ProviderKind -match "deepseek") {
        $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = ""
    }

    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Manifest) | Out-Null

    $passed = @()
    $manifestRows = @("name`tstatus`toutput`tsource`texe`tartifact")
    foreach ($spec in $Specs | Select-Object -First $AppsRequired) {
        $durableCorrection = Get-AtomLearningContext -Intent (New-AppIntent $spec "") -Atoms "scan,project,compose,measure,preserve,order" -Limit 4
        if ($durableCorrection -match 'hits=0') { $durableCorrection = "" }
        $lastFailure = ""
        $passedApp = $false
        for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
            $appDir = Join-Path $OutDir ("{0}-attempt-{1}" -f $spec.Name, $attempt)
            New-Item -ItemType Directory -Force -Path $appDir | Out-Null
            $source = Join-Path $appDir "main.rs"
            $exe = Join-Path $appDir "$($spec.Name).exe"
            $attemptIntent = New-AppIntent $spec $lastFailure
            try {
                $providerText = Invoke-ProviderProbe $attemptIntent $appDir
                $work = Get-AtomWorkEvidence -ProviderText $providerText
                $code = Get-RustCode $providerText
                if ($code -notmatch "fn\s+main\s*\(") {
                    throw "$($spec.Name) app is missing fn main"
                }
                if ($code -notmatch [regex]::Escape($spec.Struct)) {
                    throw "$($spec.Name) app is missing $($spec.Struct)"
                }
                Assert-CanonicalStackCode $code $spec.Name
                [System.IO.File]::WriteAllText($source, $code)

                Push-Location $appDir
                try {
                    Invoke-Rustc $source $exe $appDir
                    $actual = ((& $exe) -join "`n").Trim()
                }
                finally {
                    Pop-Location
                }
                if ($actual -ne $spec.Expected) {
                    throw "output mismatch. Expected '$($spec.Expected)' but got '$actual'"
                }
                $attestation = New-AtomHarnessAttestation -HarnessId "rust-console-exact-v1" -Gate "app-$($spec.Name)" -Artifact $source -Executable $exe -ExpectedOutput $spec.Expected -AttestationPath (Join-Path $appDir "harness-attestation.json") -WorkingDirectory $appDir -WorkPlanId $work.PlanId -ProviderModel $work.Model
                $correctionEvidence = if ([string]::IsNullOrWhiteSpace($lastFailure)) { $durableCorrection } else { $lastFailure }
                Write-AtomLearningRecord -Source "provider-multi-app" -Intent $attemptIntent -Recipe "provider-model-loop" -Atoms "scan,project,compose,measure,preserve,order" -Gate "app-$($spec.Name)" -Attempt $attempt -Outcome "succeeded" -Correction $correctionEvidence -Artifact $source -ProviderModel $work.Model -WorkPlanId $work.PlanId -WorkPlanManifest $work.Manifest -WorkPacketCount $work.PacketCount -CandidateVerificationManifest $work.CandidateManifest -CandidateVerificationHash $work.CandidateHash -CandidateBundleHash $work.CandidateBundleHash -CandidateAttempts $work.CandidateAttempts -CandidateRepairs $work.CandidateRepairs -HarnessAttestation $attestation.Path -HarnessAttestationHash $attestation.Hash
                $passed += "$($spec.Name)=$actual"
                $manifestRows += "$($spec.Name)`tcompiled`t$actual`t$source`t$exe`t"
                $passedApp = $true
                break
            }
            catch {
                $lastFailure = $_.Exception.Message
                Write-AtomLearningRecord -Source "provider-multi-app" -Intent $attemptIntent -Recipe "provider-model-loop" -Atoms "scan,project,compose,measure,preserve,order" -Gate "app-$($spec.Name)" -Attempt $attempt -Outcome "failed" -Failure $lastFailure -ProviderModel $ProviderModel
                if ($attempt -eq $MaxAttempts) {
                    throw "$($spec.Name) app failed after $MaxAttempts attempts. Last failure: $lastFailure"
                }
            }
        }
        if (-not $passedApp) {
            throw "$($spec.Name) app did not pass"
        }
    }

    if ($IsolatedOutput) {
        [System.IO.File]::WriteAllLines($Manifest, $manifestRows)
    }
    else {
        foreach ($row in $manifestRows | Select-Object -Skip 1) {
            $fields = $row -split "`t", 6
            Update-AtomArtifactManifest -Path $Manifest -Name $fields[0] -Status $fields[1] -Output $fields[2] -Source $fields[3] -Exe $fields[4] -Artifact $fields[5]
        }
    }
    Write-Host "provider multi-app build ok: $($passed -join '; ')"
}
finally {
    $env:MATH_ATOMS_PROVIDER_PROBE_INTENT = $OriginalProbeIntent
    $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = $OriginalTemplate
}
