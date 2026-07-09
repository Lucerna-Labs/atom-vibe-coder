$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"

Push-Location $Engine
try {
    $env:RUSTFLAGS = "-D warnings"
    cargo run -p math-atoms-core --example provider_probe --release
    if ($LASTEXITCODE -ne 0) { throw "provider execution gate failed with exit code $LASTEXITCODE" }
}
finally {
    Pop-Location
}
