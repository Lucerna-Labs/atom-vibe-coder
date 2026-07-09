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
$OriginalFakeKey = $env:MATH_ATOMS_FAKE_KEY
$TestStoreDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-provider-responsive-" + [Guid]::NewGuid().ToString("N"))

function Get-FreePort() {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = $listener.LocalEndpoint.Port
    $listener.Stop()
    return $port
}

$Port = Get-FreePort
$ServerJob = $null

try {
    $env:MATH_ATOMS_STORE_DIR = $TestStoreDir
    $env:MATH_ATOMS_PROVIDER_KIND = "openai"
    $env:MATH_ATOMS_PROVIDER_URL = "http://127.0.0.1:$Port/v1/responses"
    $env:MATH_ATOMS_PROVIDER_MODEL = "fake-responsive-provider"
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = "MATH_ATOMS_FAKE_KEY"
    $env:MATH_ATOMS_FAKE_KEY = "test-key"

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

    $ServerJob = Start-Job -ScriptBlock {
        param([int]$Port)
        $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
        $listener.Start()
        try {
            $client = $listener.AcceptTcpClient()
            try {
                $stream = $client.GetStream()
                $reader = [System.IO.StreamReader]::new($stream, [System.Text.Encoding]::ASCII, $false, 1024, $true)
                while ($true) {
                    $line = $reader.ReadLine()
                    if ($null -eq $line -or $line.Length -eq 0) { break }
                }
                Start-Sleep -Seconds 5
                $body = '{"output_text":"slow provider ok"}'
                $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
                $header = "HTTP/1.1 200 OK`r`nContent-Type: application/json`r`nContent-Length: $($bodyBytes.Length)`r`nConnection: close`r`n`r`n"
                $headerBytes = [System.Text.Encoding]::ASCII.GetBytes($header)
                $stream.Write($headerBytes, 0, $headerBytes.Length)
                $stream.Write($bodyBytes, 0, $bodyBytes.Length)
                $stream.Flush()
            }
            finally {
                $client.Close()
            }
        }
        finally {
            $listener.Stop()
        }
    } -ArgumentList $Port

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
public static class MathAtomsProviderResponsive {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, UIntPtr wParam, IntPtr lParam);
}
'@
    Add-Type $code -ErrorAction SilentlyContinue

    function Send-WmChar([IntPtr]$Handle, [int]$Code) {
        [MathAtomsProviderResponsive]::PostMessage($Handle, 0x0102, [UIntPtr]::new($Code), [IntPtr]::Zero) | Out-Null
    }
    function Send-Text([IntPtr]$Handle, [string]$Text) {
        foreach ($ch in $Text.ToCharArray()) {
            Send-WmChar $Handle ([int][char]$ch)
        }
    }
    function Clear-Intent([IntPtr]$Handle) {
        for ($i = 0; $i -lt 260; $i++) {
            Send-WmChar $Handle 8
        }
    }
    function Invoke-NativeCommand([IntPtr]$Handle, [int]$Command) {
        [MathAtomsProviderResponsive]::PostMessage($Handle, 0x804A, [UIntPtr]::new($Command), [IntPtr]::Zero) | Out-Null
    }

    Clear-Intent $proc.MainWindowHandle
    Send-Text $proc.MainWindowHandle "provider model wiki graph rag responsiveness"
    Invoke-NativeCommand $proc.MainWindowHandle 2
    Start-Sleep -Seconds 2
    $proc = Get-Process -Id $proc.Id
    if ($proc.MainWindowTitle -notmatch "provider-model-loop") {
        throw "Provider intent did not prepare provider route. Title: $($proc.MainWindowTitle)"
    }

    Invoke-NativeCommand $proc.MainWindowHandle 3
    Start-Sleep -Seconds 1
    $proc = Get-Process -Id $proc.Id
    if (-not $proc.Responding) {
        throw "Native app stopped responding during slow provider request"
    }
    if ($proc.MainWindowTitle -notmatch "provider:running") {
        throw "Native app did not show provider running state during slow request. Title: $($proc.MainWindowTitle)"
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 250
        $proc = Get-Process -Id $proc.Id
    } while ($proc.MainWindowTitle -notmatch "provider:ran" -and [DateTime]::UtcNow -lt $deadline)

    if ($proc.MainWindowTitle -notmatch "provider:ran") {
        throw "Native app did not complete slow provider request. Title: $($proc.MainWindowTitle)"
    }
    if (-not $proc.Responding) {
        throw "Native app stopped responding after slow provider request"
    }

    $store = Join-Path $TestStoreDir "MathAtomsCoder\proofs.jsonl"
    if (-not (Test-Path -LiteralPath $store)) {
        throw "Slow provider run did not write proof store: $store"
    }
    $tail = Get-Content -LiteralPath $store -Tail 1
    if ($tail -notmatch '"provider_state":"provider:ran"') {
        throw "Slow provider proof did not record provider:ran. Tail: $tail"
    }
    if ($tail -notmatch '"status":"proven"') {
        throw "Slow provider proof did not promote status to proven after execution. Tail: $tail"
    }
    if ($tail -notmatch '"provider_model":"fake-responsive-provider"') {
        throw "Slow provider proof did not record provider model. Tail: $tail"
    }
    if ($tail -notmatch '"provider_output_hash":"fnv:[0-9a-f]{16}"') {
        throw "Slow provider proof did not record output hash. Tail: $tail"
    }
    if ($tail -notmatch '"provider_output_len":16') {
        throw "Slow provider proof did not record output length. Tail: $tail"
    }

    Write-Host "native provider responsiveness ok: $($proc.MainWindowTitle)"
}
finally {
    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force
    if ($null -ne $ServerJob) {
        Stop-Job -Job $ServerJob -ErrorAction SilentlyContinue | Out-Null
        Remove-Job -Job $ServerJob -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $TestStoreDir -Recurse -Force -ErrorAction SilentlyContinue
    $env:MATH_ATOMS_STORE_DIR = $OriginalStoreDir
    $env:MATH_ATOMS_PROVIDER_KIND = $OriginalKind
    $env:MATH_ATOMS_PROVIDER_URL = $OriginalUrl
    $env:MATH_ATOMS_PROVIDER_MODEL = $OriginalModel
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = $OriginalKeyEnv
    $env:MATH_ATOMS_FAKE_KEY = $OriginalFakeKey
}
