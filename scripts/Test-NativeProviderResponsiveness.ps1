$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Exe = Join-Path $Engine "target\release\math-atoms-native.exe"
$OriginalStoreDir = $env:MATH_ATOMS_STORE_DIR
$OriginalWorkDir = $env:MATH_ATOMS_WORK_DIR
$OriginalKind = $env:MATH_ATOMS_PROVIDER_KIND
$OriginalUrl = $env:MATH_ATOMS_PROVIDER_URL
$OriginalModel = $env:MATH_ATOMS_PROVIDER_MODEL
$OriginalKeyEnv = $env:MATH_ATOMS_PROVIDER_KEY_ENV
$OriginalFakeKey = $env:MATH_ATOMS_FAKE_KEY
$TestStoreDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-provider-responsive-" + [Guid]::NewGuid().ToString("N"))
$ExpectedProviderOutput = '```text' + "`nslow provider ok`n" + '```'

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
    $env:MATH_ATOMS_WORK_DIR = Join-Path $TestStoreDir "work-packets"
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
            for ($requestIndex = 0; $requestIndex -lt 19; $requestIndex++) {
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
                    $request = ([string]::new($buffer, 0, $read)) | ConvertFrom-Json
                    $prompt = [string]$request.instructions
                    if ($prompt -notmatch '(?m)^Packet id: (?<packet>[^\r\n]+)$') { throw "missing packet id" }
                    $packetId = $Matches.packet.Trim()
                    if ($prompt -notmatch '(?m)^Stage: (?<stage>[^\r\n]+)$') { throw "missing packet stage" }
                    $stage = $Matches.stage.Trim()
                    if ($requestIndex -eq 0) { Start-Sleep -Seconds 5 }
                    if ($stage -eq 'file-manifest') {
                        $content = @{
                            packet_id = $packetId
                            status = "complete"
                            files = @(@{ path = "response.txt"; purpose = "responsive provider proof"; acceptance = @("provider remains responsive") })
                            checks = @("manifest complete")
                            risks = @()
                        } | ConvertTo-Json -Depth 8 -Compress
                    }
                    elseif ($stage -in @('file-implementation', 'file-correction', 'integration-correction', 'final-correction')) {
                        $content = '```text' + "`nslow provider ok`n" + '```'
                    }
                    else {
                        $content = @{
                            packet_id = $packetId
                            status = "complete"
                            result = "responsive fixture completed $stage"
                            checks = @("window remained responsive")
                            risks = @()
                        } | ConvertTo-Json -Depth 6 -Compress
                    }
                    $body = @{ output_text = $content } | ConvertTo-Json -Depth 5 -Compress
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

    . (Join-Path $PSScriptRoot "Native-Process.ps1")
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

    Clear-Intent $windowHandle
    Send-Text $windowHandle "provider model wiki graph rag responsiveness"
    Invoke-NativeCommand $windowHandle 2
    Start-Sleep -Seconds 2
    $proc = Get-Process -Id $proc.Id
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "provider-model-loop") {
        throw "Provider intent did not prepare provider route. Title: $title"
    }

    Invoke-NativeCommand (Get-AtomNativeWindowHandle -Process $proc) 3
    Start-Sleep -Seconds 1
    $proc = Get-Process -Id $proc.Id
    if (-not $proc.Responding) {
        throw "Native app stopped responding during slow provider request"
    }
    $title = Get-AtomNativeWindowTitle -Process $proc
    if ($title -notmatch "provider:running") {
        throw "Native app did not show provider running state during slow request. Title: $title"
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 250
        $proc = Get-Process -Id $proc.Id
        $title = Get-AtomNativeWindowTitle -Process $proc
    } while ($title -notmatch "provider:ran" -and [DateTime]::UtcNow -lt $deadline)

    if ($title -notmatch "provider:ran") {
        throw "Native app did not complete slow provider request. Title: $title"
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
    if ($tail -notmatch '"status":"verification pending"') {
        throw "Slow provider output did not remain pending the real product harness. Tail: $tail"
    }
    if ($tail -notmatch '"provider_model":"fake-responsive-provider"') {
        throw "Slow provider proof did not record provider model. Tail: $tail"
    }
    $proof = $tail | ConvertFrom-Json
    if ($proof.provider_output_hash -notmatch '^sha256:[0-9a-f]{64}$') {
        throw "Slow provider proof did not record output hash. Tail: $tail"
    }
    if (-not (Test-Path -LiteralPath $proof.provider_output_artifact)) {
        throw "Slow provider proof artifact does not exist: $($proof.provider_output_artifact)"
    }
    $actualHash = "sha256:" + (Get-FileHash -LiteralPath $proof.provider_output_artifact -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $proof.provider_output_hash) {
        throw "Slow provider proof artifact hash does not recompute. Tail: $tail"
    }
    $expectedOutputLen = [System.Text.Encoding]::UTF8.GetByteCount($ExpectedProviderOutput)
    if ([int]$proof.provider_output_len -ne $expectedOutputLen) {
        throw "Slow provider proof did not record output length. Tail: $tail"
    }
    if ($proof.work_plan_id -notmatch '^work-[0-9a-f]{24}$' -or [int]$proof.work_packet_count -ne 19) {
        throw "Slow provider proof did not bind the 19-packet meticulous work plan. Tail: $tail"
    }

    Write-Host "native provider responsiveness ok: $(Get-AtomNativeWindowTitle -Process $proc)"
}
finally {
    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force
    if ($null -ne $ServerJob) {
        Stop-Job -Job $ServerJob -ErrorAction SilentlyContinue | Out-Null
        Remove-Job -Job $ServerJob -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $TestStoreDir -Recurse -Force -ErrorAction SilentlyContinue
    $env:MATH_ATOMS_STORE_DIR = $OriginalStoreDir
    $env:MATH_ATOMS_WORK_DIR = $OriginalWorkDir
    $env:MATH_ATOMS_PROVIDER_KIND = $OriginalKind
    $env:MATH_ATOMS_PROVIDER_URL = $OriginalUrl
    $env:MATH_ATOMS_PROVIDER_MODEL = $OriginalModel
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = $OriginalKeyEnv
    $env:MATH_ATOMS_FAKE_KEY = $OriginalFakeKey
}
