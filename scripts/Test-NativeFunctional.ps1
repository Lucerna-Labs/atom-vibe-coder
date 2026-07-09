param(
    [switch]$LeaveRunning
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Exe = Join-Path $Engine "target\release\math-atoms-native.exe"

Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force

Push-Location $Engine
try {
    $env:RUSTFLAGS = "-D warnings"
    cargo build -p math-atoms-native --release
    if ($LASTEXITCODE -ne 0) { throw "native PMRE app build failed with exit code $LASTEXITCODE" }
}
finally {
    Pop-Location
}

$proc = Start-Process -FilePath $Exe -WorkingDirectory $Engine -PassThru
Start-Sleep -Seconds 2
$proc = Get-Process -Id $proc.Id
if ($proc.MainWindowHandle -eq 0) {
    throw "Native app launched without a main window handle"
}
if (-not $proc.Responding) {
    throw "Native app is not responding after launch"
}

$code = @'
using System;
using System.Runtime.InteropServices;
public static class MathAtomsNativeFunctional {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, UIntPtr wParam, IntPtr lParam);
}
'@
Add-Type $code -ErrorAction SilentlyContinue

function Send-Enter([IntPtr]$Handle) {
    [MathAtomsNativeFunctional]::PostMessage($Handle, 0x0102, [UIntPtr]::new(13), [IntPtr]::Zero) | Out-Null
}

function Make-LParam([int]$X, [int]$Y) {
    return [IntPtr](($Y -shl 16) -bor ($X -band 0xffff))
}

function Click-Provider([IntPtr]$Handle) {
    $lp = Make-LParam 186 290
    [MathAtomsNativeFunctional]::PostMessage($Handle, 0x0201, [UIntPtr]::new(1), $lp) | Out-Null
    Start-Sleep -Milliseconds 100
    [MathAtomsNativeFunctional]::PostMessage($Handle, 0x0202, [UIntPtr]::Zero, $lp) | Out-Null
}

try {
    Send-Enter $proc.MainWindowHandle
    Start-Sleep -Seconds 2
    $proc = Get-Process -Id $proc.Id
    if ($proc.MainWindowTitle -notmatch "proven") {
        throw "Native proof loop did not reach proven state. Title: $($proc.MainWindowTitle)"
    }

    Click-Provider $proc.MainWindowHandle
    Start-Sleep -Seconds 15
    $proc = Get-Process -Id $proc.Id
    if ($proc.MainWindowTitle -notmatch "provider:(ran|blocked)") {
        throw "Provider button did not reach ran/blocked state. Title: $($proc.MainWindowTitle)"
    }
    if (-not $proc.Responding) {
        throw "Native app stopped responding after provider action"
    }

    Write-Host "native functional ok: $($proc.MainWindowTitle)"
}
finally {
    if (-not $LeaveRunning) {
        Get-Process -Id $proc.Id -ErrorAction SilentlyContinue | Stop-Process -Force
    }
}
