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
$OriginalFunctionalKey = $env:MATH_ATOMS_FUNCTIONAL_KEY
$TestStoreDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-native-functional-" + [Guid]::NewGuid().ToString("N"))
$RunLogDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-native-functional-logs-" + [Guid]::NewGuid().ToString("N"))
$StdOutLog = Join-Path $RunLogDir "native.out.log"
$StdErrLog = Join-Path $RunLogDir "native.err.log"
New-Item -ItemType Directory -Path $RunLogDir -Force | Out-Null
$env:MATH_ATOMS_STORE_DIR = $TestStoreDir
$env:MATH_ATOMS_PROVIDER_KIND = "openai"
$env:MATH_ATOMS_PROVIDER_URL = "http://127.0.0.1:9/v1/responses"
$env:MATH_ATOMS_PROVIDER_MODEL = "functional-provider"
$env:MATH_ATOMS_PROVIDER_KEY_ENV = "MATH_ATOMS_FUNCTIONAL_KEY"
$env:MATH_ATOMS_FUNCTIONAL_KEY = "test-key"

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

$proc = Start-Process -FilePath $Exe -WorkingDirectory $Engine -RedirectStandardOutput $StdOutLog -RedirectStandardError $StdErrLog -PassThru
$NativePid = $proc.Id
Start-Sleep -Seconds 2
$proc = Get-Process -Id $NativePid
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

function Make-LParam([int]$X, [int]$Y) {
    return [IntPtr](($Y -shl 16) -bor ($X -band 0xffff))
}

function Click-NativeControl([IntPtr]$Handle, [int]$X, [int]$Y) {
    $lp = Make-LParam $X $Y
    [MathAtomsNativeFunctional]::PostMessage($Handle, 0x0201, [UIntPtr]::new(1), $lp) | Out-Null
    Start-Sleep -Milliseconds 100
    [MathAtomsNativeFunctional]::PostMessage($Handle, 0x0202, [UIntPtr]::Zero, $lp) | Out-Null
}

function Invoke-NativeCommand([IntPtr]$Handle, [int]$Command) {
    [MathAtomsNativeFunctional]::PostMessage($Handle, 0x804A, [UIntPtr]::new($Command), [IntPtr]::Zero) | Out-Null
}

function Send-WmChar([IntPtr]$Handle, [int]$Code) {
    [MathAtomsNativeFunctional]::PostMessage($Handle, 0x0102, [UIntPtr]::new($Code), [IntPtr]::Zero) | Out-Null
}

function Clear-Intent([IntPtr]$Handle) {
    for ($i = 0; $i -lt 260; $i++) {
        Send-WmChar $Handle 8
    }
}

function Send-Text([IntPtr]$Handle, [string]$Text) {
    foreach ($ch in $Text.ToCharArray()) {
        Send-WmChar $Handle ([int][char]$ch)
    }
}

function Get-ProofRecordCount() {
    $path = Join-Path $TestStoreDir "MathAtomsCoder\proofs.jsonl"
    if (-not (Test-Path -LiteralPath $path)) {
        return 0
    }
    return @([System.IO.File]::ReadLines($path)).Count
}

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

try {
    Invoke-NativeCommand $proc.MainWindowHandle 12
    Start-Sleep -Seconds 1
    $proc = Refresh-NativeProcess "Apply Provider"
    if ($proc.MainWindowTitle -notmatch "provider:(idle|blocked)") {
        throw "Apply Provider control did not update provider setup state. Title: $($proc.MainWindowTitle)"
    }

    Clear-Intent $proc.MainWindowHandle
    Send-Text $proc.MainWindowHandle "native renderer artifact only"
    Send-WmChar $proc.MainWindowHandle 13
    Start-Sleep -Seconds 2
    $proc = Refresh-NativeProcess "typed native intent"
    if ($proc.MainWindowTitle -notmatch "native-atom-renderer") {
        throw "Typed native intent did not select native-atom-renderer. Title: $($proc.MainWindowTitle)"
    }

    Clear-Intent $proc.MainWindowHandle
    Send-Text $proc.MainWindowHandle "provider model wiki graph rag from typed input"
    Invoke-NativeCommand $proc.MainWindowHandle 2
    Start-Sleep -Seconds 2
    $proc = Refresh-NativeProcess "Run command"
    if ($proc.MainWindowTitle -notmatch "proven") {
        throw "Run button did not reach proven state. Title: $($proc.MainWindowTitle)"
    }
    if ($proc.MainWindowTitle -notmatch "provider-model-loop") {
        throw "Typed provider intent did not select provider-model-loop. Title: $($proc.MainWindowTitle)"
    }

    $beforeCapture = Get-ProofRecordCount
    Invoke-NativeCommand $proc.MainWindowHandle 4
    Start-Sleep -Seconds 2
    $afterCapture = Get-ProofRecordCount
    if ($afterCapture -le $beforeCapture) {
        throw "Capture button did not append a proof record. Before: $beforeCapture After: $afterCapture"
    }

    Invoke-NativeCommand $proc.MainWindowHandle 3
    Start-Sleep -Seconds 15
    $proc = Refresh-NativeProcess "Provider command"
    if ($proc.MainWindowTitle -notmatch "provider:(ran|blocked)") {
        throw "Provider button did not reach ran/blocked state. Title: $($proc.MainWindowTitle)"
    }
    if (-not $proc.Responding) {
        throw "Native app stopped responding after provider action"
    }

    Invoke-NativeCommand $proc.MainWindowHandle 5
    Start-Sleep -Seconds 2
    $proc = Refresh-NativeProcess "Drift command"
    if ($proc.MainWindowTitle -notmatch "drift flagged") {
        throw "Drift button did not mark drift. Title: $($proc.MainWindowTitle)"
    }
    if (-not $proc.Responding) {
        throw "Native app stopped responding after drift action"
    }

    Write-Host "native functional ok: $($proc.MainWindowTitle)"
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
    $env:MATH_ATOMS_FUNCTIONAL_KEY = $OriginalFunctionalKey
}
