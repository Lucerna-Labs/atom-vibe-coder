$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Artifact = Join-Path $Engine "math_atoms_coder.bmp"

Push-Location $Root
try {
    node --check app\app-data.js
    node --check app\app.js
    node --check scripts\smoke.mjs
    node scripts\smoke.mjs

    Push-Location $Engine
    try {
        cargo fmt --check
        cargo test --workspace
        cargo clippy --workspace --all-targets -- -D warnings
        cargo run -p pmre-orchestrator --example math_atoms_coder --release
    }
    finally {
        Pop-Location
    }

    if (-not (Test-Path -LiteralPath $Artifact)) {
        throw "Missing native artifact: $Artifact"
    }

    $bytes = [System.IO.File]::ReadAllBytes($Artifact)
    if ($bytes.Length -lt 54) {
        throw "Native artifact is too small to be a valid BMP: $Artifact"
    }
    if ($bytes[0] -ne 0x42 -or $bytes[1] -ne 0x4D) {
        throw "Native artifact does not have a BMP header: $Artifact"
    }

    $width = [BitConverter]::ToInt32($bytes, 18)
    $height = [BitConverter]::ToInt32($bytes, 22)
    if ($width -ne 1440 -or $height -ne 960) {
        throw "Unexpected native artifact dimensions: ${width}x${height}"
    }

    Write-Host "production verification ok: static app, Rust tests, clippy, and native artifact"
}
finally {
    Pop-Location
}
