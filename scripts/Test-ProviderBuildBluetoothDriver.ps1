param(
    [int]$MaxAttempts = 6
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$ArtifactRoot = Join-Path $Engine "target\provider-built-apps"
$OutDir = Join-Path $ArtifactRoot ("runs\bluetooth-" + [Guid]::NewGuid().ToString("N"))
$Manifest = Join-Path $ArtifactRoot "artifact-window.tsv"
$Source = Join-Path $OutDir "bluetooth_driver.rs"
$Exe = Join-Path $OutDir "bluetooth_driver.exe"
$Review = Join-Path $OutDir "driver-review.md"
$OriginalProbeIntent = $env:MATH_ATOMS_PROVIDER_PROBE_INTENT
$OriginalTemplate = $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE
. (Join-Path $PSScriptRoot "Learning-Loop.ps1")
. (Join-Path $PSScriptRoot "Artifact-Manifest.ps1")

$ProviderKind = if ([string]::IsNullOrWhiteSpace($env:MATH_ATOMS_PROVIDER_KIND)) { "openai" } else { $env:MATH_ATOMS_PROVIDER_KIND }
$ProviderModel = $env:MATH_ATOMS_PROVIDER_MODEL
if ($MaxAttempts -lt 1) {
    throw "MaxAttempts must be at least 1"
}

$Expected = "MATH_ATOMS_DRIVER_OK bluetooth hci_reset=0x0C03 scan=enabled devices=2 connected=AA:BB:CC:DD:EE:01 stack=canonical"
function New-DriverIntent([string]$FailureEvidence) {
    $intent = @"
provider model build a complete dependency-free Rust Bluetooth HCI driver core through Atom Vibe Coder.
Return exactly one fenced rust code block and no prose.
The generated source must be one file, Rust standard library only, deterministic, and compile with:
rustc --edition=2021 -D warnings
Do not use external crates, files, network, stdin, timers, unsafe, FFI, OS kernel APIs, platform APIs, threads, or sleeps.
Do not use #![no_std], #![no_implicit_prelude], or extern crate std; use normal Rust 2021 standard-library prelude.
No compiler warnings are allowed: every field, method, import, variable, enum, and struct must be used in executable logic.
Do not use #[allow(...)] or #![allow(...)] lint suppression.
No placeholders: do not include todo!, unimplemented!, panic!("TODO"), FIXME, or stub comments.

Build a Bluetooth Low Energy HCI driver core, not a UI app.
Define exactly these public-facing core types in the generated code:
- struct HciCommand
- struct HciTransport
- struct BluetoothDriver
- struct Advertisement
- enum DriverState

Behavior requirements:
- Define a const ATOM_STACK with this exact ordered stack: scan -> project -> compose -> measure -> preserve -> order.
- Validate the ATOM_STACK in executable logic before printing.
- HciCommand must store opcode: u16 and payload: Vec<u8>.
- HciTransport must record HciCommand packets sent by the driver, not only opcode numbers.
- Advertisement must store address: String and rssi: i8.
- BluetoothDriver must store connected_address: Option<String>.
- DriverState must include Idle, Initialized, Scanning, and Connected.
- BluetoothDriver::initialize must send HCI Reset opcode 0x0C03 and mark DriverState::Initialized.
- The program must assert the driver is Initialized after initialize and before scanning.
- BluetoothDriver::enable_scan must send LE Set Scan Enable opcode 0x200C and mark scan enabled.
- The driver must ingest exactly two deterministic Advertisement records.
- One advertisement address must be AA:BB:CC:DD:EE:01 and the other AA:BB:CC:DD:EE:02.
- BluetoothDriver::connect must return bool.
- BluetoothDriver::connect must refuse unknown addresses and must not mark connected unless the address exists in advertisements.
- The program must first attempt to connect to unknown address AA:BB:CC:DD:EE:99, verify the attempt returns false, and verify connected_address is still None.
- The driver must then connect to AA:BB:CC:DD:EE:01 and mark DriverState::Connected.
- The program must review its own driver state in executable logic: verify reset opcode, scan opcode, scan enabled, exactly two devices, connected address, and canonical atom stack.
- fn main must print exactly:
$Expected
"@
    if (-not [string]::IsNullOrWhiteSpace($FailureEvidence)) {
        $intent += @"

Previous attempt failed. Correct the Bluetooth driver and return a fresh complete fenced rust code block.
Failure evidence:
$FailureEvidence
"@
    }
    return $intent
}

function Invoke-ProviderProbe([string]$Intent, [string]$AttemptDir) {
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
    [System.IO.File]::WriteAllText((Join-Path $AttemptDir "provider-output.txt"), $text)
    if ($exit -ne 0) {
        throw "provider probe failed with exit code $exit. Output: $text"
    }
    return $text
}

function Get-RustCode([string]$ProviderText) {
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

function Assert-Contains([string]$Code, [string]$Pattern, [string]$Message) {
    if ($Code -notmatch $Pattern) {
        throw $Message
    }
}

function Assert-CanonicalStackCode([string]$Code) {
    $stackStart = $Code.IndexOf("ATOM_STACK", [System.StringComparison]::OrdinalIgnoreCase)
    if ($stackStart -lt 0) {
        throw "Bluetooth driver is missing ATOM_STACK"
    }
    $required = @("scan", "project", "compose", "measure", "preserve", "order")
    $last = $stackStart
    foreach ($atom in $required) {
        $idx = $Code.IndexOf($atom, $stackStart, [System.StringComparison]::OrdinalIgnoreCase)
        if ($idx -lt 0) {
            throw "Bluetooth driver ATOM_STACK is missing $atom"
        }
        if ($idx -le $last) {
            throw "Bluetooth driver ATOM_STACK is not canonical order"
        }
        $last = $idx
    }
}

function Assert-DriverSource([string]$Code) {
    Assert-Contains $Code 'pub\s+struct\s+HciTransport\b' "missing public HciTransport"
    Assert-Contains $Code 'pub\s+struct\s+HciCommand\b' "missing public HciCommand"
    Assert-Contains $Code 'pub\s+struct\s+BluetoothDriver\b' "missing public BluetoothDriver"
    Assert-Contains $Code 'pub\s+struct\s+Advertisement\b' "missing public Advertisement"
    Assert-Contains $Code 'pub\s+enum\s+DriverState\b' "missing public DriverState"
    Assert-Contains $Code 'opcode\s*:\s*u16' "HciCommand must store opcode: u16"
    Assert-Contains $Code 'payload\s*:\s*Vec\s*<\s*u8\s*>' "HciCommand must store payload: Vec<u8>"
    Assert-Contains $Code 'rssi\s*:\s*i8' "Advertisement must store rssi: i8"
    Assert-Contains $Code 'connected_address\s*:\s*Option\s*<\s*String\s*>' "BluetoothDriver must store connected_address: Option<String>"
    Assert-Contains $Code 'fn\s+connect\s*\(\s*&mut\s+self\s*,\s*address\s*:\s*&str\s*\)\s*->\s*bool' "BluetoothDriver::connect must return bool"
    Assert-Contains $Code 'Initialized' "DriverState must include Initialized"
    Assert-Contains $Code 'state\s*=\s*DriverState::Initialized' "initialize must mark DriverState::Initialized"
    Assert-Contains $Code '0x0C03|0x0c03' "missing HCI Reset opcode 0x0C03"
    Assert-Contains $Code '0x200C|0x200c' "missing LE Set Scan Enable opcode 0x200C"
    Assert-Contains $Code 'AA:BB:CC:DD:EE:01' "missing expected connected address"
    Assert-Contains $Code 'AA:BB:CC:DD:EE:02' "missing second advertisement address"
    Assert-Contains $Code 'AA:BB:CC:DD:EE:99' "missing rejected unknown-address connection probe"
    Assert-Contains $Code 'Connected' "missing connected state"
    Assert-CanonicalStackCode $Code
    if ($Code -match '\bunsafe\s*(\{|fn\b|impl\b|trait\b)') {
        throw "Bluetooth driver must not use unsafe blocks, functions, impls, or traits"
    }
    if ($Code -match '#!\s*\[\s*no_std|#\s*!\s*\[\s*no_implicit_prelude|extern\s+crate\s+std') {
        throw "Bluetooth driver must use normal Rust 2021 std prelude, not no_std/no_implicit_prelude/extern crate std"
    }
    if ($Code -match '#!\s*\[\s*allow|#\s*\[\s*allow') {
        throw "Bluetooth driver must not use lint suppression attributes"
    }
    if ($Code -match 'todo!|unimplemented!|FIXME|TODO|stub') {
        throw "Bluetooth driver contains placeholder marker"
    }
}

function Invoke-Rustc([string]$SourcePath, [string]$ExePath, [string]$AttemptDir) {
    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $output = & rustc --edition=2021 -D warnings $SourcePath -o $ExePath 2>&1
        $exit = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $oldErrorActionPreference
    }
    $text = ($output | Out-String)
    [System.IO.File]::WriteAllText((Join-Path $AttemptDir "rustc-output.txt"), $text)
    if ($exit -ne 0) {
        throw "rustc exit $exit`n$text"
    }
}

function Update-ArtifactManifest([string]$Actual) {
    Update-AtomArtifactManifest -Path $Manifest -Name "bluetooth-driver" -Status "compiled" -Output $Actual -Source $Source -Exe $Exe
}

try {
    if ($ProviderKind -match "deepseek") {
        $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = ""
    }

    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    $durableCorrection = Get-AtomLearningContext -Intent "Build a Bluetooth driver" -Atoms "scan,project,compose,measure,preserve,order" -Limit 4
    if ($durableCorrection -match 'hits=0') { $durableCorrection = "" }
    $lastFailure = ""
    for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
        $attemptDir = Join-Path $OutDir ("attempt-{0}" -f $attempt)
        New-Item -ItemType Directory -Force -Path $attemptDir | Out-Null
        $attemptIntent = New-DriverIntent $lastFailure
        try {
            $providerText = Invoke-ProviderProbe $attemptIntent $attemptDir
            $work = Get-AtomWorkEvidence -ProviderText $providerText
            $code = Get-RustCode $providerText
            Assert-DriverSource $code
            [System.IO.File]::WriteAllText($Source, $code)
            Copy-Item -LiteralPath $Source -Destination (Join-Path $attemptDir "bluetooth_driver.rs") -Force

            Push-Location $OutDir
            try {
                Invoke-Rustc $Source $Exe $attemptDir
                $actual = ((& $Exe) -join "`n").Trim()
            }
            finally {
                Pop-Location
            }
            if ($actual -ne $Expected) {
                throw "output mismatch. Expected '$Expected' but got '$actual'"
            }

            $reviewText = @"
# Bluetooth Driver Review

Status: PASS

- Generated through provider_probe via Atom Vibe Coder provider path.
- Compiled with rustc --edition=2021 -D warnings.
- Ran executable and matched expected proof output.
- Static review found HciCommand, HciTransport, BluetoothDriver, Advertisement, DriverState.
- Static review found opcode/payload command packets, advertisement RSSI, and explicit connected address storage.
- Static review found connect returns bool and includes rejected unknown-address probe.
- Static review found reset initializes state before scanning.
- Static review found HCI Reset opcode 0x0C03 and LE Set Scan Enable opcode 0x200C.
- Static review found both deterministic advertisement addresses and connected state.
- Static review found canonical atom stack: scan -> project -> compose -> measure -> preserve -> order.
- Static review found no unsafe, TODO, FIXME, stub, todo!, or unimplemented! markers.
- Static review found no lint suppression attributes.

Output:
$actual
"@
            [System.IO.File]::WriteAllText($Review, $reviewText)
            Update-ArtifactManifest $actual
            $attestation = New-AtomHarnessAttestation -HarnessId "rust-console-exact-v1" -Gate "bluetooth-driver" -Artifact $Source -Executable $Exe -ExpectedOutput $Expected -AttestationPath (Join-Path $attemptDir "harness-attestation.json") -WorkingDirectory $attemptDir -WorkPlanId $work.PlanId -ProviderModel $work.Model
            $correctionEvidence = if ([string]::IsNullOrWhiteSpace($lastFailure)) { $durableCorrection } else { $lastFailure }
            Write-AtomLearningRecord -Source "provider-bluetooth-driver" -Intent "Build a Bluetooth driver" -Recipe "provider-model-loop" -Atoms "scan,project,compose,measure,preserve,order" -Gate "bluetooth-driver" -Attempt $attempt -Outcome "succeeded" -Correction $correctionEvidence -Artifact $Source -ProviderModel $work.Model -WorkPlanId $work.PlanId -WorkPlanManifest $work.Manifest -WorkPacketCount $work.PacketCount -HarnessAttestation $attestation.Path -HarnessAttestationHash $attestation.Hash
            Write-Host "provider bluetooth driver ok: $actual"
            Write-Host "driver review: $Review"
            return
        }
        catch {
            $lastFailure = $_.Exception.Message
            [System.IO.File]::WriteAllText((Join-Path $attemptDir "failure.txt"), $lastFailure)
            Write-AtomLearningRecord -Source "provider-bluetooth-driver" -Intent "Build a Bluetooth driver" -Recipe "provider-model-loop" -Atoms "scan,project,compose,measure,preserve,order" -Gate "bluetooth-driver" -Attempt $attempt -Outcome "failed" -Failure $lastFailure -ProviderModel $ProviderModel
            if ($attempt -eq $MaxAttempts) {
                throw "Bluetooth driver failed after $MaxAttempts attempts. Last failure: $lastFailure"
            }
        }
    }
}
finally {
    $env:MATH_ATOMS_PROVIDER_PROBE_INTENT = $OriginalProbeIntent
    $env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE = $OriginalTemplate
}
