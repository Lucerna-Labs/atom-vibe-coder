param(
    [int]$AppsRequired = 3,
    [int]$MaxAttempts = 3
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$OutDir = Join-Path $Engine "target\provider-built-apps"
$Manifest = Join-Path $OutDir "artifact-window.tsv"
$OriginalProbeIntent = $env:MATH_ATOMS_PROVIDER_PROBE_INTENT
$OriginalTemplate = $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE

$ProviderKind = if ([string]::IsNullOrWhiteSpace($env:MATH_ATOMS_PROVIDER_KIND)) { "openai" } else { $env:MATH_ATOMS_PROVIDER_KIND }
$ProviderModel = $env:MATH_ATOMS_PROVIDER_MODEL
if ($ProviderKind -match "deepseek" -and $ProviderModel -match "pro") {
    throw "Provider app-build gate is configured for a DeepSeek Pro model; expected Flash"
}

$DeepSeekTemplate = '{"model":{{model_json}},"messages":[{"role":"system","content":"You generate small, dependency-free Rust programs. Return exactly one fenced rust code block and no prose. The code must compile with rustc --edition=2021 -D warnings and print the required exact line."},{"role":"user","content":{{prompt_json}}}],"thinking":{"type":"disabled"},"temperature":0.1,"stream":false}'

$Specs = @(
    @{
        Name = "counter"
        Struct = "CounterApp"
        Expected = "MATH_ATOMS_APP_OK counter total=4"
        Requirements = @(
            "store a vector of exactly four atom names",
            "compute the total through a method on CounterApp"
        )
    },
    @{
        Name = "todo"
        Struct = "TodoApp"
        Expected = "MATH_ATOMS_APP_OK todo open=2 done=1"
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
        Expected = "MATH_ATOMS_APP_OK router health=200 atoms=3"
        Requirements = @(
            "route /health to status 200",
            "route /atoms by returning a collection with exactly three atoms"
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
provider model build a complete tiny dependency-free Rust console app through Math Atoms Coder.
Return exactly one fenced rust code block and no prose.
The generated source must be one file, Rust standard library only, deterministic, and compile with:
rustc --edition=2021 -D warnings
Do not use external crates, files, network, stdin, timers, unsafe, or platform APIs.
No compiler warnings are allowed: every field, method, import, variable, and struct must be used.
Define a struct named $($Spec.Struct).
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
        $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = $DeepSeekTemplate
    }

    Remove-Item -LiteralPath $OutDir -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

    $passed = @()
    $manifestRows = @("name`tstatus`toutput`tsource`texe")
    foreach ($spec in $Specs | Select-Object -First $AppsRequired) {
        $lastFailure = ""
        $passedApp = $false
        for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
            $appDir = Join-Path $OutDir ("{0}-attempt-{1}" -f $spec.Name, $attempt)
            New-Item -ItemType Directory -Force -Path $appDir | Out-Null
            $source = Join-Path $appDir "main.rs"
            $exe = Join-Path $appDir "$($spec.Name).exe"
            try {
                $providerText = Invoke-ProviderProbe (New-AppIntent $spec $lastFailure) $appDir
                $code = Get-RustCode $providerText
                if ($code -notmatch "fn\s+main\s*\(") {
                    throw "$($spec.Name) app is missing fn main"
                }
                if ($code -notmatch [regex]::Escape($spec.Struct)) {
                    throw "$($spec.Name) app is missing $($spec.Struct)"
                }
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
                $passed += "$($spec.Name)=$actual"
                $manifestRows += "$($spec.Name)`tcompiled`t$actual`t$source`t$exe"
                $passedApp = $true
                break
            }
            catch {
                $lastFailure = $_.Exception.Message
                if ($attempt -eq $MaxAttempts) {
                    throw "$($spec.Name) app failed after $MaxAttempts attempts. Last failure: $lastFailure"
                }
            }
        }
        if (-not $passedApp) {
            throw "$($spec.Name) app did not pass"
        }
    }

    [System.IO.File]::WriteAllLines($Manifest, $manifestRows)
    Write-Host "provider multi-app build ok: $($passed -join '; ')"
}
finally {
    $env:MATH_ATOMS_PROVIDER_PROBE_INTENT = $OriginalProbeIntent
    $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = $OriginalTemplate
}
