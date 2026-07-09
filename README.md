# Math Atoms Coder

Math Atoms Coder is a local, recipe-first coding workbench for the Rekonquest atom doctrine. The current workspace started as a doctrine note and UI reference; this scaffold turns that intent into a runnable static MVP.

## Current Surface

- `app/index.html` opens the app directly in a browser.
- `app/app-data.js` contains the 16 atom definitions, starter recipes, gates, and Spiderweb fabric nodes.
- `app/app.js` runs the proof loop, recipe capture, atom filtering, and bench verdict updates.
- `scripts/smoke.mjs` validates that the static app files and doctrine data are present.

## Operator Mission

The benchmark lane must match or beat DS4 by selecting the smallest gate-passing Q2/Q3/Q4/Q5/Q6/Q8 recipe from clean full-precision evidence.

## Doctrine

The stable root atoms are `scan`, `hash`, `fold`, `project`, `scale`, `compare`, `combine`, and `order`.

The extended atoms under test are `transform`, `flow`, `preserve`, `compose`, `dual`, `measure`, `symmetrize`, and `superpose`.

Extended atoms must not be adopted as stable without a bench result. A bench result names the currency, runs a concrete test, compares before and after behavior, and records a verdict: `REAL`, `PAINTED`, `HURTS`, or `BASELINE`.

## Run

Open `C:\Projects\Atoms Coder by Lucerna Labs\app\index.html`.

No package install is required for the MVP.

## Verify

```powershell
node --check app\app-data.js
node --check app\app.js
node scripts\smoke.mjs
```
