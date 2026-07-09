# Math Atoms Coder

Math Atoms Coder is a local, recipe-first coding workbench for the Rekonquest atom doctrine. The current product path is the native PMRE shell backed by `math-atoms-core`: Spiderweb Bus routing, wiki graph retrieval, provider adapters, and proof state.

## Current Surface

- `atom-rendering-engine-main/math-atoms-core` owns the Spiderweb Bus, wiki graph RAG, provider adapters, recipes, and proof state.
- `atom-rendering-engine-main/math-atoms-native` is the native PMRE app shell; it does not use Chrome, Electron, Tauri, or browser-local state.
- `app/` is an archived legacy static doctrine mirror only; it is not a product runtime or production verification path.
- `scripts/doctrine-check.mjs` validates that archived mirror; it is not a functional readiness test.
- `scripts/Test-NativeFunctional.ps1` launches the real native window and exercises Run, Provider, and Drift.
- `scripts/Test-ProviderExecution.ps1` runs the configured provider through `math-atoms-core` and requires returned model text.
- `scripts/Test-RustCrateLineCaps.ps1` enforces the 4,000 Rust source-line cap per crate.
- `scripts/verify-production.ps1` is strict by default: warning-fatal Rust doctrine/tests, clippy, native build/artifact, and provider execution must all pass.

## Operator Mission

The product lane must meet or exceed Ornith 1.0 by selecting the smallest gate-passing native recipe from current Spiderweb Bus, wiki graph, provider, and proof evidence.

## Doctrine

The stable root atoms are `scan`, `hash`, `fold`, `project`, `scale`, `compare`, `combine`, and `order`.

The extended atoms under test are `transform`, `flow`, `preserve`, `compose`, `dual`, `measure`, `symmetrize`, and `superpose`.

Extended atoms must not be adopted as stable without a bench result. A bench result names the currency, runs a concrete test, compares before and after behavior, and records a verdict: `REAL`, `PAINTED`, `HURTS`, or `BASELINE`.

## Run

Run the native app:

```powershell
cd "C:\Projects\Atoms Coder by Lucerna Labs\atom-rendering-engine-main"
$env:RUSTFLAGS="-D warnings"
cargo run -p math-atoms-native --release
```

Provider selection:

```powershell
$env:MATH_ATOMS_PROVIDER_KIND="openai" # OPENAI_API_KEY, Responses API
$env:MATH_ATOMS_PROVIDER_KIND="ollama" # OLLAMA_API_KEY, Ollama Cloud API
```

Generate the native artifact:

```powershell
cd "C:\Projects\Atoms Coder by Lucerna Labs\atom-rendering-engine-main"
$env:RUSTFLAGS="-D warnings"
cargo run -p pmre-examples --example math_atoms_coder --release
```

## Verify

```powershell
.\scripts\verify-production.ps1
.\scripts\Test-NativeFunctional.ps1
```

For local structural debugging only, `.\scripts\verify-production.ps1 -AllowProviderBlock` skips the live provider gate and is not a production-ready result.
