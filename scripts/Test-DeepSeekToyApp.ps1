$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$OutDir = Join-Path $Engine "target\deepseek-toy-app"
$Source = Join-Path $OutDir "main.rs"
$Exe = Join-Path $OutDir "toy_app.exe"
$ResponseText = Join-Path $OutDir "provider-response.txt"
$Expected = "MATH_ATOMS_DEEPSEEK_TOY_OK atoms=3 proof=pass"
$Endpoint = "https://api.deepseek.com/chat/completions"
$Model = "deepseek-v4-flash"
. (Join-Path $PSScriptRoot "Learning-Loop.ps1")
$LearningIntent = "Build a dependency-free Rust CounterApp toy application with an executable three-atom proof"

Add-Type -AssemblyName System.Net.Http

if ($Model -like "*pro*") {
    throw "DeepSeek toy app test is configured for a Pro model, expected Flash"
}

try {
$Key = [Environment]::GetEnvironmentVariable("DEEPSEEK_API_KEY", "Process")
if ([string]::IsNullOrWhiteSpace($Key)) {
    $Key = [Environment]::GetEnvironmentVariable("DEEPSEEK_API_KEY", "User")
}
if ([string]::IsNullOrWhiteSpace($Key)) {
    $Key = [Environment]::GetEnvironmentVariable("DEEPSEEK_API_KEY", "Machine")
}
if ([string]::IsNullOrWhiteSpace($Key)) {
    throw "Missing DEEPSEEK_API_KEY"
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$PriorLearning = Get-AtomLearningContext -Intent $LearningIntent -Atoms "scan,project,compose,measure,preserve,order" -Limit 4

$Prompt = @"
Return exactly one fenced rust code block and no other text.

Build a tiny dependency-free Rust toy app that proves this model can generate runnable code.
Requirements:
- single source file, Rust standard library only
- define a CounterApp struct with an atoms vector
- include a proof_passes method that checks there are exactly 3 atoms
- main must print exactly:
$Expected
"@
if ($PriorLearning -notmatch 'hits=0') {
    $Prompt += "`nDurable correction evidence from earlier real gates:`n$PriorLearning"
}

$Body = @{
    model = $Model
    messages = @(
        @{
            role = "system"
            content = "You generate small, compiling Rust programs and obey exact output contracts."
        },
        @{
            role = "user"
            content = $Prompt
        }
    )
    thinking = @{
        type = "disabled"
    }
    temperature = 0.1
    stream = $false
} | ConvertTo-Json -Depth 10 -Compress

$Client = [System.Net.Http.HttpClient]::new()
try {
    $Request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, $Endpoint)
    $Request.Headers.Authorization = [System.Net.Http.Headers.AuthenticationHeaderValue]::new("Bearer", $Key.Trim())
    $Request.Content = [System.Net.Http.StringContent]::new($Body, [System.Text.Encoding]::UTF8, "application/json")
    $Response = $Client.SendAsync($Request).GetAwaiter().GetResult()
    $Raw = $Response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
    if (-not $Response.IsSuccessStatusCode) {
        $Redacted = $Raw.Replace($Key.Trim(), "[redacted]")
        throw "DeepSeek request failed with HTTP $([int]$Response.StatusCode): $Redacted"
    }
}
finally {
    $Client.Dispose()
}

$Json = $Raw | ConvertFrom-Json
$Content = [string]$Json.choices[0].message.content
if ([string]::IsNullOrWhiteSpace($Content)) {
    throw "DeepSeek response did not include message content"
}
[System.IO.File]::WriteAllText($ResponseText, $Content)

$Match = [regex]::Match(
    $Content,
    '```(?:rust)?\s*(?<code>[\s\S]*?)```',
    [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
)
if (-not $Match.Success) {
    throw "DeepSeek response did not contain a fenced Rust code block"
}

$Code = $Match.Groups["code"].Value.Trim()
if ($Code -notmatch "fn\s+main\s*\(") {
    throw "Generated toy app is missing fn main"
}
if ($Code -notmatch "CounterApp") {
    throw "Generated toy app is missing CounterApp"
}
[System.IO.File]::WriteAllText($Source, $Code)

Push-Location $OutDir
try {
    rustc --edition=2021 $Source -o $Exe
    if ($LASTEXITCODE -ne 0) {
        throw "rustc failed for DeepSeek-generated toy app with exit code $LASTEXITCODE"
    }
    $Output = (& $Exe) -join "`n"
    $Output = $Output.Trim()
    if ($Output -ne $Expected) {
        throw "DeepSeek-generated toy app output mismatch. Expected '$Expected' but got '$Output'"
    }
    Write-AtomLearningRecord -Source "deepseek-toy-app" -Intent $LearningIntent -Recipe "provider-model-loop" -Atoms "scan,project,compose,measure,preserve,order" -Gate "deepseek-toy-app" -Attempt 1 -Outcome "succeeded" -Correction $PriorLearning -Artifact $Source -ProviderModel $Model
    Write-Host "deepseek toy app ok: generated, compiled, ran: $Output"
}
finally {
    Pop-Location
}
}
catch {
    $failure = $_.Exception.Message
    Write-AtomLearningRecord -Source "deepseek-toy-app" -Intent $LearningIntent -Recipe "provider-model-loop" -Atoms "scan,project,compose,measure,preserve,order" -Gate "deepseek-toy-app" -Attempt 1 -Outcome "failed" -Failure $failure -ProviderModel $Model
    throw
}
