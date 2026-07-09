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
$OriginalStoreDir = $env:MATH_ATOMS_STORE_DIR
$OriginalKind = $env:MATH_ATOMS_PROVIDER_KIND
$OriginalUrl = $env:MATH_ATOMS_PROVIDER_URL
$OriginalModel = $env:MATH_ATOMS_PROVIDER_MODEL
$OriginalKeyEnv = $env:MATH_ATOMS_PROVIDER_KEY_ENV
$OriginalFunctionalKey = $env:MATH_ATOMS_INPUT_FUNCTIONAL_KEY
$TestStoreDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-native-input-" + [Guid]::NewGuid().ToString("N"))
$RunLogDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-native-input-logs-" + [Guid]::NewGuid().ToString("N"))
$StdOutLog = Join-Path $RunLogDir "native.out.log"
$StdErrLog = Join-Path $RunLogDir "native.err.log"
$ExpectedIntent = "native renderer artifact only"

New-Item -ItemType Directory -Path $RunLogDir -Force | Out-Null
$env:MATH_ATOMS_STORE_DIR = $TestStoreDir
$env:MATH_ATOMS_PROVIDER_KIND = "openai"
$env:MATH_ATOMS_PROVIDER_URL = "http://127.0.0.1:9/v1/responses"
$env:MATH_ATOMS_PROVIDER_MODEL = "input-editing-provider"
$env:MATH_ATOMS_PROVIDER_KEY_ENV = "MATH_ATOMS_INPUT_FUNCTIONAL_KEY"
$env:MATH_ATOMS_INPUT_FUNCTIONAL_KEY = "test-key"

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

$code = @'
using System;
using System.Runtime.InteropServices;
public static class MathAtomsNativeInputEditing {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, UIntPtr wParam, IntPtr lParam);
}
'@
Add-Type $code -ErrorAction SilentlyContinue

function Refresh-NativeProcess([string]$Stage) {
    try {
        $refreshed = Get-Process -Id $script:NativePid -ErrorAction Stop
        if ($refreshed.MainWindowHandle -eq 0) {
            throw "Native app lost its main window handle during $Stage"
        }
        return $refreshed
    }
    catch {
        $script:proc.Refresh() | Out-Null
        $stdout = if (Test-Path -LiteralPath $StdOutLog) { Get-Content -LiteralPath $StdOutLog -Raw } else { "" }
        $stderr = if (Test-Path -LiteralPath $StdErrLog) { Get-Content -LiteralPath $StdErrLog -Raw } else { "" }
        throw "Native app exited during $Stage for pid $script:NativePid with exit code $($script:proc.ExitCode). stdout: $stdout stderr: $stderr"
    }
}

function Wait-ForTitlePattern([string]$Pattern, [string]$Stage, [int]$Seconds = 30) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        Start-Sleep -Milliseconds 500
        $script:proc = Refresh-NativeProcess $Stage
        if ($script:proc.MainWindowTitle -match $Pattern) {
            return $script:proc
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "$Stage did not reach expected title pattern '$Pattern'. Title: $($script:proc.MainWindowTitle)"
}

function Focus-NativeWindow() {
    $script:proc = Refresh-NativeProcess "focus"
    [MathAtomsNativeInputEditing]::ShowWindow($script:proc.MainWindowHandle, 5) | Out-Null
    [MathAtomsNativeInputEditing]::SetForegroundWindow($script:proc.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 350
}

function Send-WmChar([int]$Code, [string]$Stage) {
    [MathAtomsNativeInputEditing]::PostMessage($script:proc.MainWindowHandle, 0x0102, [UIntPtr]::new($Code), [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 120
    $script:proc = Refresh-NativeProcess $Stage
    if (-not $script:proc.Responding) {
        throw "Native app stopped responding during $Stage"
    }
}

function Send-KeyDown([int]$Code, [string]$Stage) {
    [MathAtomsNativeInputEditing]::PostMessage($script:proc.MainWindowHandle, 0x0100, [UIntPtr]::new($Code), [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 120
    $script:proc = Refresh-NativeProcess $Stage
    if (-not $script:proc.Responding) {
        throw "Native app stopped responding during $Stage"
    }
}

function Clear-FocusedInput() {
    for ($i = 0; $i -lt 180; $i++) {
        [MathAtomsNativeInputEditing]::PostMessage($script:proc.MainWindowHandle, 0x0102, [UIntPtr]::new(8), [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 3
    }
    Start-Sleep -Milliseconds 350
    $script:proc = Refresh-NativeProcess "clear default intent with backspace"
    if (-not $script:proc.Responding) {
        throw "Native app stopped responding during clear default intent with backspace"
    }
}

$proc = Start-Process -FilePath $Exe -WorkingDirectory $Engine -RedirectStandardOutput $StdOutLog -RedirectStandardError $StdErrLog -PassThru
$NativePid = $proc.Id
$WindowDeadline = [DateTime]::UtcNow.AddSeconds(20)
do {
    Start-Sleep -Milliseconds 250
    try {
        $proc = Get-Process -Id $NativePid -ErrorAction Stop
    }
    catch {
        $stdout = if (Test-Path -LiteralPath $StdOutLog) { Get-Content -LiteralPath $StdOutLog -Raw } else { "" }
        $stderr = if (Test-Path -LiteralPath $StdErrLog) { Get-Content -LiteralPath $StdErrLog -Raw } else { "" }
        throw "Native app exited before creating a main window for pid $NativePid. stdout: $stdout stderr: $stderr"
    }
} while (($proc.MainWindowHandle -eq 0 -or -not $proc.Responding) -and [DateTime]::UtcNow -lt $WindowDeadline)
if ($proc.MainWindowHandle -eq 0) {
    $stdout = if (Test-Path -LiteralPath $StdOutLog) { Get-Content -LiteralPath $StdOutLog -Raw } else { "" }
    $stderr = if (Test-Path -LiteralPath $StdErrLog) { Get-Content -LiteralPath $StdErrLog -Raw } else { "" }
    throw "Native app launched without a main window handle after 20s. stdout: $stdout stderr: $stderr"
}

try {
    Focus-NativeWindow
    Set-Clipboard -Value ($ExpectedIntent + "x")
    Clear-FocusedInput
    Send-WmChar 22 "paste clipboard into intent"
    Send-KeyDown 0x25 "move caret before trailing correction"
    Send-KeyDown 0x2E "delete trailing correction at caret"
    Send-WmChar 1 "select corrected intent"
    Send-WmChar 3 "copy corrected intent"
    $copied = (Get-Clipboard -Raw).TrimEnd("`r", "`n")
    if ($copied -ne $ExpectedIntent) {
        throw "Ctrl+C copied unexpected intent. Expected '$ExpectedIntent', got '$copied'"
    }
    Send-WmChar 24 "cut corrected intent"
    $cut = (Get-Clipboard -Raw).TrimEnd("`r", "`n")
    if ($cut -ne $ExpectedIntent) {
        throw "Ctrl+X copied unexpected intent. Expected '$ExpectedIntent', got '$cut'"
    }
    Send-WmChar 22 "paste cut intent back"
    Send-WmChar 13 "submit edited intent"
    $proc = Wait-ForTitlePattern "native-atom-renderer" "submit edited intent" 20
    Write-Host "native input editing ok: copied='$copied' title=$($proc.MainWindowTitle)"
}
finally {
    if (-not $LeaveRunning) {
        Get-Process -Id $NativePid -ErrorAction SilentlyContinue | Stop-Process -Force
        Remove-Item -LiteralPath $TestStoreDir -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $RunLogDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    $env:MATH_ATOMS_STORE_DIR = $OriginalStoreDir
    $env:MATH_ATOMS_PROVIDER_KIND = $OriginalKind
    $env:MATH_ATOMS_PROVIDER_URL = $OriginalUrl
    $env:MATH_ATOMS_PROVIDER_MODEL = $OriginalModel
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = $OriginalKeyEnv
    $env:MATH_ATOMS_INPUT_FUNCTIONAL_KEY = $OriginalFunctionalKey
}
