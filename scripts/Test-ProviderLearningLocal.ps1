$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$TestDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-local-provider-" + [Guid]::NewGuid().ToString("N"))
$LearningStore = Join-Path $TestDir "learning.jsonl"
$BluetoothSource = Join-Path $Root "artifacts\bluetooth-driver\bluetooth_driver.rs"
$SavedEnvironment = @{}
$EnvironmentNames = @(
    "MATH_ATOMS_STORE_DIR",
    "MATH_ATOMS_LEARNING_STORE",
    "MATH_ATOMS_WORK_DIR",
    "MATH_ATOMS_PROVIDER_KIND",
    "MATH_ATOMS_PROVIDER_FORMAT",
    "MATH_ATOMS_PROVIDER_MODEL",
    "MATH_ATOMS_PROVIDER_URL",
    "MATH_ATOMS_PROVIDER_KEY_ENV",
    "MATH_ATOMS_PROVIDER_AUTH_HEADER",
    "MATH_ATOMS_PROVIDER_AUTH_SCHEME",
    "MATH_ATOMS_PROVIDER_RESPONSE_KEY",
    "MATH_ATOMS_PROVIDER_BODY_TEMPLATE",
    "MATH_ATOMS_LOCAL_PROVIDER_KEY"
)
foreach ($name in $EnvironmentNames) {
    $SavedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}

function Get-FreePort {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    $listener.Stop()
    return $port
}

$CounterSource = @'
const ATOM_STACK: [&str; 6] = ["scan", "project", "compose", "measure", "preserve", "order"];

struct CounterApp {
    atoms: Vec<&'static str>,
}

impl CounterApp {
    fn total(&self) -> usize {
        self.atoms.iter().count()
    }
}

fn main() {
    assert_eq!(ATOM_STACK.join("->"), "scan->project->compose->measure->preserve->order");
    let app = CounterApp { atoms: vec!["scan", "project", "compose", "measure"] };
    println!("MATH_ATOMS_APP_OK counter total={} stack=canonical", app.total());
}
'@

$TaskBoardSpec = @'
{
  "slug": "pmre-task-board",
  "title": "Task Board",
  "kind": "task_board",
  "tasks": ["Write spec", "Build UI", "Test artifact", "Ship build"],
  "done_indices": [0, 2],
  "filter": "open",
  "accent": "teal"
}
'@

$Port = Get-FreePort
$ServerJob = $null
try {
    New-Item -ItemType Directory -Path $TestDir -Force | Out-Null
    if (-not (Test-Path -LiteralPath $BluetoothSource)) {
        throw "local provider fixture is missing Bluetooth source: $BluetoothSource"
    }
    $env:MATH_ATOMS_STORE_DIR = $TestDir
    $env:MATH_ATOMS_LEARNING_STORE = $LearningStore
    $env:MATH_ATOMS_WORK_DIR = Join-Path $TestDir "work-packets"
    $env:MATH_ATOMS_PROVIDER_KIND = "custom"
    $env:MATH_ATOMS_PROVIDER_FORMAT = "chat"
    $env:MATH_ATOMS_PROVIDER_MODEL = "local-functional-provider"
    $env:MATH_ATOMS_PROVIDER_URL = "http://127.0.0.1:$Port/v1/chat/completions"
    $env:MATH_ATOMS_PROVIDER_KEY_ENV = "MATH_ATOMS_LOCAL_PROVIDER_KEY"
    $env:MATH_ATOMS_PROVIDER_AUTH_HEADER = "Authorization"
    $env:MATH_ATOMS_PROVIDER_AUTH_SCHEME = "Bearer"
    $env:MATH_ATOMS_PROVIDER_RESPONSE_KEY = "content"
    $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = ""
    $env:MATH_ATOMS_LOCAL_PROVIDER_KEY = "functional-test-key"

    $ServerJob = Start-Job -ScriptBlock {
        param([int]$Port, [string]$CounterSource, [string]$TaskBoardSpec, [string]$BluetoothSource)
        $bluetooth = [System.IO.File]::ReadAllText($BluetoothSource)
        $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
        $listener.Start()
        try {
            for ($requestIndex = 0; $requestIndex -lt 52; $requestIndex++) {
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
                    $requestBody = [string]::new($buffer, 0, $read)
                    $request = $requestBody | ConvertFrom-Json
                    $prompt = [string]$request.messages[-1].content
                    if ($prompt -notmatch '(?m)^Packet id: (?<packet>[^\r\n]+)$') {
                        throw "work packet prompt is missing Packet id: $prompt"
                    }
                    $packetId = $Matches.packet.Trim()
                    if ($prompt -notmatch '(?m)^Stage: (?<stage>[^\r\n]+)$') {
                        throw "work packet prompt is missing Stage: $prompt"
                    }
                    $stage = $Matches.stage.Trim()
                    $isBluetooth = $prompt -match 'Bluetooth Low Energy HCI'
                    $isTaskBoard = $prompt -match 'product spec' -or $prompt -match 'pmre-task-board'
                    $isCounter = $prompt -match 'CounterApp'
                    if ($stage -eq 'file-manifest') {
                        $file = if ($isBluetooth) {
                            @{ path = "bluetooth_driver.rs"; purpose = "complete Bluetooth HCI driver core"; acceptance = @("compiles and passes the driver behavior gate") }
                        }
                        elseif ($isTaskBoard) {
                            @{ path = "app-spec.json"; purpose = "validated task board product specification"; acceptance = @("matches the task board schema") }
                        }
                        elseif ($isCounter) {
                            @{ path = "main.rs"; purpose = "complete counter console application"; acceptance = @("compiles and prints the exact proof line") }
                        }
                        else {
                            @{ path = "response.txt"; purpose = "provider proof response"; acceptance = @("contains non-empty provider evidence") }
                        }
                        $content = @{
                            packet_id = $packetId
                            status = "complete"
                            files = @($file)
                            checks = @("manifest covers the fixture requirement")
                            risks = @()
                        } | ConvertTo-Json -Depth 8 -Compress
                    }
                    elseif ($stage -in @('file-implementation', 'file-correction')) {
                        if ($isBluetooth) {
                            $content = '```rust' + "`n" + $bluetooth + "`n" + '```'
                        }
                        elseif ($isTaskBoard) {
                            $content = '```json' + "`n" + $TaskBoardSpec + "`n" + '```'
                        }
                        elseif ($isCounter) {
                            $content = '```rust' + "`n" + $CounterSource + "`n" + '```'
                        }
                        else {
                            $content = '```text' + "`nlocal provider proof`n" + '```'
                        }
                    }
                    else {
                        $content = @{
                            packet_id = $packetId
                            status = "complete"
                            result = "fixture completed $stage with explicit evidence"
                            checks = @("deterministic fixture gate passed")
                            risks = @()
                        } | ConvertTo-Json -Depth 6 -Compress
                    }
                    $response = @{ choices = @(@{ message = @{ content = $content } }) } | ConvertTo-Json -Depth 6 -Compress
                    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($response)
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
    } -ArgumentList $Port, $CounterSource, $TaskBoardSpec, $BluetoothSource
    Start-Sleep -Milliseconds 300

    powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderExecution.ps1")
    if ($LASTEXITCODE -ne 0) { throw "local provider execution gate failed with exit code $LASTEXITCODE" }
    powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderBuildSeveralApps.ps1") -AppsRequired 1 -MaxAttempts 1
    if ($LASTEXITCODE -ne 0) { throw "local provider counter gate failed with exit code $LASTEXITCODE" }
    powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderBuildRealPmreApp.ps1") -MaxAttempts 1
    if ($LASTEXITCODE -ne 0) { throw "local provider PMRE gate failed with exit code $LASTEXITCODE" }
    powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "Test-ProviderBuildBluetoothDriver.ps1") -MaxAttempts 1
    if ($LASTEXITCODE -ne 0) { throw "local provider Bluetooth gate failed with exit code $LASTEXITCODE" }

    $job = Wait-Job -Job $ServerJob -Timeout 10
    if ($null -eq $job -or $ServerJob.State -ne "Completed") {
        throw "local provider server did not complete 52 meticulous packet requests; state=$($ServerJob.State)"
    }
    $records = @([System.IO.File]::ReadAllLines($LearningStore))
    if ($records.Count -ne 4) {
        throw "local provider learning gate expected 4 successful records, found $($records.Count)"
    }
    $learningText = $records -join "`n"
    foreach ($source in @("provider-execution", "provider-multi-app", "provider-pmre-app", "provider-bluetooth-driver")) {
        if ($learningText -notmatch [regex]::Escape("`"source`":`"$source`"")) {
            throw "local provider learning ledger is missing source $source"
        }
    }
    if ($learningText -match '"outcome":"failed"') {
        throw "local provider deterministic fixtures unexpectedly required a correction: $learningText"
    }
    Write-Host "local provider learning ok: adapter=1 console-app=1 pmre-app=1 bluetooth-driver=1 learned=4"
}
finally {
    if ($null -ne $ServerJob) {
        Stop-Job -Job $ServerJob -ErrorAction SilentlyContinue | Out-Null
        Remove-Job -Job $ServerJob -Force -ErrorAction SilentlyContinue
    }
    foreach ($name in $EnvironmentNames) {
        [Environment]::SetEnvironmentVariable($name, $SavedEnvironment[$name], "Process")
    }
    Remove-Item -LiteralPath $TestDir -Recurse -Force -ErrorAction SilentlyContinue
}
