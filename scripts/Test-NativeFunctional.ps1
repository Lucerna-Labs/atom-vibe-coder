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
$ArtifactManifest = Join-Path $Engine "target\provider-built-apps\artifact-window.tsv"
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
. (Join-Path $PSScriptRoot "Native-Process.ps1")
New-Item -ItemType Directory -Path $RunLogDir -Force | Out-Null
$env:MATH_ATOMS_STORE_DIR = $TestStoreDir
$env:MATH_ATOMS_PROVIDER_KIND = "openai"
$env:MATH_ATOMS_PROVIDER_URL = "http://127.0.0.1:9/v1/responses"
$env:MATH_ATOMS_PROVIDER_MODEL = "functional-provider"
$env:MATH_ATOMS_PROVIDER_KEY_ENV = "MATH_ATOMS_FUNCTIONAL_KEY"
$env:MATH_ATOMS_FUNCTIONAL_KEY = "test-key"
$CommandApplyProvider = 12
$CommandWorkspaceTab = 21
$CommandSettingsTab = 22
$CommandProviderConnectionsTab = 23
$CommandRuntimeSettingsTab = 24
$CommandDesignUploadTab = 26
$CommandBuildDesignUpload = 29

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

$proc = Start-AtomNativeProcess -FilePath $Exe -WorkingDirectory $Engine -StdOutLog $StdOutLog -StdErrLog $StdErrLog
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
    $windowHandle = Get-AtomNativeWindowHandle -Process $proc
} while (($windowHandle -eq 0 -or -not $proc.Responding) -and [DateTime]::UtcNow -lt $WindowDeadline)
if ($windowHandle -eq 0) {
    $stdout = if (Test-Path -LiteralPath $StdOutLog) { Get-Content -LiteralPath $StdOutLog -Raw } else { "" }
    $stderr = if (Test-Path -LiteralPath $StdErrLog) { Get-Content -LiteralPath $StdErrLog -Raw } else { "" }
    throw "Native app launched without a main window handle after 20s. stdout: $stdout stderr: $stderr"
}
if (-not $proc.Responding) {
    $stdout = if (Test-Path -LiteralPath $StdOutLog) { Get-Content -LiteralPath $StdOutLog -Raw } else { "" }
    $stderr = if (Test-Path -LiteralPath $StdErrLog) { Get-Content -LiteralPath $StdErrLog -Raw } else { "" }
    throw "Native app is not responding after launch. stdout: $stdout stderr: $stderr"
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
    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        if ([MathAtomsNativeFunctional]::PostMessage($Handle, 0x804A, [UIntPtr]::new($Command), [IntPtr]::Zero)) {
            return
        }
        Start-Sleep -Milliseconds 100
    }
    throw "Native command $Command could not be posted to window $Handle"
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

function Get-LearningRecordCount() {
    $path = Join-Path $TestStoreDir "MathAtomsCoder\learning.jsonl"
    if (-not (Test-Path -LiteralPath $path)) {
        return 0
    }
    return @([System.IO.File]::ReadLines($path)).Count
}

function Get-ExpectedArtifactCount() {
    if (-not (Test-Path -LiteralPath $ArtifactManifest)) {
        return 0
    }
    return [Math]::Max(0, @([System.IO.File]::ReadLines($ArtifactManifest)).Count - 1)
}

function Test-DesignArtifactRow() {
    if (-not (Test-Path -LiteralPath $ArtifactManifest)) {
        return $false
    }
    foreach ($line in [System.IO.File]::ReadLines($ArtifactManifest)) {
        if ($line -match '^uploaded-design-app\tcompiled\tMATH_ATOMS_DESIGN_APP_OK uploaded-design-app html=1 css=1 bmp=design-upload-app\.bmp\t') {
            return $true
        }
    }
    return $false
}

function Refresh-NativeProcess([string]$Stage) {
    try {
        $refreshed = Get-Process -Id $script:NativePid -ErrorAction Stop
        if ((Get-AtomNativeWindowHandle -Process $refreshed) -eq 0) {
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

function Wait-ForTitlePattern([string]$Pattern, [string]$Stage, [int]$Seconds = 90) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        Start-Sleep -Seconds 1
        $script:proc = Refresh-NativeProcess $Stage
        if ((Get-AtomNativeWindowTitle -Process $script:proc) -match $Pattern) {
            return $script:proc
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    $title = Get-AtomNativeWindowTitle -Process $script:proc
    throw "$Stage did not reach expected title pattern '$Pattern'. Title: $title"
}

try {
    $expectedArtifactCount = Get-ExpectedArtifactCount
    if ($expectedArtifactCount -gt 0) {
        $proc = Wait-ForTitlePattern "artifacts:$expectedArtifactCount" "side artifact window load" 20
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandSettingsTab
    Start-Sleep -Seconds 1
    $proc = Refresh-NativeProcess "Settings tab command"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "settings-runtime") {
        throw "Settings tab did not expose runtime settings as the default settings panel. Title: $title"
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandRuntimeSettingsTab
    Start-Sleep -Seconds 1
    $proc = Refresh-NativeProcess "Runtime settings tab command"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "settings-runtime") {
        throw "Runtime settings tab did not update native navigation state. Title: $title"
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandProviderConnectionsTab
    Start-Sleep -Seconds 1
    $proc = Refresh-NativeProcess "Provider connections tab command"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "provider-connections") {
        throw "Provider connections tab did not update native navigation state. Title: $title"
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandDesignUploadTab
    Start-Sleep -Seconds 1
    $proc = Refresh-NativeProcess "Design Upload settings tab command"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "design-upload") {
        throw "Design Upload tab did not update native navigation state. Title: $title"
    }

    $designStartDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandBuildDesignUpload
        Start-Sleep -Milliseconds 500
        $proc = Refresh-NativeProcess "Build Design Upload acknowledgement"
        $title = Get-AtomNativeWindowTitle -Process $proc
    } while ($title -match "design:idle" -and [DateTime]::UtcNow -lt $designStartDeadline)
    if ($title -match "design:idle") {
        throw "Build Design Upload command was not acknowledged after retries. Title: $title"
    }
    if ($title -match "design:blocked") {
        throw "Build Design Upload failed before artifact completion. Title: $title"
    }
    if ($title -notmatch "design:built") {
        $proc = Wait-ForTitlePattern "design:built" "Build Design Upload" 120
    }
    if (-not (Test-DesignArtifactRow)) {
        throw "Build Design Upload did not write the uploaded-design-app artifact manifest row"
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandWorkspaceTab
    Start-Sleep -Seconds 1
    $proc = Refresh-NativeProcess "Workspace tab command"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "assistant") {
        throw "Workspace tab did not return to the assistant surface. Title: $title"
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandSettingsTab
    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandProviderConnectionsTab
    Start-Sleep -Seconds 1
    $proc = Refresh-NativeProcess "Provider settings before apply"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "provider-connections") {
        throw "Provider apply path was not on the provider connections settings tab. Title: $title"
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandApplyProvider
    Start-Sleep -Seconds 1
    $proc = Refresh-NativeProcess "Apply Provider"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "provider:(idle|blocked)") {
        throw "Apply Provider control did not update provider setup state. Title: $title"
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) $CommandWorkspaceTab
    Start-Sleep -Seconds 1
    $proc = Refresh-NativeProcess "Workspace tab after provider apply"

    $windowHandle = Get-AtomNativeWindowHandle -Process $proc
    Clear-Intent $windowHandle
    Send-Text $windowHandle "native renderer artifact only"
    Send-WmChar $windowHandle 13
    Start-Sleep -Seconds 2
    $proc = Refresh-NativeProcess "typed native intent"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "native-atom-renderer") {
        throw "Typed native intent did not select native-atom-renderer. Title: $title"
    }

    $windowHandle = Get-AtomNativeWindowHandle -Process $proc
    Clear-Intent $windowHandle
    Send-Text $windowHandle "provider model wiki graph rag from typed input"
    Invoke-NativeCommand $windowHandle 2
    Start-Sleep -Seconds 2
    $proc = Refresh-NativeProcess "Run command"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "provider-model-loop") {
        throw "Typed provider intent did not select provider-model-loop. Title: $title"
    }
    # Run chains the full loop: proof route, then provider execution. Against this
    # gate's unreachable endpoint the chained call must fail closed to provider:blocked.
    if ($title -notmatch "provider:(running|ran|blocked)") {
        $proc = Wait-ForTitlePattern "provider:(running|ran|blocked)" "Run chained provider execution" 30
        $title = Get-AtomNativeWindowTitle -Process $proc
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) 3
    Start-Sleep -Seconds 15
    $proc = Refresh-NativeProcess "Provider command"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "provider:(ran|blocked)") {
        throw "Provider button did not reach ran/blocked state. Title: $title"
    }
    if (-not $proc.Responding) {
        throw "Native app stopped responding after provider action"
    }

    $learningPath = Join-Path $TestStoreDir "MathAtomsCoder\learning.jsonl"
    $beforeCaptureLearning = Get-LearningRecordCount
    if ($beforeCaptureLearning -lt 2) {
        throw "Native runs did not persist both successful and failed learning events. Count: $beforeCaptureLearning"
    }
    $learningText = Get-Content -LiteralPath $learningPath -Raw
    if ($learningText -notmatch '"outcome":"succeeded"' -or $learningText -notmatch '"outcome":"failed"') {
        throw "Native learning ledger did not contain both terminal outcomes: $learningText"
    }

    $beforeCapture = Get-ProofRecordCount
    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) 4
    Start-Sleep -Seconds 2
    $afterCapture = Get-ProofRecordCount
    if ($afterCapture -le $beforeCapture) {
        throw "Capture button did not append a proof record after provider completion/block. Before: $beforeCapture After: $afterCapture"
    }
    $afterCaptureLearning = Get-LearningRecordCount
    if ($afterCaptureLearning -ne $beforeCaptureLearning) {
        throw "Capture duplicated a learning event. Before: $beforeCaptureLearning After: $afterCaptureLearning"
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) 5
    Start-Sleep -Seconds 2
    $proc = Refresh-NativeProcess "Drift command"
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "drift flagged") {
        throw "Drift button did not mark drift. Title: $title"
    }
    if (-not $proc.Responding) {
        throw "Native app stopped responding after drift action"
    }

    Write-Host "native functional ok: $(Get-AtomNativeWindowTitle -Process $proc)"
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
