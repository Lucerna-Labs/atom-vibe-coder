param(
    [switch]$Build,
    [switch]$SkipProbe
)

$ErrorActionPreference = "Stop"
$Model = "qwen3.5-9b@q6_k_xl"
$BaseUri = "http://127.0.0.1:1234"
$LocalKeyName = "MATH_ATOMS_LOCAL_LMSTUDIO_KEY"

if (-not $SkipProbe) {
    & (Join-Path $PSScriptRoot "Test-QwenLmStudio.ps1") -Model $Model -BaseUri $BaseUri
}

$env:MATH_ATOMS_PROVIDER_KIND = "custom"
$env:MATH_ATOMS_PROVIDER_FORMAT = "chat"
$env:MATH_ATOMS_PROVIDER_MODEL = $Model
$env:MATH_ATOMS_PROVIDER_URL = "$BaseUri/v1/chat/completions"
$env:MATH_ATOMS_PROVIDER_KEY_ENV = $LocalKeyName
$env:MATH_ATOMS_LOCAL_LMSTUDIO_KEY = "local-lmstudio"
$env:MATH_ATOMS_PROVIDER_AUTH_HEADER = "Authorization"
$env:MATH_ATOMS_PROVIDER_AUTH_SCHEME = "Bearer"
$env:MATH_ATOMS_PROVIDER_THINKING_LEVEL = "low"
$env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = ""
$env:MATH_ATOMS_PROVIDER_RESPONSE_KEY = ""
$env:MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS = "900"
$env:MATH_ATOMS_PROVIDER_PLAN_TIMEOUT_SECONDS = "21600"

Write-Host "launching Atom Vibe with LM Studio Qwen diagnostic profile: $Model"
& (Join-Path $PSScriptRoot "Launch-Native.ps1") -Build:$Build -Restart
