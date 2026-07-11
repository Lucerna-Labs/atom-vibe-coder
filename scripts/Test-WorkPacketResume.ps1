$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
. (Join-Path $PSScriptRoot "Learning-Loop.ps1")
$TestDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-work-resume-" + [Guid]::NewGuid().ToString("N"))
$Saved = @{}
$Names = @(
    "MATH_ATOMS_STORE_DIR",
    "MATH_ATOMS_LEARNING_STORE",
    "MATH_ATOMS_WORK_DIR",
    "MATH_ATOMS_PROVIDER_KIND",
    "MATH_ATOMS_PROVIDER_FORMAT",
    "MATH_ATOMS_PROVIDER_MODEL",
    "MATH_ATOMS_PROVIDER_URL",
    "MATH_ATOMS_PROVIDER_KEY_ENV",
    "MATH_ATOMS_PROVIDER_RESPONSE_KEY",
    "MATH_ATOMS_PROVIDER_PROBE_INTENT",
    "MATH_ATOMS_RESUME_KEY"
)
foreach ($name in $Names) {
    $Saved[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}

function Get-FreePort {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    $listener.Stop()
    return $port
}

function Invoke-ResumeProbe {
    Push-Location $Engine
    try {
        $output = & cargo run --quiet -p math-atoms-core --example provider_probe --release 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }
    $text = ($output | Out-String -Width 10000).Trim()
    if ($exitCode -ne 0) {
        throw "work resume provider probe failed with exit code ${exitCode}: $text"
    }
    return $text
}

$Port = Get-FreePort
$ServerJob = $null
try {
    New-Item -ItemType Directory -Path $TestDir -Force | Out-Null
    $env:MATH_ATOMS_STORE_DIR = $TestDir
    $env:MATH_ATOMS_LEARNING_STORE = Join-Path $TestDir "learning.jsonl"
    $env:MATH_ATOMS_WORK_DIR = Join-Path $TestDir "work-packets"
    $env:MATH_ATOMS_PROVIDER_KIND = "custom"
    $env:MATH_ATOMS_PROVIDER_FORMAT = "chat"
    $env:MATH_ATOMS_PROVIDER_MODEL = "resume-functional-provider"
    $env:MATH_ATOMS_PROVIDER_URL = "http://127.0.0.1:$Port/v1/chat/completions"
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = "MATH_ATOMS_RESUME_KEY"
    $env:MATH_ATOMS_PROVIDER_RESPONSE_KEY = "content"
    $env:MATH_ATOMS_PROVIDER_PROBE_INTENT = "provider model build a resumable dependency-free proof response"
    $env:MATH_ATOMS_RESUME_KEY = "fixture-key"

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
                    $prompt = [string]$request.messages[0].content
                    if ($prompt -notmatch '(?m)^Packet id: (?<packet>[^\r\n]+)$') { throw "missing packet id" }
                    $packetId = $Matches.packet.Trim()
                    if ($prompt -notmatch '(?m)^Stage: (?<stage>[^\r\n]+)$') { throw "missing packet stage" }
                    $stage = $Matches.stage.Trim()
                    if ($stage -eq "file-manifest") {
                        $content = @{
                            packet_id = $packetId
                            status = "complete"
                            files = @(@{ path = "response.txt"; purpose = "resumable proof"; acceptance = @("resumes without network") })
                            checks = @("manifest complete")
                            risks = @()
                        } | ConvertTo-Json -Depth 8 -Compress
                    }
                    elseif ($stage -in @("file-implementation", "file-correction", "integration-correction", "final-correction")) {
                        $content = '```text' + "`nresumable provider proof`n" + '```'
                    }
                    else {
                        $content = @{
                            packet_id = $packetId
                            status = "complete"
                            result = "resume fixture completed $stage"
                            checks = @("packet contract passed")
                            risks = @()
                        } | ConvertTo-Json -Depth 6 -Compress
                    }
                    $response = @{
                        choices = @(@{ message = @{ content = $content; reasoning_content = "fixture reasoning" } })
                        usage = @{ completion_tokens_details = @{ reasoning_tokens = 8 } }
                    } | ConvertTo-Json -Depth 8 -Compress
                    $body = [System.Text.Encoding]::UTF8.GetBytes($response)
                    $header = [System.Text.Encoding]::ASCII.GetBytes("HTTP/1.1 200 OK`r`nContent-Type: application/json`r`nContent-Length: $($body.Length)`r`nConnection: close`r`n`r`n")
                    $stream.Write($header, 0, $header.Length)
                    $stream.Write($body, 0, $body.Length)
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
    Start-Sleep -Milliseconds 300

    $first = Invoke-ResumeProbe
    $firstWork = Get-AtomWorkEvidence -ProviderText $first
    if ($first -notmatch 'packets=19 executed=19 resumed=0') {
        throw "first work run did not execute all 19 packets: $first"
    }
    $finished = Wait-Job -Job $ServerJob -Timeout 15
    if ($null -eq $finished -or $ServerJob.State -ne "Completed") {
        throw "resume fixture server did not stop after exactly 19 requests"
    }

    $second = Invoke-ResumeProbe
    $secondWork = Get-AtomWorkEvidence -ProviderText $second
    if ($second -notmatch 'packets=19 executed=0 resumed=19') {
        throw "second work run did not resume all 19 packets: $second"
    }
    if ($firstWork.PlanId -ne $secondWork.PlanId -or $firstWork.Manifest -ne $secondWork.Manifest) {
        throw "resumed run changed its plan identity"
    }
    if ($second -notmatch 'resumable provider proof') {
        throw "resumed run did not reconstruct the verified deliverable"
    }
    Write-Host "work packet resume ok: plan=$($secondWork.PlanId) executed=19 then resumed=19 with endpoint offline"
}
finally {
    if ($null -ne $ServerJob) {
        Stop-Job -Job $ServerJob -ErrorAction SilentlyContinue | Out-Null
        Remove-Job -Job $ServerJob -Force -ErrorAction SilentlyContinue
    }
    foreach ($name in $Names) {
        [Environment]::SetEnvironmentVariable($name, $Saved[$name], "Process")
    }
    Remove-Item -LiteralPath $TestDir -Recurse -Force -ErrorAction SilentlyContinue
}
