# Atom Vibe Coder

Atom Vibe Coder by Lucerna Labs is a local, recipe-first coding workbench for the Rekonquest atom doctrine. The current product path is the native PMRE shell backed by `math-atoms-core`: Spiderweb Bus routing, wiki graph retrieval, provider adapters, and proof state.

## Current Surface

- `vibe-coder/` owns the Atom Vibe Coder harness. It is a separate Rust workspace
  that consumes renderer crates without making the renderer responsible for
  planning, provider turns, Wiki Graph RAG, scratchpads, or build gates.
- `vibe-coder/atom-vibe-build-protocol` owns the fixed six-step build protocol.
- `vibe-coder/atom-vibe-build-gates` owns artifact-backed pass/fail decisions.
- `vibe-coder/atom-vibe-build-planner` owns persistent state, bounded correction,
  restart recovery, and planner-visible Spiderweb routes.
- `vibe-coder/atom-vibe-context` joins Wiki Graph RAG and model-scoped scratchpad
  context on a complete L0-L3 Spiderweb route for every build step.
- `vibe-coder/atom-vibe-mode` owns the product mode and progressive skill release.
- `vibe-coder/atom-vibe-provider` owns direct, thinking-required model turns over
  the credential-safe renderer transport.
- `vibe-coder/atom-vibe-scratchpad` owns isolated, append-only working context for
  one build and one provider-model identity. It does not replace graph RAG.
- `atom-rendering-engine-main/math-atoms-attestation` owns allowlisted executable harness runs and immutable source/executable/output attestations for learning authority.
- `atom-rendering-engine-main/math-atoms-bus` owns dependency-free L0-L3 Spiderweb Bus routing, ramps, intersections, backpressure, and fabric-thread evidence.
- `atom-rendering-engine-main/math-atoms-core` owns provider adapters, recipe selection, proof state, and runtime orchestration over the bus.
- `atom-rendering-engine-main/math-atoms-graph` owns graph-native wiki chunking, relationship retrieval, proof promotion, and bounded learning-node memory.
- `atom-rendering-engine-main/math-atoms-hash` owns dependency-free SHA-256 hashing for recomputable provider and artifact evidence.
- `atom-rendering-engine-main/math-atoms-learning` owns the append-only learning ledger, concurrent writer lock, artifact hashing, bounded memory, relevance ranking, redaction, and `learning_probe` CLI.
- `atom-rendering-engine-main/math-atoms-lock` owns kernel-released cross-process leases and auditable process-start owner tokens for work, learning, and proof stores.
- `atom-rendering-engine-main/math-atoms-json` fully parses provider and ledger JSON, including Unicode surrogate pairs, duplicate-key rejection, depth limits, and trailing-data rejection, without third-party dependencies.
- `atom-rendering-engine-main/math-atoms-native` is the native PMRE app shell; it does not use Chrome, Electron, Tauri, or browser-local state.
- `atom-rendering-engine-main/math-atoms-proof` owns the strict append-only proof ledger and backward-compatible proof-record schema.
- `atom-rendering-engine-main/math-atoms-provider-transport` owns bounded credential-safe single-submit HTTP execution and content-addressed provider output evidence.
- `atom-rendering-engine-main/math-atoms-secrets` owns format-preserving credential redaction for every durable evidence boundary.
- `atom-rendering-engine-main/math-atoms-work` owns strict, resumable, content-addressed provider work packets and expanded-plan verification.
- `app/` is an archived legacy static doctrine mirror only; it is not a product runtime or production verification path.
- `scripts/doctrine-check.mjs` validates that archived mirror; it is not a functional readiness test.
- `scripts/Test-NativeFunctional.ps1` launches the real native window against an isolated temp proof store and exercises typed intent routing, Run, Capture, Provider, and Drift.
- `scripts/Test-NativeProviderResponsiveness.ps1` launches the native app against a slow local provider and proves the window remains responsive while provider execution is running.
- `scripts/Test-ProviderExecution.ps1` runs the configured provider through `math-atoms-core` and requires returned model text.
- `scripts/Test-ProviderBuildSeveralApps.ps1` asks the configured provider to generate several tiny Rust fixtures, compiles them, runs them, and writes side-window artifact rows.
- `scripts/Test-ProviderBuildRealPmreApp.ps1` gives the configured provider only a natural-language app request, validates the returned product spec, compiles that spec through the harness-owned PMRE scaffold, drives UI events, writes a BMP artifact, and adds it to the native side artifact window.
- `scripts/Test-ProviderBuildBluetoothDriver.ps1` generates, compiles, runs, and statically reviews a dependency-free Bluetooth HCI driver core.
- `scripts/Test-DesignUploadBuild.ps1` accepts uploaded HTML/CSS file paths, compiles a PMRE app that embeds the design, renders it through `render_html`, validates the BMP, and adds it to the native side artifact window.
- `scripts/Test-RustCrateLineCaps.ps1` enforces the 4,000 Rust source-line cap per crate.
- `scripts/Test-SelfLearningFunctional.ps1` proves failed and corrected attempts survive separate processes, redact token-like secrets, hash artifacts, and re-enter Wiki Graph RAG after restart.
- `scripts/Test-ProviderLearningLocal.ps1` runs the real provider adapter, console-app, PMRE-app, Bluetooth, and learning gates against an isolated local endpoint.
- `scripts/Test-NativeLaunchEnvironment.ps1` proves the detached Win32 launcher inherits session-only provider and store settings.
- `scripts/Test-NativeIdleCpu.ps1` measures the real minimized native process and rejects background rerender loops.
- `scripts/Test-WorkPacketResume.ps1` executes every provider packet, takes the endpoint offline, then requires the identical plan to resume the full canonical DAG from revalidated evidence without a network request.
- `scripts/Launch-Native.ps1` builds when needed and launches the native PMRE app through an environment-preserving detached Win32 process, preferring job breakaway when Windows permits it, with an explicit working directory.
- `scripts/verify-production.ps1` is strict by default: warning-fatal Rust doctrine/tests, clippy, native build/artifact, and provider execution must all pass.
- The interactive PMRE renderer auto-injects a dependency-free `Design` rail into every `render_ui` surface. The rail opens a native customization panel with hue, saturation, light, text scale, radius, glass/frost, animation, typography, control-shape, palette, button, and toggle controls.
- Atom stack order is a production gate. Recipes are scored by canonical stack order, proof state records the selected stack, and provider app-build gates reject shuffled or missing stacks.

## Durable Self-Learning

Every terminal native or build-harness attempt appends a validated event to `learning.jsonl`. Failed events remain correction evidence and cannot promote a recipe as proof. Model completion remains `verification pending`; it is not a successful learning event. Schema-v4 provider successes require an allowlisted executable harness attestation, existing SHA-256 source and executable artifacts, exact expected output, a recomputable canonical expanded work manifest, and every model-bound packet artifact; native non-provider successes require a complete L0-L3 route. Immediate retries receive the current failure, while later runs retrieve related durable lessons through recipe and atom relationships before the provider request is prepared. Legacy schema-v1/v2/v3 records remain readable audit history but cannot promote provider evidence.

Learning events move through explicit L0 observation, L1 persistence, L2 graph joining, and L3 orchestration messages. The active graph memory is deduplicated and capped at 256 learning nodes; the append-only ledger remains the audit history. Provider prompts label retrieved evidence as untrusted historical data so stored text cannot become executable prompt instructions.

## Meticulous Work Packets

One natural-language request always enters a fine-grained provider plan. Five base packets normalize intent, derive functional and quality contracts, define architecture, and return a strict relative-path file manifest. Every file receives an initial review/correction, an integration-aware correction tied to its authoritative contract, and a final correction after functional and hostile review. Bounded three-input closure and release groups keep the context usable by small models. A one-file product requires 19 packets; larger products add six focused packets per file plus bounded integration, closure, and release groups. There is no one-shot provider bypass.

Planning packets accept only exact ID-bound JSON schemas. File packets accept exactly one complete fenced file, reject credential material and incomplete-code markers, and otherwise remain byte-for-byte unchanged. Trusted packet control is sent in the provider system/instructions role; the operator request, graph evidence, and prior output are encoded as data in a separate user role. The schema-v3 work manifest is verified by reconstructing the canonical packet ID and dependency graph. The final deliverable is assembled only from final-correction packets, so a small-context model never has to regenerate an entire multi-file product in one response.

## Complex Build Recipes

The wiki graph indexes every numbered section as a separate bounded evidence node. Step-by-step references cover the dependency-free [2D engine](knowledge/wiki/recipes/2d-engine-build.md), [3D engine](knowledge/wiki/recipes/3d-engine-build.md), and [WiFi adapter](knowledge/wiki/recipes/wifi-adapter-build.md). The [browser engine](knowledge/wiki/recipes/browser-engine-build.md) is explicitly incomplete and cannot be treated as general-browser production proof.

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

Atom Vibe Coder requires a reasoning-capable model. The recommended minimum
local baseline is **Qwen3.5 9B Q8 with thinking enabled**, or a demonstrably
stronger thinking model. Lower quantizations may be used for diagnostics, but
they do not qualify a production build. Thinking is mandatory for every intake,
planning, implementation, review, repair, test, and launch-verification turn.
The adapter accepts `low`, `medium`, or `high`, rejects disabled thinking and
maximum/xhigh modes, and fails closed when the response contains no reasoning
evidence. See [Provider and Model Requirements](vibe-coder/docs/PROVIDER_MODELS.md).

```powershell
$env:MATH_ATOMS_PROVIDER_KIND="openai"  # OPENAI_API_KEY, Responses API
$env:MATH_ATOMS_PROVIDER_KIND="ollama"  # OLLAMA_API_KEY, Ollama Cloud chat API
$env:MATH_ATOMS_PROVIDER_KIND="mistral" # MISTRAL_API_KEY, Mistral chat completions API
$env:MATH_ATOMS_PROVIDER_KIND="deepseek" # DEEPSEEK_API_KEY, DeepSeek V4 Pro with provider-default thinking

# Any OpenAI-compatible/custom provider:
$env:MATH_ATOMS_PROVIDER_KIND="custom"
$env:MATH_ATOMS_PROVIDER_FORMAT="chat" # responses | chat | ollama-chat
$env:MATH_ATOMS_PROVIDER_MODEL="provider-model-name"
$env:MATH_ATOMS_PROVIDER_URL="https://provider.example/v1/chat/completions"
$env:MATH_ATOMS_PROVIDER_KEY_ENV="MY_PROVIDER_API_KEY"
$env:MATH_ATOMS_PROVIDER_AUTH_HEADER="Authorization"
$env:MATH_ATOMS_PROVIDER_AUTH_SCHEME="Bearer" # use raw/none for x-api-key style headers
$env:MATH_ATOMS_PROVIDER_THINKING_LEVEL="low" # low | medium | high; always on, never max/xhigh
$env:MATH_ATOMS_PROVIDER_RESPONSE_KEY="answer" # optional top-level response key for a custom provider
$env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE='{"model":{{model_json}},"system":{{instructions_json}},"data":{{data_json}},"reasoning_effort":{{thinking_json}}}'
$env:MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS="900" # optional, bounded to 10..1800 seconds
$env:MATH_ATOMS_PROVIDER_PLAN_TIMEOUT_SECONDS="21600" # optional total plan budget, bounded to 60..86400 seconds
```

The native PMRE app also exposes provider kind, wire format, model, endpoint, key-env, auth-header, auth-scheme, thinking level, response-key, and body-template controls. Leave the body template blank for the selected wire format defaults; set it only for providers that need a custom JSON shape. A custom template must carry `{{instructions_json}}`, `{{data_json}}`, and `{{thinking_json}}`. `Apply Provider` reloads provider config in the runtime and clears stale proof state before the next run. See [Provider Runtime Requirements](atom-rendering-engine-main/docs/PROVIDER_RUNTIME.md) and the harness-owned [Provider and Model Requirements](vibe-coder/docs/PROVIDER_MODELS.md).

The native Settings tab also exposes `Design Upload` with HTML and CSS path inputs. `Build Design` runs the PMRE design-upload gate, compiles the uploaded design into a native renderer app, and refreshes the side artifact window.

Every generated interactive PMRE app also gets the renderer-owned `Design` rail without app-specific wiring. Open it to tune colors, text scale, typography preset, rounded/square/pill/circle control shape, glass intensity, animation intensity, and preview controls such as mic, mute, and record buttons. The controls are native PMRE widgets, not browser/Electron/Tauri widgets.

Run a real DeepSeek V4 Pro thinking-model test that asks the provider to generate a dependency-free Rust toy app through meticulous packets, compiles it with `rustc`, runs it, and verifies output:

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
