param(
    [int]$MaxAttempts = 10
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$OutDir = Join-Path $Engine "target\provider-built-apps"
$Manifest = Join-Path $OutDir "artifact-window.tsv"
$OriginalProbeIntent = $env:MATH_ATOMS_PROVIDER_PROBE_INTENT
$OriginalTemplate = $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE
$OriginalRustFlags = $env:RUSTFLAGS
$OriginalBmpPath = $env:MATH_ATOMS_REAL_APP_BMP

$ProviderKind = if ([string]::IsNullOrWhiteSpace($env:MATH_ATOMS_PROVIDER_KIND)) { "openai" } else { $env:MATH_ATOMS_PROVIDER_KIND }
$ProviderModel = $env:MATH_ATOMS_PROVIDER_MODEL
if ($ProviderKind -match "deepseek" -and $ProviderModel -match "pro") {
    throw "Provider real-app gate is configured for a DeepSeek Pro model; expected Flash"
}
if ($MaxAttempts -lt 1) {
    throw "MaxAttempts must be at least 1"
}

$Expected = "MATH_ATOMS_REAL_APP_OK pmre-task-board tasks=4 done=2 open=2 filtered=2 bmp=pmre-task-board.bmp"
$DeepSeekTemplate = '{"model":{{model_json}},"messages":[{"role":"system","content":"You generate complete Rust PMRE apps. Return exactly one fenced rust code block and no prose. The code must compile in a Cargo project with pmre-kit and pmre-orchestrator path dependencies, use the PMRE renderer/event APIs, write a BMP, and print the exact required success line."},{"role":"user","content":{{prompt_json}}}],"thinking":{"type":"disabled"},"temperature":0.1,"stream":false}'

function Convert-ToTomlPath([string]$Path) {
    return $Path.Replace("\", "/")
}

function New-RealAppIntent([string]$FailureEvidence) {
    $intent = @"
Provider model build a real user-facing PMRE app artifact through Math Atoms Coder.
Return exactly one fenced rust code block and no prose.

The generated source is src/main.rs for a Cargo binary with these dependencies already wired:
- pmre-kit
- pmre-orchestrator

Required app:
- define a struct named PmreTaskBoard
- use pmre_kit::ux::{Align, Dim, Edges, Justify, Style, UxNode} and Rgba
- use pmre_orchestrator::{handle_event, render_ui, widget_rect, UiEvent, UiState}
- every widget id must be a u32 constant, not a string
- build a task-board UI with a text input, add button, scrollable list, visible done/open counts, and selectable filter state
- simulate real UI events through handle_event: focus input, type four tasks using UiEvent::Char, click Add through widget_rect, toggle two tasks done by clicking their controls, wheel-scroll the list, select the open filter, then render with render_ui
- write the rendered BMP to std::env::var("MATH_ATOMS_REAL_APP_BMP").unwrap_or_else(|_| "pmre-task-board.bmp".to_string()) using framebuffer.to_bmp(...)
- main must fail with assertions if the interaction state is wrong
- main must print exactly:
$Expected

Rules:
- Rust 2021
- no external crates beyond pmre-kit and pmre-orchestrator
- no unsafe, no #[allow(...)] attributes, no network, no files except the output BMP, no browser, no Chrome, no WebView, no Tauri, no Electron
- ASCII source only
- no compiler warnings under RUSTFLAGS="-D warnings"

Use the PMRE builder API exactly like this style. Do not construct Style, UxNode, Dim, or Edges with struct literals.
Example shape:
const INPUT: u32 = 1;
const ADD: u32 = 2;
const LIST: u32 = 3;
fn build(app: &PmreTaskBoard, ui: &UiState) -> UxNode {
    UxNode::boxed(
        Style::col().w(Dim::Flex(1.0)).h(Dim::Flex(1.0)).pad(Edges::all(18.0)).gap(12.0).bg(Rgba::rgb8(20, 24, 28)),
        vec![
            UxNode::text("PMRE TASK BOARD", 22.0, Rgba::rgb8(245, 248, 250)),
            UxNode::boxed(Style::row().h(Dim::Px(42.0)).gap(8.0), vec![
                UxNode::boxed(Style::row().input(INPUT).w(Dim::Flex(1.0)).h(Dim::Px(42.0)).align(Align::Center).pad(Edges::xy(12.0, 0.0)).radius(8.0).bg(Rgba::rgb8(35, 41, 48)), vec![UxNode::text(ui.input_text(INPUT), 14.0, Rgba::rgb8(245, 248, 250))]),
                UxNode::boxed(Style::row().button(ADD).w(Dim::Px(96.0)).h(Dim::Px(42.0)).align(Align::Center).justify(Justify::Center).radius(8.0).bg(Rgba::rgb8(0, 132, 142)), vec![UxNode::text("ADD", 13.0, Rgba::rgb8(255, 255, 255))])
            ]),
            UxNode::boxed(Style::col().scroll(LIST).h(Dim::Px(230.0)).gap(8.0).pad(Edges::all(8.0)).bg(Rgba::rgb8(28, 34, 40)), app.filtered_rows(ui))
        ],
    )
}

Only these event forms are valid:
let mut ui = UiState::new(760, 520);
let build_fn = |state: &UiState| build(&app, state);
let rect = widget_rect(&build_fn, &ui, ADD).expect("add rect");
let x = (rect.min.x + rect.max.x) * 0.5;
let y = (rect.min.y + rect.max.y) * 0.5;
handle_event(&mut ui, &build_fn, UiEvent::PointerMove(x, y));
handle_event(&mut ui, &build_fn, UiEvent::PointerDown(x, y));
handle_event(&mut ui, &build_fn, UiEvent::PointerUp(x, y));
handle_event(&mut ui, &build_fn, UiEvent::Char('A'));
handle_event(&mut ui, &build_fn, UiEvent::Wheel(x, y, 96.0));
let frame = render_ui(&build_fn, &ui, Rgba::rgb8(20, 24, 28));
std::fs::write(&bmp_path, frame.to_bmp(Rgba::rgb8(20, 24, 28))).expect("write bmp");

Do not use UiEvent::Click, UiEvent::Focus, UiEvent::FocusInput, UiEvent::WheelScroll, Bounds imports, UiState::new() with no args, widget_rect(&node,...), render_ui(&node,...), or framebuffer.to_bmp(path).

Do not keep a build closure alive across app mutations. Use short scopes:
fn center(app: &PmreTaskBoard, ui: &UiState, id: u32) -> (f32, f32) {
    let build_fn = |state: &UiState| build(app, state);
    let rect = widget_rect(&build_fn, ui, id).expect("widget rect");
    ((rect.min.x + rect.max.x) * 0.5, (rect.min.y + rect.max.y) * 0.5)
}
fn click(app: &PmreTaskBoard, ui: &mut UiState, id: u32) {
    let (x, y) = center(app, ui, id);
    let build_fn = |state: &UiState| build(app, state);
    handle_event(ui, &build_fn, UiEvent::PointerMove(x, y));
    handle_event(ui, &build_fn, UiEvent::PointerDown(x, y));
    handle_event(ui, &build_fn, UiEvent::PointerUp(x, y));
}
After click returns, it is safe to mutate app based on ui.take_click().
Every local variable must be used or omitted. Print the exact required success line as a literal string, not a formatted path.
If an enum or struct appears in assert_eq!, derive Debug on it. If a function parameter is intentionally unused, remove it or prefix it with an underscore.
"@
    if (-not [string]::IsNullOrWhiteSpace($FailureEvidence)) {
        $intent += @"

Previous attempt failed. Correct the app and return a fresh complete fenced rust code block.
Failure evidence:
$FailureEvidence
"@
    }
    return $intent
}

function Invoke-ProviderProbe($Intent, $AppDir) {
    $env:MATH_ATOMS_PROVIDER_PROBE_INTENT = $Intent
    Push-Location $Engine
    try {
        $oldErrorActionPreference = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            $output = & cargo run --quiet -p math-atoms-core --example provider_probe --release 2>&1
            $exit = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $oldErrorActionPreference
        }
    }
    finally {
        Pop-Location
    }
    $text = ($output | Out-String)
    [System.IO.File]::WriteAllText((Join-Path $AppDir "provider-output.txt"), $text)
    if ($exit -ne 0) {
        throw "provider probe failed with exit code $exit. Output: $text"
    }
    return $text
}

function Get-RustCode($ProviderText) {
    $matches = [regex]::Matches(
        $ProviderText,
        '```(?:rust)?\s*(?<code>[\s\S]*?)```',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    if ($matches.Count -eq 0) {
        throw "provider output did not contain a fenced Rust code block"
    }
    return $matches[$matches.Count - 1].Groups["code"].Value.Trim()
}

function Assert-RealAppContract([string]$Code) {
    $required = @(
        @("PmreTaskBoard", "PmreTaskBoard"),
        @("pmre-kit import", "pmre_kit"),
        @("pmre-orchestrator import", "pmre_orchestrator"),
        @("render_ui call", "\brender_ui\b"),
        @("handle_event call", "\bhandle_event\b"),
        @("widget_rect call", "\bwidget_rect\b"),
        @("typed UI event", "UiEvent::Char"),
        @("UI event enum use", "\bUiEvent::"),
        @("BMP write", "\.to_bmp\("),
        @("success line prefix", "MATH_ATOMS_REAL_APP_OK pmre-task-board")
    )
    foreach ($item in $required) {
        if ($Code -notmatch $item[1]) {
            throw "generated PMRE app is missing $($item[0])"
        }
    }
    $forbidden = @(
        "unsafe",
        "#\s*\[\s*allow",
        "std::net",
        "std::process::Command",
        "reqwest",
        "webbrowser",
        "tauri",
        "electron",
        "chrome",
        "winit",
        "winapi",
        "windows::"
    )
    foreach ($pattern in $forbidden) {
        if ($Code -match $pattern) {
            throw "generated PMRE app contains forbidden pattern: $pattern"
        }
    }
}

function Write-CargoProject($AppDir, $SourceCode) {
    $srcDir = Join-Path $AppDir "src"
    New-Item -ItemType Directory -Force -Path $srcDir | Out-Null
    $kitPath = Convert-ToTomlPath ((Resolve-Path (Join-Path $Engine "pmre-kit")).Path)
    $orchPath = Convert-ToTomlPath ((Resolve-Path (Join-Path $Engine "pmre-orchestrator")).Path)
    $cargo = @"
[package]
name = "pmre-generated-real-app"
version = "0.1.0"
edition = "2021"

[dependencies]
pmre-kit = { path = "$kitPath" }
pmre-orchestrator = { path = "$orchPath" }

[workspace]
"@
    [System.IO.File]::WriteAllText((Join-Path $AppDir "Cargo.toml"), $cargo)
    [System.IO.File]::WriteAllText((Join-Path $srcDir "main.rs"), $SourceCode)
}

function Invoke-CargoBuild($AppDir) {
    for ($repairAttempt = 1; $repairAttempt -le 4; $repairAttempt++) {
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
        [System.IO.File]::WriteAllText((Join-Path $AppDir "cargo-build-output-$repairAttempt.txt"), $text)
        [System.IO.File]::WriteAllText((Join-Path $AppDir "cargo-build-output.txt"), $text)
        if ($exit -eq 0) {
            return
        }
        if (-not (Repair-GeneratedSource $AppDir $text)) {
            throw "cargo build exit $exit`n$text"
        }
    }
    throw "cargo build still failed after generated-source repairs"
}

function Repair-GeneratedSource($AppDir, [string]$BuildOutput) {
    $source = Join-Path $AppDir "src\main.rs"
    if (-not (Test-Path -LiteralPath $source)) {
        return $false
    }
    $code = [System.IO.File]::ReadAllText($source)
    $original = $code

    foreach ($match in [regex]::Matches($BuildOutput, "unused variable: ``(?<name>[A-Za-z_][A-Za-z0-9_]*)``")) {
        $name = $match.Groups["name"].Value
        if ($name.StartsWith("_")) {
            continue
        }
        $escaped = [regex]::Escape($name)
        $before = $code
        $code = ([regex]::new("([\(,]\s*)$escaped\s*:")).Replace($code, '${1}_' + $name + ':', 1)
        if ($code -ne $before) {
            continue
        }
        $code = ([regex]::new("\blet\s+mut\s+$escaped\b")).Replace($code, "let mut _$name", 1)
        $code = ([regex]::new("\blet\s+$escaped\b")).Replace($code, "let _$name", 1)
    }

    foreach ($match in [regex]::Matches($BuildOutput, "constant ``(?<name>[A-Za-z_][A-Za-z0-9_]*)`` is never used")) {
        $name = [regex]::Escape($match.Groups["name"].Value)
        $code = ([regex]::new("(?m)^\s*const\s+$name\s*:[^;]+;\r?\n?")).Replace($code, "", 1)
    }

    foreach ($match in [regex]::Matches($BuildOutput, "unused import: ``(?<import>[^``]+)``")) {
        $import = [regex]::Escape($match.Groups["import"].Value.Trim())
        $code = ([regex]::new("(?m)^\s*use\s+$import\s*;\r?\n?")).Replace($code, "", 1)
    }

    foreach ($match in [regex]::Matches($BuildOutput, "``(?<name>[A-Za-z_][A-Za-z0-9_]*)`` doesn't implement ``Debug``")) {
        $name = [regex]::Escape($match.Groups["name"].Value)
        if ($code -notmatch "#\[derive\([^\)]*Debug") {
            $code = ([regex]::new("(?m)^(\s*(?:enum|struct)\s+$name\b)")).Replace($code, "#[derive(Debug)]`r`n`${1}", 1)
        }
    }

    if ($code -eq $original) {
        return $false
    }
    [System.IO.File]::WriteAllText($source, $code)
    [System.IO.File]::AppendAllText((Join-Path $AppDir "repair-log.txt"), "applied compiler-guided generated-source repair`r`n")
    return $true
}

function Invoke-GeneratedApp($AppDir, $BmpPath) {
    $exe = Join-Path $AppDir "target\release\pmre-generated-real-app.exe"
    if (-not (Test-Path -LiteralPath $exe)) {
        throw "missing generated PMRE app executable: $exe"
    }
    $env:MATH_ATOMS_REAL_APP_BMP = $BmpPath
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
        throw "generated PMRE app exited $exit`n$text"
    }
    if ($text -ne $Expected) {
        throw "generated PMRE app output mismatch. Expected '$Expected' but got '$text'"
    }
    return $exe
}

function Assert-BmpArtifact($BmpPath) {
    if (-not (Test-Path -LiteralPath $BmpPath)) {
        throw "generated PMRE app did not write BMP artifact: $BmpPath"
    }
    $bytes = [System.IO.File]::ReadAllBytes($BmpPath)
    if ($bytes.Length -lt 54) {
        throw "generated PMRE BMP artifact is too small: $BmpPath"
    }
    if ($bytes[0] -ne 0x42 -or $bytes[1] -ne 0x4D) {
        throw "generated PMRE artifact is not a BMP: $BmpPath"
    }
    $width = [BitConverter]::ToInt32($bytes, 18)
    $height = [BitConverter]::ToInt32($bytes, 22)
    if ($width -lt 640 -or $height -lt 420) {
        throw "generated PMRE BMP dimensions are too small: ${width}x${height}"
    }
    $pixelOffset = [BitConverter]::ToInt32($bytes, 10)
    if ($pixelOffset -lt 54 -or $pixelOffset -ge $bytes.Length) {
        throw "generated PMRE BMP has an invalid pixel offset: $pixelOffset"
    }
    $unique = [System.Collections.Generic.HashSet[byte]]::new()
    for ($i = $pixelOffset; $i -lt $bytes.Length; $i += 97) {
        [void]$unique.Add($bytes[$i])
    }
    if ($unique.Count -lt 4) {
        throw "generated PMRE BMP appears visually blank or nearly uniform: sampled $($unique.Count) byte values"
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
    if ($ProviderKind -match "deepseek") {
        $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = $DeepSeekTemplate
    }
    $env:RUSTFLAGS = "-D warnings"
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

    $lastFailure = ""
    for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
        $appDir = Join-Path $OutDir ("pmre-task-board-attempt-{0}" -f $attempt)
        New-Item -ItemType Directory -Force -Path $appDir | Out-Null
        $bmp = Join-Path $appDir "pmre-task-board.bmp"
        try {
            $providerText = Invoke-ProviderProbe (New-RealAppIntent $lastFailure) $appDir
            $code = Get-RustCode $providerText
            Assert-RealAppContract $code
            Write-CargoProject $appDir $code
            Invoke-CargoBuild $appDir
            $exe = Invoke-GeneratedApp $appDir $bmp
            Assert-BmpArtifact $bmp
            $source = Join-Path $appDir "src\main.rs"
            Add-ManifestRow "pmre-task-board" $Expected $source $exe $bmp
            Write-Host "provider real PMRE app ok: generated, compiled, interacted, rendered: $Expected"
            return
        }
        catch {
            $lastFailure = $_.Exception.Message
            [System.IO.File]::WriteAllText((Join-Path $appDir "failure.txt"), $lastFailure)
            if ($attempt -eq $MaxAttempts) {
                throw "provider real PMRE app failed after $MaxAttempts attempts. Last failure: $lastFailure"
            }
        }
    }
}
finally {
    $env:MATH_ATOMS_PROVIDER_PROBE_INTENT = $OriginalProbeIntent
    $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = $OriginalTemplate
    $env:RUSTFLAGS = $OriginalRustFlags
    $env:MATH_ATOMS_REAL_APP_BMP = $OriginalBmpPath
}
