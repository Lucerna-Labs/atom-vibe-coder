$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$StoreDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-launch-env-" + [Guid]::NewGuid().ToString("N"))
$ProofPath = Join-Path $StoreDir "MathAtomsCoder\proofs.jsonl"
$SessionKeyName = "MATH_ATOMS_LAUNCH_SESSION_KEY"
$ExpectedModel = "session-only-launch-model"
$ExpectedEndpoint = "http://127.0.0.1:9/v1/responses"
$saved = @{
    Store = $env:MATH_ATOMS_STORE_DIR
    Kind = $env:MATH_ATOMS_PROVIDER_KIND
    Format = $env:MATH_ATOMS_PROVIDER_FORMAT
    Model = $env:MATH_ATOMS_PROVIDER_MODEL
    Url = $env:MATH_ATOMS_PROVIDER_URL
    KeyEnv = $env:MATH_ATOMS_PROVIDER_KEY_ENV
    Key = [Environment]::GetEnvironmentVariable($SessionKeyName, "Process")
}

if (-not ("MathAtomsLaunchEnvironment" -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MathAtomsLaunchEnvironment {
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint message, UIntPtr wParam, IntPtr lParam);
}
'@
}

. (Join-Path $PSScriptRoot "Native-Process.ps1")

try {
    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force
    $env:MATH_ATOMS_STORE_DIR = $StoreDir
    $env:MATH_ATOMS_PROVIDER_KIND = "custom"
    $env:MATH_ATOMS_PROVIDER_FORMAT = "responses"
    $env:MATH_ATOMS_PROVIDER_MODEL = $ExpectedModel
    $env:MATH_ATOMS_PROVIDER_URL = $ExpectedEndpoint
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = $SessionKeyName
    [Environment]::SetEnvironmentVariable($SessionKeyName, "session-only-test-key", "Process")

    & (Join-Path $PSScriptRoot "Launch-Native.ps1") -Restart
    $proc = Get-Process -Name math-atoms-native -ErrorAction Stop |
        Sort-Object StartTime -Descending |
        Select-Object -First 1
    $handle = Get-AtomNativeWindowHandle -Process $proc
    if ($handle -eq [IntPtr]::Zero) {
        throw "session environment launch did not create the native window"
    }
    if (-not [MathAtomsLaunchEnvironment]::PostMessage($handle, 0x804A, [UIntPtr]::new(2), [IntPtr]::Zero)) {
        throw "session environment launch could not post the Run command"
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 250
    } while (-not (Test-Path -LiteralPath $ProofPath) -and [DateTime]::UtcNow -lt $deadline)
    if (-not (Test-Path -LiteralPath $ProofPath)) {
        throw "detached app did not inherit the session-only store path"
    }
    # Run chains provider execution, so later records may already be blocked against
    # this gate's unreachable endpoint; the FIRST record is the prepared route.
    $proof = (Get-Content -LiteralPath $ProofPath -TotalCount 1) | ConvertFrom-Json
    if ($proof.provider_model -ne $ExpectedModel) {
        throw "detached app lost session-only provider model: $($proof.provider_model)"
    }
    if ($proof.provider_endpoint -ne $ExpectedEndpoint) {
        throw "detached app lost session-only provider endpoint: $($proof.provider_endpoint)"
    }
    if ($proof.status -ne "provider pending") {
        throw "detached app did not prepare the inherited provider route: $($proof.status)"
    }
    Write-Host "native launch environment ok: pid=$($proc.Id) model=$($proof.provider_model) status=$($proof.status)"
}
finally {
    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force
    Remove-Item -LiteralPath $StoreDir -Recurse -Force -ErrorAction SilentlyContinue
    $env:MATH_ATOMS_STORE_DIR = $saved.Store
    $env:MATH_ATOMS_PROVIDER_KIND = $saved.Kind
    $env:MATH_ATOMS_PROVIDER_FORMAT = $saved.Format
    $env:MATH_ATOMS_PROVIDER_MODEL = $saved.Model
    $env:MATH_ATOMS_PROVIDER_URL = $saved.Url
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = $saved.KeyEnv
    [Environment]::SetEnvironmentVariable($SessionKeyName, $saved.Key, "Process")
}
