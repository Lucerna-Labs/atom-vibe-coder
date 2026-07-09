param(
    [switch]$Build,
    [switch]$Restart
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Exe = Join-Path $Engine "target\release\math-atoms-native.exe"

if ($Restart) {
    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force
}

if ($Build -or -not (Test-Path -LiteralPath $Exe)) {
    Push-Location $Engine
    try {
        $env:RUSTFLAGS = "-D warnings"
        cargo build -p math-atoms-native --release
        if ($LASTEXITCODE -ne 0) { throw "native PMRE app build failed with exit code $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
}

$proc = Start-Process -FilePath $Exe -WorkingDirectory $Engine -PassThru
$NativePid = $proc.Id
$WindowDeadline = [DateTime]::UtcNow.AddSeconds(20)
do {
    Start-Sleep -Milliseconds 250
    $proc = Get-Process -Id $NativePid -ErrorAction Stop
} while (($proc.MainWindowHandle -eq 0 -or -not $proc.Responding) -and [DateTime]::UtcNow -lt $WindowDeadline)
if ($proc.MainWindowHandle -eq 0) {
    throw "Native app launched without a main window handle after 20s"
}
if (-not $proc.Responding) {
    throw "Native app is not responding after launch"
}

Write-Host "native app launched: pid=$($proc.Id) title=$($proc.MainWindowTitle)"
