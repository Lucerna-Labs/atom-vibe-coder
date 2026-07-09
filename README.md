# Math Atoms Coder

Math Atoms Coder is a local, recipe-first coding workbench for the Rekonquest atom doctrine. The current product path is the native PMRE shell backed by `math-atoms-core`: Spiderweb Bus routing, wiki graph retrieval, provider adapters, and proof state.

## Current Surface

- `atom-rendering-engine-main/math-atoms-core` owns the Spiderweb Bus, wiki graph RAG, provider adapters, recipes, and proof state.
- `atom-rendering-engine-main/math-atoms-native` is the native PMRE app shell; it does not use Chrome, Electron, Tauri, or browser-local state.
- `app/` is a legacy static doctrine mirror for quick inspection only.
- `scripts/doctrine-check.mjs` validates legacy doctrine data; it is not a functional readiness test.
- `scripts/verify-production.ps1` runs the current static and Rust baseline gate, builds the native shell, and regenerates `math_atoms_coder.bmp`.

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
node --check app\app-data.js
node --check app\app.js
node scripts\doctrine-check.mjs
.\scripts\verify-production.ps1
```
