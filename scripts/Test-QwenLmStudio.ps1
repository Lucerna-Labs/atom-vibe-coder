param(
    [string]$Model = "qwen3.5-9b@q6_k_xl",
    [string]$BaseUri = "http://127.0.0.1:1234"
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$catalog = Invoke-RestMethod -Uri "$BaseUri/api/v1/models" -TimeoutSec 10
$loaded = @($catalog.models) | Where-Object {
    $_.key -eq $Model -and @($_.loaded_instances).Count -gt 0
} | Select-Object -First 1
if ($null -eq $loaded) {
    throw "LM Studio is reachable, but the required model is not loaded: $Model"
}
if (-not $loaded.capabilities.reasoning) {
    throw "Loaded LM Studio model does not report reasoning capability: $Model"
}

$request = @{
    model = $Model
    messages = @(
        @{
            role = "system"
            content = "You are a provider readiness probe. Think briefly and follow the requested output exactly."
        },
        @{
            role = "user"
            content = "Reply with exactly ATOM_QWEN_READY."
        }
    )
    reasoning_effort = "low"
    temperature = 0.1
    stream = $false
    max_tokens = 512
} | ConvertTo-Json -Depth 6 -Compress

$response = Invoke-RestMethod `
    -Method Post `
    -Uri "$BaseUri/v1/chat/completions" `
    -Headers @{ Authorization = "Bearer local-lmstudio" } `
    -ContentType "application/json" `
    -Body $request `
    -TimeoutSec 180
$message = $response.choices[0].message
if ([string]$message.content -ne "ATOM_QWEN_READY") {
    throw "Qwen readiness response was unexpected: $($message.content)"
}
if ([string]::IsNullOrWhiteSpace([string]$message.reasoning_content)) {
    throw "Qwen response omitted reasoning_content required by Atom Vibe"
}

Write-Host "Qwen LM Studio ready: model=$Model quant=$($loaded.quantization.name) context=$($loaded.loaded_instances[0].config.context_length) reasoning_chars=$(([string]$message.reasoning_content).Length)"
