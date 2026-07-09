# Math Atoms Coder

Math Atoms Coder is a local, recipe-first coding workbench for the Rekonquest atom doctrine. The current workspace combines a runnable static operator surface with a recovered PMRE native renderer that can generate the diagram as a proof artifact.

## Current Surface

- `app/index.html` opens the app directly in a browser.
- `app/app-data.js` contains the 16 atom definitions, starter recipes, gates, and Spiderweb fabric nodes.
- `app/app.js` runs the proof loop, recipe capture, atom filtering, and bench verdict updates.
- `scripts/smoke.mjs` validates that the static app files and doctrine data are present; it is not a functional readiness test.
- `atom-rendering-engine-main` contains the Rust PMRE engine and the native `math_atoms_coder` artifact renderer.
- `scripts/verify-production.ps1` runs the current static and Rust baseline gate and regenerates `math_atoms_coder.bmp`.

## Operator Mission

The benchmark lane must match or beat DS4 by selecting the smallest gate-passing Q2/Q3/Q4/Q5/Q6/Q8 recipe from clean full-precision evidence.

## Doctrine

The stable root atoms are `scan`, `hash`, `fold`, `project`, `scale`, `compare`, `combine`, and `order`.

The extended atoms under test are `transform`, `flow`, `preserve`, `compose`, `dual`, `measure`, `symmetrize`, and `superpose`.

Extended atoms must not be adopted as stable without a bench result. A bench result names the currency, runs a concrete test, compares before and after behavior, and records a verdict: `REAL`, `PAINTED`, `HURTS`, or `BASELINE`.

## Run

Open `C:\Projects\Atoms Coder by Lucerna Labs\app\index.html`.

No package install is required for the MVP.

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
node scripts\smoke.mjs
.\scripts\verify-production.ps1
```
