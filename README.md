# Atom Vibe Coder

Atom Vibe Coder by Lucerna Labs is a local, recipe-first coding workbench for the Rekonquest atom doctrine. The current product path is the native PMRE shell backed by `math-atoms-core`: Spiderweb Bus routing, wiki graph retrieval, provider adapters, and proof state.

## Current Surface

- `atom-rendering-engine-main/math-atoms-core` owns the Spiderweb Bus, wiki graph RAG, provider adapters, recipes, and proof state.
- `atom-rendering-engine-main/math-atoms-native` is the native PMRE app shell; it does not use Chrome, Electron, Tauri, or browser-local state.
- `app/` is an archived legacy static doctrine mirror only; it is not a product runtime or production verification path.
- `scripts/doctrine-check.mjs` validates that archived mirror; it is not a functional readiness test.
- `scripts/Test-NativeFunctional.ps1` launches the real native window against an isolated temp proof store and exercises typed intent routing, Run, Capture, Provider, and Drift.
- `scripts/Test-NativeProviderResponsiveness.ps1` launches the native app against a slow local provider and proves the window remains responsive while provider execution is running.
- `scripts/Test-ProviderExecution.ps1` runs the configured provider through `math-atoms-core` and requires returned model text.
- `scripts/Test-ProviderBuildSeveralApps.ps1` asks the configured provider to generate several tiny Rust fixtures, compiles them, runs them, and writes side-window artifact rows.
- `scripts/Test-ProviderBuildRealPmreApp.ps1` gives the configured provider only a natural-language app request, validates the returned product spec, compiles that spec through the harness-owned PMRE scaffold, drives UI events, writes a BMP artifact, and adds it to the native side artifact window.
- `scripts/Test-DesignUploadBuild.ps1` accepts uploaded HTML/CSS file paths, compiles a PMRE app that embeds the design, renders it through `render_html`, validates the BMP, and adds it to the native side artifact window.
- `scripts/Test-RustCrateLineCaps.ps1` enforces the 4,000 Rust source-line cap per crate.
- `scripts/Launch-Native.ps1` builds when needed and launches the native PMRE app.
- `scripts/verify-production.ps1` is strict by default: warning-fatal Rust doctrine/tests, clippy, native build/artifact, and provider execution must all pass.
- The interactive PMRE renderer auto-injects a dependency-free `Design` rail into every `render_ui` surface. The rail opens a native customization panel with hue, saturation, light, text scale, radius, glass/frost, animation, typography, control-shape, palette, button, and toggle controls.
- Atom stack order is a production gate. Recipes are scored by canonical stack order, proof state records the selected stack, and provider app-build gates reject shuffled or missing stacks.

## Product Mission

The product lane must select the smallest gate-passing native recipe from current Spiderweb Bus, wiki graph, provider, and proof evidence that matches the actual app the user asked to build.

## Doctrine

The stable root atoms are `scan`, `hash`, `fold`, `project`, `scale`, `compare`, `combine`, and `order`.

The extended atoms under test are `transform`, `flow`, `preserve`, `compose`, `dual`, `measure`, `symmetrize`, and `superpose`.

Extended atoms must not be adopted as stable without a bench result. A bench result names the currency, runs a concrete test, compares before and after behavior, and records a verdict: `REAL`, `PAINTED`, `HURTS`, or `BASELINE`.

## Run

Run the native app:

```powershell
cd "C:\Projects\Atoms Coder by Lucerna Labs"
.\scripts\Launch-Native.ps1 -Build -Restart
```

Provider selection:

```powershell
$env:MATH_ATOMS_PROVIDER_KIND="openai"  # OPENAI_API_KEY, Responses API
$env:MATH_ATOMS_PROVIDER_KIND="ollama"  # OLLAMA_API_KEY, Ollama Cloud chat API
$env:MATH_ATOMS_PROVIDER_KIND="mistral" # MISTRAL_API_KEY, Mistral chat completions API
$env:MATH_ATOMS_PROVIDER_KIND="deepseek" # DEEPSEEK_API_KEY, DeepSeek V4 Flash chat API

# Any OpenAI-compatible/custom provider:
$env:MATH_ATOMS_PROVIDER_KIND="custom"
$env:MATH_ATOMS_PROVIDER_FORMAT="chat" # responses | chat | ollama-chat
$env:MATH_ATOMS_PROVIDER_MODEL="provider-model-name"
$env:MATH_ATOMS_PROVIDER_URL="https://provider.example/v1/chat/completions"
$env:MATH_ATOMS_PROVIDER_KEY_ENV="MY_PROVIDER_API_KEY"
$env:MATH_ATOMS_PROVIDER_AUTH_HEADER="Authorization"
$env:MATH_ATOMS_PROVIDER_AUTH_SCHEME="Bearer" # use raw/none for x-api-key style headers
$env:MATH_ATOMS_PROVIDER_RESPONSE_KEY="output_text" # output_text | text | response | content | custom key
$env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE='{"model":{{model_json}},"prompt":{{prompt_json}}}'
```

The native PMRE app also exposes provider kind, wire format, model, endpoint, key-env, auth-header, auth-scheme, response-key, and body-template controls. Leave the body template blank for the selected wire format defaults; set it only for providers that need a custom JSON shape. `Apply Provider` reloads provider config in the runtime and clears stale proof state before the next run.

The native Settings tab also exposes `Design Upload` with HTML and CSS path inputs. `Build Design` runs the PMRE design-upload gate, compiles the uploaded design into a native renderer app, and refreshes the side artifact window.

Every generated interactive PMRE app also gets the renderer-owned `Design` rail without app-specific wiring. Open it to tune colors, text scale, typography preset, rounded/square/pill/circle control shape, glass intensity, animation intensity, and preview controls such as mic, mute, and record buttons. The controls are native PMRE widgets, not browser/Electron/Tauri widgets.

Run a real DeepSeek Flash model test that asks the provider to generate a dependency-free Rust toy app, compiles it with `rustc`, runs it, and verifies output:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\Test-DeepSeekToyApp.ps1
```

Run the stricter app-build gate. The provider receives only a natural-language user request; Atom Vibe Coder owns the PMRE renderer scaffold, compiles the resulting app, exercises UI events, writes a BMP, and exposes it in the native side artifact window:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\Test-ProviderBuildRealPmreApp.ps1
```

Build a native PMRE app from uploaded HTML/CSS design files and render its side artifact:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\Test-DesignUploadBuild.ps1 -HtmlPath C:\path\design.html -CssPath C:\path\design.css
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
