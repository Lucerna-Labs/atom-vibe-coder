$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Harness = Join-Path $Root "vibe-coder"
$Limit = 4000
$violations = @()
$reports = @()

$CrateRoots = @($Engine, $Harness)
$CrateRoots | ForEach-Object { Get-ChildItem -Path $_ -Directory } | ForEach-Object {
    $crate = $_
    if (-not (Test-Path -LiteralPath (Join-Path $crate.FullName "Cargo.toml"))) {
        return
    }
    [int]$lines = 0
    Get-ChildItem -Path $crate.FullName -Recurse -Filter *.rs | Where-Object {
        $_.FullName -notmatch "\\target\\"
    } | ForEach-Object {
        [int]$fileLines = 0
        foreach ($line in [System.IO.File]::ReadLines($_.FullName)) {
            $fileLines += 1
        }
        $lines += $fileLines
    }
    $reports += "$($crate.Name):$lines"
    if ($lines -gt $Limit) {
        $violations += "$($crate.Name) has $lines Rust source lines (limit $Limit)"
    }
}

if ($violations.Count -gt 0) {
    throw "Rust crate line cap violation: $($violations -join '; ')"
}

Write-Host "Rust crate line cap ok: $($reports -join ', ')"
