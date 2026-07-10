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
. (Join-Path $PSScriptRoot "Native-Process.ps1")

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

$proc = Start-AtomNativeProcess -FilePath $Exe -WorkingDirectory $Engine
$NativePid = $proc.Id
$WindowDeadline = [DateTime]::UtcNow.AddSeconds(20)
do {
    Start-Sleep -Milliseconds 250
    $proc = Get-Process -Id $NativePid -ErrorAction Stop
    $windowHandle = Get-AtomNativeWindowHandle -Process $proc
} while (($windowHandle -eq 0 -or -not $proc.Responding) -and [DateTime]::UtcNow -lt $WindowDeadline)
if ($windowHandle -eq 0) {
    throw "Native app launched without a main window handle after 20s"
}
if (-not $proc.Responding) {
    throw "Native app is not responding after launch"
}

$title = Get-AtomNativeWindowTitle -Process $proc
Write-Host "native app launched: pid=$($proc.Id) title=$title"
