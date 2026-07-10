param(
    [string]$HtmlPath = "",
    [string]$CssPath = "",
    [string]$Name = "uploaded-design-app"
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$OutDir = Join-Path $Engine "target\provider-built-apps"
$Manifest = Join-Path $OutDir "artifact-window.tsv"
$DefaultInputDir = Join-Path $Engine "target\design-upload-input"
$OriginalRustFlags = $env:RUSTFLAGS
$OriginalBmpPath = $env:MATH_ATOMS_DESIGN_APP_BMP
. (Join-Path $PSScriptRoot "Learning-Loop.ps1")

if ($Name -notmatch '^[a-z0-9][a-z0-9-]{0,48}$') {
    throw "Name must be a lowercase slug containing only a-z, 0-9, and hyphen"
}
$LearningIntent = "Build native PMRE app '$Name' from uploaded HTML and CSS design files"
$DurableCorrection = Get-AtomLearningContext -Intent $LearningIntent -Atoms "project,combine,measure,compose" -Limit 4
if ($DurableCorrection -match 'hits=0') { $DurableCorrection = "" }

function Convert-ToTomlPath([string]$Path) {
    return $Path.Replace("\", "/")
}

function Escape-RustString([string]$Text) {
    return $Text.Replace("\", "\\").Replace('"', '\"')
}

function Write-DefaultDesignInputs {
    New-Item -ItemType Directory -Force -Path $DefaultInputDir | Out-Null
    $html = @"
<main class="design-shell">
  <section class="hero">
    <span id="eyebrow">Design Upload</span>
    <h1>Native PMRE Design Build</h1>
    <p class="summary">This app was compiled from uploaded HTML and CSS, then rendered through the atom renderer.</p>
    <div class="stats">
      <div class="metric"><strong class="metric-value">HTML</strong><span class="metric-label"> mounted</span></div>
      <div class="metric"><strong class="metric-value">CSS</strong><span class="metric-label"> applied</span></div>
      <div class="metric"><strong class="metric-value">PMRE</strong><span class="metric-label"> artifact</span></div>
    </div>
    <button class="primary">Build From Design</button>
  </section>
</main>
"@
    $css = @"
.design-shell {
  background: #f4f7f6;
  padding: 26px;
  gap: 18px;
}
.hero {
  background: #ffffff;
  padding: 28px;
  border: 1px solid #ccd6d3;
  border-radius: 8px;
  gap: 14px;
}
#eyebrow {
  color: #00848e;
  font-size: 13px;
  font-weight: 700;
}
h1 {
  color: #101818;
  font-size: 30px;
  font-weight: 800;
}
.summary {
  color: #5a6766;
  font-size: 15px;
}
.stats {
  display: flex;
  gap: 10px;
}
.metric {
  background: #eef6f4;
  border: 1px solid #ccd6d3;
  border-radius: 7px;
  padding: 12px;
  width: 28%;
}
.metric-value {
  color: #101818;
  font-size: 15px;
}
.metric-label {
  color: #5a6766;
  font-size: 12px;
}
.primary {
  background: #00848e;
  color: #ffffff;
  padding: 12px;
  border-radius: 7px;
  font-size: 15px;
  font-weight: 700;
}
"@
    $htmlFile = Join-Path $DefaultInputDir "design.html"
    $cssFile = Join-Path $DefaultInputDir "design.css"
    [System.IO.File]::WriteAllText($htmlFile, $html)
    [System.IO.File]::WriteAllText($cssFile, $css)
    return [pscustomobject]@{ Html = $htmlFile; Css = $cssFile }
}

function Resolve-DesignInput([string]$Path, [string]$Kind) {
    if ([string]::IsNullOrWhiteSpace($Path)) {
        return $null
    }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Kind path does not exist: $Path"
    }
    return (Resolve-Path -LiteralPath $Path).Path
}

function Write-CargoProject($AppDir, $HtmlSource, $CssSource, $Name) {
    $srcDir = Join-Path $AppDir "src"
    New-Item -ItemType Directory -Force -Path $srcDir | Out-Null
    Copy-Item -LiteralPath $HtmlSource -Destination (Join-Path $AppDir "design.html") -Force
    Copy-Item -LiteralPath $CssSource -Destination (Join-Path $AppDir "design.css") -Force

    $kitPath = Convert-ToTomlPath ((Resolve-Path (Join-Path $Engine "pmre-kit")).Path)
    $orchPath = Convert-ToTomlPath ((Resolve-Path (Join-Path $Engine "pmre-orchestrator")).Path)
    $cargo = @"
[package]
name = "pmre-design-upload-app"
version = "0.1.0"
edition = "2021"

[dependencies]
pmre-kit = { path = "$kitPath" }
pmre-orchestrator = { path = "$orchPath" }

[workspace]
"@
    [System.IO.File]::WriteAllText((Join-Path $AppDir "Cargo.toml"), $cargo)

    $escapedName = Escape-RustString $Name
    $expected = "MATH_ATOMS_DESIGN_APP_OK $Name html=1 css=1 bmp=design-upload-app.bmp"
    $escapedExpected = Escape-RustString $expected
    $source = @"
use pmre_kit::Rgba;
use pmre_orchestrator::render_html;

const DESIGN_NAME: &str = "$escapedName";
const DESIGN_HTML: &str = include_str!("../design.html");
const DESIGN_CSS: &str = include_str!("../design.css");
const EXPECTED: &str = "$escapedExpected";

fn main() {
    assert!(!DESIGN_NAME.trim().is_empty());
    assert!(!DESIGN_HTML.trim().is_empty(), "uploaded html is empty");
    assert!(!DESIGN_CSS.trim().is_empty(), "uploaded css is empty");
    let document = format!("<style>{}</style>{}", DESIGN_CSS, DESIGN_HTML);
    let clear = Rgba::rgb8(244, 247, 246);
    let frame = render_html(&document, 900, 640, clear);
    let bmp_path = std::env::var("MATH_ATOMS_DESIGN_APP_BMP")
        .unwrap_or_else(|_| "design-upload-app.bmp".to_string());
    std::fs::write(&bmp_path, frame.to_bmp(clear)).expect("write design bmp");
    println!("{EXPECTED}");
}
"@
    [System.IO.File]::WriteAllText((Join-Path $srcDir "main.rs"), $source)
}

function Invoke-CargoBuild($AppDir) {
    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $output = & cargo build --release --manifest-path (Join-Path $AppDir "Cargo.toml") 2>&1
        $exit = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $oldErrorActionPreference
    }
    $text = ($output | Out-String)
    [System.IO.File]::WriteAllText((Join-Path $AppDir "cargo-build-output.txt"), $text)
    if ($exit -ne 0) {
        throw "cargo build exit $exit`n$text"
    }
}

function Invoke-GeneratedDesignApp($AppDir, $BmpPath, $Expected) {
    $exe = Join-Path $AppDir "target\release\pmre-design-upload-app.exe"
    if (-not (Test-Path -LiteralPath $exe)) {
        throw "missing generated design app executable: $exe"
    }
    $env:MATH_ATOMS_DESIGN_APP_BMP = $BmpPath
    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $output = & $exe 2>&1
        $exit = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $oldErrorActionPreference
    }
    $text = (($output | Out-String).Trim())
    [System.IO.File]::WriteAllText((Join-Path $AppDir "app-output.txt"), $text)
    if ($exit -ne 0) {
        throw "generated design app exited $exit`n$text"
    }
    if ($text -ne $Expected) {
        throw "generated design app output mismatch. Expected '$Expected' but got '$text'"
    }
    return $exe
}

function Assert-BmpArtifact($BmpPath) {
    if (-not (Test-Path -LiteralPath $BmpPath)) {
        throw "generated design app did not write BMP artifact: $BmpPath"
    }
    $bytes = [System.IO.File]::ReadAllBytes($BmpPath)
    if ($bytes.Length -lt 54) {
        throw "generated design BMP artifact is too small: $BmpPath"
    }
    if ($bytes[0] -ne 0x42 -or $bytes[1] -ne 0x4D) {
        throw "generated design artifact is not a BMP: $BmpPath"
    }
    $width = [BitConverter]::ToInt32($bytes, 18)
    $height = [BitConverter]::ToInt32($bytes, 22)
    if ($width -ne 900 -or $height -ne 640) {
        throw "generated design BMP dimensions are wrong: ${width}x${height}"
    }
    $pixelOffset = [BitConverter]::ToInt32($bytes, 10)
    if ($pixelOffset -lt 54 -or $pixelOffset -ge $bytes.Length) {
        throw "generated design BMP has an invalid pixel offset: $pixelOffset"
    }
    $unique = [System.Collections.Generic.HashSet[byte]]::new()
    for ($i = $pixelOffset; $i -lt $bytes.Length; $i += 113) {
        [void]$unique.Add($bytes[$i])
    }
    if ($unique.Count -lt 6) {
        throw "generated design BMP appears visually blank or nearly uniform: sampled $($unique.Count) byte values"
    }
}

function Upsert-ManifestHeader() {
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    if (-not (Test-Path -LiteralPath $Manifest)) {
        [System.IO.File]::WriteAllLines($Manifest, @("name`tstatus`toutput`tsource`texe`tartifact"))
        return
    }
    $lines = [System.Collections.Generic.List[string]]::new()
    foreach ($line in [System.IO.File]::ReadLines($Manifest)) {
        $lines.Add($line)
    }
    if ($lines.Count -eq 0) {
        $lines.Add("name`tstatus`toutput`tsource`texe`tartifact")
    }
    elseif ($lines[0] -ne "name`tstatus`toutput`tsource`texe`tartifact") {
        $lines[0] = "name`tstatus`toutput`tsource`texe`tartifact"
    }
    [System.IO.File]::WriteAllLines($Manifest, $lines)
}

function Add-ManifestRow($Name, $Output, $Source, $Exe, $Artifact) {
    Upsert-ManifestHeader
    $lines = [System.Collections.Generic.List[string]]::new()
    foreach ($line in [System.IO.File]::ReadLines($Manifest)) {
        if ($line -notmatch "^$([regex]::Escape($Name))\t") {
            $lines.Add($line)
        }
    }
    $lines.Add("$Name`tcompiled`t$Output`t$Source`t$Exe`t$Artifact")
    [System.IO.File]::WriteAllLines($Manifest, $lines)
}

try {
    $env:RUSTFLAGS = "-D warnings"
    $defaults = $null
    $resolvedHtml = Resolve-DesignInput $HtmlPath "HTML"
    $resolvedCss = Resolve-DesignInput $CssPath "CSS"
    if ($null -eq $resolvedHtml -or $null -eq $resolvedCss) {
        $defaults = Write-DefaultDesignInputs
        if ($null -eq $resolvedHtml) {
            $resolvedHtml = $defaults.Html
        }
        if ($null -eq $resolvedCss) {
            $resolvedCss = $defaults.Css
        }
    }

    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    $appDir = Join-Path $OutDir $Name
    New-Item -ItemType Directory -Force -Path $appDir | Out-Null
    $bmp = Join-Path $appDir "design-upload-app.bmp"
    $expected = "MATH_ATOMS_DESIGN_APP_OK $Name html=1 css=1 bmp=design-upload-app.bmp"

    Write-CargoProject $appDir $resolvedHtml $resolvedCss $Name
    Invoke-CargoBuild $appDir
    $exe = Invoke-GeneratedDesignApp $appDir $bmp $expected
    Assert-BmpArtifact $bmp
    $source = Join-Path $appDir "src\main.rs"
    Add-ManifestRow $Name $expected $source $exe $bmp
    $attestation = New-AtomHarnessAttestation -HarnessId "design-upload-functional-v1" -Gate "design-upload-build" -Artifact $bmp -Executable $exe -ExpectedOutput $expected -AttestationPath (Join-Path $appDir "harness-attestation.json") -WorkingDirectory $appDir -ArtifactEnv "MATH_ATOMS_DESIGN_APP_BMP"
    Write-AtomLearningRecord -Source "design-upload" -Intent $LearningIntent -Recipe "native-atom-renderer" -Atoms "project,combine,measure,compose" -Gate "design-upload-build" -Attempt 1 -Outcome "succeeded" -Correction $DurableCorrection -Artifact $bmp -HarnessAttestation $attestation.Path -HarnessAttestationHash $attestation.Hash
    Write-Host "design upload build ok: $expected"
}
catch {
    $failure = $_.Exception.Message
    Write-AtomLearningRecord -Source "design-upload" -Intent $LearningIntent -Recipe "native-atom-renderer" -Atoms "project,combine,measure,compose" -Gate "design-upload-build" -Attempt 1 -Outcome "failed" -Failure $failure
    throw
}
finally {
    $env:RUSTFLAGS = $OriginalRustFlags
    $env:MATH_ATOMS_DESIGN_APP_BMP = $OriginalBmpPath
}
