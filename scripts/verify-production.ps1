param(
    [switch]$AllowProviderBlock
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Artifact = Join-Path $Engine "math_atoms_coder.bmp"

Push-Location $Root
try {
    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force

    powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-RustCrateLineCaps.ps1")
    if ($LASTEXITCODE -ne 0) { throw "Rust crate line cap gate failed with exit code $LASTEXITCODE" }

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

    if ($AllowProviderBlock) {
        Write-Warning "provider execution gate skipped by -AllowProviderBlock; this is not a production-ready verification"
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeFunctional.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native functional gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeIdleCpu.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native idle CPU gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeLaunchEnvironment.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native launch environment gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeInputEditing.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native input editing gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeProviderResponsiveness.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native provider responsiveness gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-SelfLearningFunctional.ps1")
        if ($LASTEXITCODE -ne 0) { throw "self-learning functional gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderLearningLocal.ps1")
        if ($LASTEXITCODE -ne 0) { throw "local provider app-build learning gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-WorkPacketResume.ps1")
        if ($LASTEXITCODE -ne 0) { throw "work packet resume gate failed with exit code $LASTEXITCODE" }
        Write-Host "structural verification ok: Rust doctrine/tests, clippy, native app build, native artifact, native functional gate, native input editing gate, native provider responsiveness gate, restart-level self-learning gate, and local provider adapter/app/PMRE/Bluetooth learning gate"
    }
    else {
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderExecution.ps1")
        if ($LASTEXITCODE -ne 0) { throw "provider execution gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderBuildSeveralApps.ps1")
        if ($LASTEXITCODE -ne 0) { throw "provider multi-app build gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderBuildRealPmreApp.ps1")
        if ($LASTEXITCODE -ne 0) { throw "provider real PMRE app gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderBuildBluetoothDriver.ps1")
        if ($LASTEXITCODE -ne 0) { throw "provider Bluetooth driver gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-DesignUploadBuild.ps1")
        if ($LASTEXITCODE -ne 0) { throw "design upload build gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeFunctional.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native functional gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeIdleCpu.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native idle CPU gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeLaunchEnvironment.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native launch environment gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeInputEditing.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native input editing gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-NativeProviderResponsiveness.ps1")
        if ($LASTEXITCODE -ne 0) { throw "native provider responsiveness gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-SelfLearningFunctional.ps1")
        if ($LASTEXITCODE -ne 0) { throw "self-learning functional gate failed with exit code $LASTEXITCODE" }
        powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-WorkPacketResume.ps1")
        if ($LASTEXITCODE -ne 0) { throw "work packet resume gate failed with exit code $LASTEXITCODE" }
        Write-Host "production verification ok: Rust doctrine/tests, clippy, native app build, native artifact, native functional gate, native input editing gate, native provider responsiveness gate, provider execution gate, provider multi-app build gate, provider real PMRE app gate, provider Bluetooth driver gate, design upload build gate, and restart-level self-learning gate"
    }
}
finally {
    Pop-Location
}
