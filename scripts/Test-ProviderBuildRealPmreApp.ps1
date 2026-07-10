param(
    [string]$UserIntent = "Build me a usable task board app where I can add tasks, mark two finished, filter open work, and see the counts.",
    [int]$MaxAttempts = 4
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
. (Join-Path $PSScriptRoot "Learning-Loop.ps1")

$ProviderKind = if ([string]::IsNullOrWhiteSpace($env:MATH_ATOMS_PROVIDER_KIND)) { "openai" } else { $env:MATH_ATOMS_PROVIDER_KIND }
$ProviderModel = $env:MATH_ATOMS_PROVIDER_MODEL
if ([string]::IsNullOrWhiteSpace($UserIntent)) {
    throw "UserIntent must be natural language, not an empty prompt"
}
if ($UserIntent -match "pmre_|UiEvent|widget_rect|render_ui|Rust 2021|Cargo|Rgba|UxNode|Spiderweb") {
    throw "UserIntent must stay user-level natural language; renderer/API instructions belong to the harness"
}
if ($MaxAttempts -lt 1) {
    throw "MaxAttempts must be at least 1"
}

$Expected = "MATH_ATOMS_REAL_APP_OK pmre-task-board tasks=5 done=2 open=3 filtered=3 stack=canonical bmp=pmre-task-board.bmp"
function Convert-ToTomlPath([string]$Path) {
    return $Path.Replace("\", "/")
}

function New-AppSpecIntent([string]$NaturalLanguageRequest, [string]$FailureEvidence) {
    $intent = @"
The operator asked for an app in natural language:
$NaturalLanguageRequest

Return exactly one fenced json code block and no prose.
Create a product spec for the app. Do not include Rust, PMRE, widget APIs, renderer instructions, or implementation details.

Schema:
{
  "slug": "pmre-task-board",
  "title": "Task Board",
  "kind": "task_board",
  "tasks": ["Write spec", "Build UI", "Test artifact", "Ship build"],
  "done_indices": [0, 2],
  "filter": "open",
  "accent": "teal"
}

Rules:
- slug must be "pmre-task-board"
- kind must be "task_board"
- exactly four non-empty tasks
- exactly two done_indices, each between 0 and 3
- filter must be "open"
- accent must be one of: teal, blue, amber, green
- keep all strings ASCII
"@
    if (-not [string]::IsNullOrWhiteSpace($FailureEvidence)) {
        $intent += @"

Previous spec failed validation. Return a corrected complete json block.
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

function Get-FencedJson($ProviderText) {
    $matches = [regex]::Matches(
        $ProviderText,
        '```(?:json)?\s*(?<json>[\s\S]*?)```',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    if ($matches.Count -eq 0) {
        throw "provider output did not contain a fenced JSON app spec"
    }
    return $matches[$matches.Count - 1].Groups["json"].Value.Trim()
}

function Assert-Ascii([string]$Text, [string]$FieldName) {
    foreach ($ch in $Text.ToCharArray()) {
        if ([int][char]$ch -gt 127) {
            throw "$FieldName contains non-ASCII character U+$(([int][char]$ch).ToString('X4'))"
        }
    }
}

function Get-AppSpec($JsonText) {
    Assert-Ascii $JsonText "provider app spec"
    try {
        $spec = $JsonText | ConvertFrom-Json
    }
    catch {
        throw "provider app spec is not valid JSON: $($_.Exception.Message)"
    }
    if ($spec.slug -ne "pmre-task-board") {
        throw "spec slug must be pmre-task-board"
    }
    if ($spec.kind -ne "task_board") {
        throw "spec kind must be task_board"
    }
    if ([string]::IsNullOrWhiteSpace([string]$spec.title)) {
        throw "spec title is required"
    }
    Assert-Ascii ([string]$spec.title) "spec title"
    $tasks = @($spec.tasks)
    if ($tasks.Count -ne 4) {
        throw "spec must contain exactly four tasks"
    }
    foreach ($task in $tasks) {
        if ([string]::IsNullOrWhiteSpace([string]$task)) {
            throw "spec tasks must be non-empty"
        }
        Assert-Ascii ([string]$task) "spec task"
    }
    $done = @($spec.done_indices | ForEach-Object { [int]$_ })
    if ($done.Count -ne 2) {
        throw "spec must contain exactly two done_indices"
    }
    foreach ($index in $done) {
        if ($index -lt 0 -or $index -gt 3) {
            throw "done index out of range: $index"
        }
    }
    if (($done | Sort-Object -Unique).Count -ne 2) {
        throw "done_indices must be unique"
    }
    if ($spec.filter -ne "open") {
        throw "spec filter must be open"
    }
    $accent = [string]$spec.accent
    if (@("teal", "blue", "amber", "green") -notcontains $accent) {
        throw "spec accent must be teal, blue, amber, or green"
    }
    return [pscustomobject]@{
        Slug = [string]$spec.slug
        Title = [string]$spec.title
        Tasks = @($tasks | ForEach-Object { [string]$_ })
        DoneIndices = @($done)
        Accent = $accent
    }
}

function Escape-RustString([string]$Text) {
    return $Text.Replace("\", "\\").Replace('"', '\"')
}

function Rust-StringArray($Values) {
    return (($Values | ForEach-Object { '"' + (Escape-RustString ([string]$_)) + '"' }) -join ", ")
}

function Rust-IndexArray($Values) {
    return (($Values | ForEach-Object { [string][int]$_ }) -join ", ")
}

function Rust-IndexArrayUsize($Values) {
    return (($Values | ForEach-Object { "$([int]$_)usize" }) -join ", ")
}

function Accent-Rust($Accent) {
    switch ($Accent) {
        "blue" { "Rgba::rgb8(59, 112, 220)" }
        "amber" { "Rgba::rgb8(210, 150, 20)" }
        "green" { "Rgba::rgb8(46, 160, 96)" }
        default { "Rgba::rgb8(0, 132, 142)" }
    }
}

function Write-CargoProject($AppDir, $Spec) {
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
    [System.IO.File]::WriteAllText((Join-Path $srcDir "main.rs"), (New-PmreTaskBoardSource $Spec))
}

function New-PmreTaskBoardSource($Spec) {
    $title = Escape-RustString $Spec.Title
    $tasks = Rust-StringArray $Spec.Tasks
    $done = Rust-IndexArrayUsize $Spec.DoneIndices
    $accent = Accent-Rust $Spec.Accent
    return @"
use pmre_kit::{
    ux::{Align, Dim, Edges, Justify, Style, UxNode},
    Rgba,
};
use pmre_orchestrator::{handle_event, render_ui, widget_rect, UiEvent, UiState};

const INPUT: u32 = 1;
const ADD: u32 = 2;
const FILTER_OPEN: u32 = 3;
const LIST: u32 = 4;
const TOGGLE_BASE: u32 = 100;
const BG: Rgba = Rgba::new(0.07, 0.08, 0.09, 1.0);

#[derive(Clone, Debug)]
struct Task {
    text: String,
    done: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Filter {
    All,
    Open,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AtomStage {
    Scan,
    Project,
    Compose,
    Measure,
    Preserve,
    Order,
}

const ATOM_STACK: [AtomStage; 6] = [
    AtomStage::Scan,
    AtomStage::Project,
    AtomStage::Compose,
    AtomStage::Measure,
    AtomStage::Preserve,
    AtomStage::Order,
];

#[derive(Debug)]
struct PmreTaskBoard {
    title: String,
    tasks: Vec<Task>,
    filter: Filter,
}

impl PmreTaskBoard {
    fn from_spec() -> Self {
        let source_tasks = [$tasks];
        let tasks = source_tasks
            .iter()
            .map(|text| Task {
                text: (*text).to_string(),
                done: false,
            })
            .collect();
        Self {
            title: "$title".to_string(),
            tasks,
            filter: Filter::All,
        }
    }

    fn open_count(&self) -> usize {
        self.tasks.iter().filter(|task| !task.done).count()
    }

    fn done_count(&self) -> usize {
        self.tasks.iter().filter(|task| task.done).count()
    }

    fn visible_tasks(&self) -> Vec<(usize, &Task)> {
        self.tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| self.filter == Filter::All || !task.done)
            .collect()
    }
}

fn stack_is_canonical(stack: &[AtomStage]) -> bool {
    stack
        == [
            AtomStage::Scan,
            AtomStage::Project,
            AtomStage::Compose,
            AtomStage::Measure,
            AtomStage::Preserve,
            AtomStage::Order,
        ]
}

fn stack_performance_score(stack: &[AtomStage]) -> usize {
    stack
        .iter()
        .enumerate()
        .map(|(index, stage)| {
            let weight = match stage {
                AtomStage::Scan => 7,
                AtomStage::Project => 11,
                AtomStage::Compose => 17,
                AtomStage::Measure => 19,
                AtomStage::Preserve => 23,
                AtomStage::Order => 29,
            };
            (index + 1) * weight
        })
        .sum()
}

fn accent() -> Rgba {
    $accent
}

fn panel() -> Rgba {
    Rgba::rgb8(29, 35, 41)
}

fn muted() -> Rgba {
    Rgba::rgb8(158, 170, 176)
}

fn text_hi() -> Rgba {
    Rgba::rgb8(244, 248, 248)
}

fn build(app: &PmreTaskBoard, ui: &UiState) -> UxNode {
    let task_rows: Vec<UxNode> = app
        .visible_tasks()
        .into_iter()
        .map(|(index, task)| {
            let mark = if task.done { "DONE" } else { "OPEN" };
            UxNode::boxed(
                Style::row()
                    .button(TOGGLE_BASE + index as u32)
                    .h(Dim::Px(42.0))
                    .gap(10.0)
                    .align(Align::Center)
                    .pad(Edges::xy(10.0, 0.0))
                    .radius(7.0)
                    .bg(Rgba::rgb8(39, 47, 55)),
                vec![
                    UxNode::boxed(
                        Style::row()
                            .w(Dim::Px(58.0))
                            .h(Dim::Px(24.0))
                            .align(Align::Center)
                            .justify(Justify::Center)
                            .radius(6.0)
                            .bg(if task.done { accent() } else { Rgba::rgb8(74, 85, 94) }),
                        vec![UxNode::text(mark, 10.0, text_hi())],
                    ),
                    UxNode::text(&task.text, 14.0, text_hi()),
                ],
            )
        })
        .collect();
    let counts = format!(
        "tasks={} done={} open={}",
        app.tasks.len(),
        app.done_count(),
        app.open_count()
    );
    UxNode::boxed(
        Style::col()
            .w(Dim::Flex(1.0))
            .h(Dim::Flex(1.0))
            .pad(Edges::all(18.0))
            .gap(12.0)
            .bg(BG),
        vec![
            UxNode::text(&app.title, 24.0, text_hi()),
            UxNode::text(&counts, 13.0, muted()),
            UxNode::boxed(
                Style::row().h(Dim::Px(42.0)).gap(8.0),
                vec![
                    UxNode::boxed(
                        Style::row()
                            .input(INPUT)
                            .w(Dim::Flex(1.0))
                            .h(Dim::Px(42.0))
                            .align(Align::Center)
                            .pad(Edges::xy(12.0, 0.0))
                            .radius(7.0)
                            .bg(panel()),
                        vec![UxNode::text(ui.input_text(INPUT), 14.0, text_hi())],
                    ),
                    UxNode::boxed(
                        Style::row()
                            .button(ADD)
                            .w(Dim::Px(92.0))
                            .h(Dim::Px(42.0))
                            .align(Align::Center)
                            .justify(Justify::Center)
                            .radius(7.0)
                            .bg(accent()),
                        vec![UxNode::text("ADD", 13.0, text_hi())],
                    ),
                    UxNode::boxed(
                        Style::row()
                            .button(FILTER_OPEN)
                            .w(Dim::Px(108.0))
                            .h(Dim::Px(42.0))
                            .align(Align::Center)
                            .justify(Justify::Center)
                            .radius(7.0)
                            .bg(if app.filter == Filter::Open { accent() } else { panel() }),
                        vec![UxNode::text("OPEN", 13.0, text_hi())],
                    ),
                ],
            ),
            UxNode::boxed(
                Style::col()
                    .scroll(LIST)
                    .h(Dim::Px(270.0))
                    .gap(8.0)
                    .pad(Edges::all(8.0))
                    .radius(8.0)
                    .bg(panel()),
                task_rows,
            ),
        ],
    )
}

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

fn type_text(app: &PmreTaskBoard, ui: &mut UiState, text: &str) {
    let (x, y) = center(app, ui, INPUT);
    {
        let build_fn = |state: &UiState| build(app, state);
        handle_event(ui, &build_fn, UiEvent::PointerDown(x, y));
    }
    for ch in text.chars() {
        let build_fn = |state: &UiState| build(app, state);
        handle_event(ui, &build_fn, UiEvent::Char(ch));
    }
}

fn main() {
    assert!(stack_is_canonical(&ATOM_STACK));
    assert!(stack_performance_score(&ATOM_STACK) >= 350);

    let mut app = PmreTaskBoard::from_spec();
    let mut ui = UiState::new(760, 520);

    type_text(&app, &mut ui, "Operator typed task");
    click(&app, &mut ui, ADD);
    if ui.take_click() == Some(ADD) {
        let text = ui.input_text(INPUT).trim().to_string();
        if !text.is_empty() {
            app.tasks.push(Task { text, done: false });
        }
        ui.clear_input(INPUT);
    }
    assert_eq!(app.tasks.len(), 5);

    for index in [$done] {
        click(&app, &mut ui, TOGGLE_BASE + index as u32);
        if ui.take_click() == Some(TOGGLE_BASE + index as u32) {
            app.tasks[index].done = true;
        }
    }

    let (sx, sy) = center(&app, &ui, LIST);
    {
        let build_fn = |state: &UiState| build(&app, state);
        handle_event(&mut ui, &build_fn, UiEvent::Wheel(sx, sy, 96.0));
    }
    click(&app, &mut ui, FILTER_OPEN);
    if ui.take_click() == Some(FILTER_OPEN) {
        app.filter = Filter::Open;
    }

    assert_eq!(app.done_count(), 2);
    assert_eq!(app.open_count(), 3);
    assert_eq!(app.visible_tasks().len(), 3);
    assert_eq!(app.filter, Filter::Open);

    let bmp_path =
        std::env::var("MATH_ATOMS_REAL_APP_BMP").unwrap_or_else(|_| "pmre-task-board.bmp".to_string());
    {
        let build_fn = |state: &UiState| build(&app, state);
        let frame = render_ui(&build_fn, &ui, BG);
        std::fs::write(&bmp_path, frame.to_bmp(BG)).expect("write bmp");
    }

    println!("MATH_ATOMS_REAL_APP_OK pmre-task-board tasks=5 done=2 open=3 filtered=3 stack=canonical bmp=pmre-task-board.bmp");
}
"@
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
        $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = ""
    }
    $env:RUSTFLAGS = "-D warnings"
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

    $durableCorrection = Get-AtomLearningContext -Intent $UserIntent -Atoms "scan,project,compose,measure,preserve,order" -Limit 4
    if ($durableCorrection -match 'hits=0') { $durableCorrection = "" }
    $lastFailure = ""
    for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
        $appDir = Join-Path $OutDir ("pmre-task-board-attempt-{0}" -f $attempt)
        New-Item -ItemType Directory -Force -Path $appDir | Out-Null
        $bmp = Join-Path $appDir "pmre-task-board.bmp"
        $attemptIntent = New-AppSpecIntent $UserIntent $lastFailure
        try {
            $providerText = Invoke-ProviderProbe $attemptIntent $appDir
            $work = Get-AtomWorkEvidence -ProviderText $providerText
            $json = Get-FencedJson $providerText
            [System.IO.File]::WriteAllText((Join-Path $appDir "app-spec.json"), $json)
            $spec = Get-AppSpec $json
            Write-CargoProject $appDir $spec
            Invoke-CargoBuild $appDir
            $exe = Invoke-GeneratedApp $appDir $bmp
            Assert-BmpArtifact $bmp
            $source = Join-Path $appDir "src\main.rs"
            Add-ManifestRow $spec.Slug $Expected $source $exe $bmp
            $attestation = New-AtomHarnessAttestation -HarnessId "native-pmre-functional-v1" -Gate "natural-language-pmre-app" -Artifact $bmp -Executable $exe -ExpectedOutput $Expected -AttestationPath (Join-Path $appDir "harness-attestation.json") -WorkingDirectory $appDir -WorkPlanId $work.PlanId -ProviderModel $work.Model -ArtifactEnv "MATH_ATOMS_REAL_APP_BMP"
            $correctionEvidence = if ([string]::IsNullOrWhiteSpace($lastFailure)) { $durableCorrection } else { $lastFailure }
            Write-AtomLearningRecord -Source "provider-pmre-app" -Intent $UserIntent -Recipe "production-app-runtime" -Atoms "scan,project,compose,measure,preserve,order" -Gate "natural-language-pmre-app" -Attempt $attempt -Outcome "succeeded" -Correction $correctionEvidence -Artifact $bmp -ProviderModel $work.Model -WorkPlanId $work.PlanId -WorkPlanManifest $work.Manifest -WorkPacketCount $work.PacketCount -HarnessAttestation $attestation.Path -HarnessAttestationHash $attestation.Hash
            Write-Host "provider natural-language PMRE app ok: spec generated, harness compiled, interacted, rendered: $Expected"
            return
        }
        catch {
            $lastFailure = $_.Exception.Message
            [System.IO.File]::WriteAllText((Join-Path $appDir "failure.txt"), $lastFailure)
            Write-AtomLearningRecord -Source "provider-pmre-app" -Intent $UserIntent -Recipe "production-app-runtime" -Atoms "scan,project,compose,measure,preserve,order" -Gate "natural-language-pmre-app" -Attempt $attempt -Outcome "failed" -Failure $lastFailure -ProviderModel $ProviderModel
            if ($attempt -eq $MaxAttempts) {
                throw "provider natural-language PMRE app failed after $MaxAttempts attempts. Last failure: $lastFailure"
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
