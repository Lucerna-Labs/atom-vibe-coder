$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Exe = Join-Path $Engine "target\release\math-atoms-native.exe"
$TestRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("atom-native-vibe-" + [Guid]::NewGuid().ToString("N"))
$VibeRoot = Join-Path $TestRoot "vibe-runtime"
$Original = @{}
foreach ($name in @(
    "MATH_ATOMS_STORE_DIR",
    "MATH_ATOMS_VIBE_RUNTIME_DIR",
    "MATH_ATOMS_PROVIDER_KIND",
    "MATH_ATOMS_PROVIDER_FORMAT",
    "MATH_ATOMS_PROVIDER_URL",
    "MATH_ATOMS_PROVIDER_MODEL",
    "MATH_ATOMS_PROVIDER_KEY_ENV",
    "MATH_ATOMS_PROVIDER_THINKING_LEVEL",
    "MATH_ATOMS_VIBE_TEST_KEY"
)) {
    $Original[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}

function Get-FreePort {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = $listener.LocalEndpoint.Port
    $listener.Stop()
    return $port
}

$Port = Get-FreePort
$ServerJob = $null
. (Join-Path $PSScriptRoot "Native-Process.ps1")

try {
    $env:MATH_ATOMS_STORE_DIR = $TestRoot
    $env:MATH_ATOMS_VIBE_RUNTIME_DIR = $VibeRoot
    $env:MATH_ATOMS_PROVIDER_KIND = "custom"
    $env:MATH_ATOMS_PROVIDER_FORMAT = "chat"
    $env:MATH_ATOMS_PROVIDER_URL = "http://127.0.0.1:$Port/v1/chat/completions"
    $env:MATH_ATOMS_PROVIDER_MODEL = "qwen3.5-9b-q8-test"
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = "MATH_ATOMS_VIBE_TEST_KEY"
    $env:MATH_ATOMS_PROVIDER_THINKING_LEVEL = "low"
    $env:MATH_ATOMS_VIBE_TEST_KEY = "test-key"

    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force
    Push-Location $Engine
    try {
        $env:RUSTFLAGS = "-D warnings"
        cargo build -p math-atoms-native --release
        if ($LASTEXITCODE -ne 0) { throw "native build failed with exit code $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }

    $ServerJob = Start-Job -ScriptBlock {
        param([int]$Port)
        $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
        $listener.Start()
        try {
            # Serve every request with the same canned intake turn until the job is
            # stopped: Run now chains provider execution, so the app may connect
            # more than once before the Vibe Step makes its own call.
            while ($true) {
            $client = $listener.AcceptTcpClient()
            try {
                $stream = $client.GetStream()
                $reader = [System.IO.StreamReader]::new($stream, [System.Text.Encoding]::UTF8, $false, 4096, $true)
                $contentLength = 0
                while ($true) {
                    $line = $reader.ReadLine()
                    if ($null -eq $line -or $line.Length -eq 0) { break }
                    if ($line -match '^Content-Length:\s*(\d+)$') { $contentLength = [int]$Matches[1] }
                }
                $buffer = New-Object char[] $contentLength
                $read = 0
                while ($read -lt $contentLength) {
                    $count = $reader.Read($buffer, $read, $contentLength - $read)
                    if ($count -le 0) { break }
                    $read += $count
                }
                Start-Sleep -Seconds 2
                $output = @{
                    schema_version = 1
                    build_id = "native-ui-fixture"
                    step = "intake"
                    summary = "requirements captured"
                    payload = @{
                        purpose = "test native Vibe integration"
                        user_behaviors = @("submit a build request")
                        ui_decision = "native PMRE"
                        persistence_decision = "durable local state"
                        external_boundaries = @("test provider")
                        execution_siting = "local"
                        out_of_scope = @("release packaging")
                        definition_of_done = @("turn is persisted with both Spiderweb routes")
                    }
                } | ConvertTo-Json -Depth 8 -Compress
                $body = @{
                    choices = @(@{
                        message = @{
                            reasoning_content = "checked every required intake field and preserved the current skill boundary"
                            content = $output
                        }
                    })
                    usage = @{
                        prompt_tokens = 24
                        completion_tokens = 32
                        completion_tokens_details = @{ reasoning_tokens = 12 }
                    }
                } | ConvertTo-Json -Depth 8 -Compress
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
        }
        finally {
            $listener.Stop()
        }
    } -ArgumentList $Port

    $proc = Start-AtomNativeProcess -FilePath $Exe -WorkingDirectory $Engine
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 250
        $proc = Get-Process -Id $proc.Id -ErrorAction Stop
        $windowHandle = Get-AtomNativeWindowHandle -Process $proc
    } while (($windowHandle -eq 0 -or -not $proc.Responding) -and [DateTime]::UtcNow -lt $deadline)
    if ($windowHandle -eq 0 -or -not $proc.Responding) { throw "native window did not become ready" }

    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class AtomNativeVibeTest {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, UIntPtr wParam, IntPtr lParam);
}
'@ -ErrorAction SilentlyContinue

    function Invoke-NativeCommand([int]$Command) {
        [AtomNativeVibeTest]::PostMessage($windowHandle, 0x804A, [UIntPtr]::new($Command), [IntPtr]::Zero) | Out-Null
    }

    Invoke-NativeCommand 2
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 250
        $proc = Get-Process -Id $proc.Id -ErrorAction Stop
        $title = Get-AtomNativeWindowTitle -Process $proc
    } while ($title -notmatch "vibe:prepared" -and [DateTime]::UtcNow -lt $deadline)
    if ($title -notmatch "vibe:prepared") { throw "Run did not prepare the Vibe intake session. Title: $title" }

    Invoke-NativeCommand 35
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    $sawRunning = $false
    do {
        Start-Sleep -Milliseconds 200
        $proc = Get-Process -Id $proc.Id -ErrorAction Stop
        if (-not $proc.Responding) { throw "native app became unresponsive during Vibe provider execution" }
        $title = Get-AtomNativeWindowTitle -Process $proc
        if ($title -match "vibe:running") { $sawRunning = $true }
    } while ($title -notmatch "vibe:(verification-pending|blocked)" -and [DateTime]::UtcNow -lt $deadline)
    if (-not $sawRunning) { throw "native title never exposed vibe:running" }
    if ($title -notmatch "vibe:verification-pending") { throw "Vibe step did not complete successfully. Title: $title" }

    $session = Get-ChildItem -LiteralPath (Join-Path $VibeRoot "sessions") -Filter "build-*.json" | Select-Object -First 1
    if ($null -eq $session) { throw "Vibe session manifest was not persisted" }
    $buildId = $session.BaseName
    $turns = @(Get-ChildItem -LiteralPath (Join-Path $VibeRoot "turns\$buildId") -Filter "*.json")
    if ($turns.Count -ne 1) { throw "Expected one durable Vibe turn for $buildId, found $($turns.Count)" }
    $turn = Get-Content -LiteralPath $turns[0].FullName -Raw | ConvertFrom-Json
    if (@($turn.context_route).Count -ne 4 -or @($turn.result_route).Count -ne 4) {
        throw "Vibe turn did not persist complete L0-L3 routes: $($turns[0].FullName)"
    }
    if ([string]$turn.thinking_source -eq "") { throw "Vibe turn did not persist thinking evidence" }
    $artifact = Join-Path $VibeRoot ([string]$turn.output_artifact).Replace('/', [IO.Path]::DirectorySeparatorChar)
    if (-not (Test-Path -LiteralPath $artifact)) { throw "Vibe output artifact is missing: $artifact" }

    Write-Host "native Vibe runtime ok: build=$buildId title=$title"
}
finally {
    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force
    if ($null -ne $ServerJob) {
        Stop-Job -Job $ServerJob -ErrorAction SilentlyContinue | Out-Null
        Remove-Job -Job $ServerJob -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $TestRoot -Recurse -Force -ErrorAction SilentlyContinue
    foreach ($name in $Original.Keys) {
        [Environment]::SetEnvironmentVariable($name, $Original[$name], "Process")
    }
}
