$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Artifact = Join-Path $Engine "math_atoms_coder.bmp"

Push-Location $Root
try {
    node --check app\app-data.js
    if ($LASTEXITCODE -ne 0) { throw "node --check app\app-data.js failed with exit code $LASTEXITCODE" }
    node --check app\app.js
    if ($LASTEXITCODE -ne 0) { throw "node --check app\app.js failed with exit code $LASTEXITCODE" }
    node --check scripts\smoke.mjs
    if ($LASTEXITCODE -ne 0) { throw "node --check scripts\smoke.mjs failed with exit code $LASTEXITCODE" }
    node scripts\smoke.mjs
    if ($LASTEXITCODE -ne 0) { throw "node scripts\smoke.mjs failed with exit code $LASTEXITCODE" }

    Push-Location $Engine
    try {
        $env:RUSTFLAGS = "-D warnings"
        cargo fmt --check
        if ($LASTEXITCODE -ne 0) { throw "cargo fmt --check failed with exit code $LASTEXITCODE" }
        cargo test --workspace
        if ($LASTEXITCODE -ne 0) { throw "cargo test --workspace failed with exit code $LASTEXITCODE" }
        cargo clippy --workspace --all-targets -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw "cargo clippy failed with exit code $LASTEXITCODE" }
        cargo build -p math-atoms-native --release
        if ($LASTEXITCODE -ne 0) { throw "native PMRE app build failed with exit code $LASTEXITCODE" }
        cargo run -p pmre-examples --example math_atoms_coder --release
        if ($LASTEXITCODE -ne 0) { throw "native artifact render failed with exit code $LASTEXITCODE" }
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

    Write-Host "baseline verification ok: doctrine check, Rust tests, clippy, native app build, and native artifact"
}
finally {
    Pop-Location
}
