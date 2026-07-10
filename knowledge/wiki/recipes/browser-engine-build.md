# Browser Engine Build Recipe
tags: browser, engine, html, css, layout, networking, security, accessibility, incomplete, draft, recipe

Status: incomplete reference only. This recipe may guide decomposition, but it is not a proven or complete browser implementation and must never be promoted as production evidence. Script execution, process isolation, standards coverage, sandboxing, media, and many web-platform surfaces remain open gates. Related graph nodes: [[bus:spiderweb]], [[scan]], [[project]], [[compose]], [[flow]], [[preserve]], [[measure]], [[compare]], [[order]], [[hash]].

## Step 01 Freeze a deliberately narrow browser contract
Declare URL schemes, HTML/CSS subset, script policy, networking, storage, accessibility, security model, rendering target, platform targets, and explicit non-goals. A document viewer is not a general browser. Gate: every unsupported web feature has deterministic fallback or failure behavior.

## Step 02 Build URL and origin primitives
Parse and serialize URLs, resolve relative references, normalize hosts and paths, distinguish opaque origins, and enforce scheme policy without string concatenation shortcuts. Gate: a standards-derived corpus covers malformed input, Unicode hosts, ports, fragments, credentials, and relative resolution.

## Step 03 Build bounded network loading
Use audited OS TLS and networking facilities, enforce redirects, timeouts, cancellation, response limits, MIME handling, decompression limits, cache policy, and certificate failures. Gate: local real servers exercise redirects, chunking, partial responses, oversized bodies, invalid TLS, cancellation, and offline behavior.

## Step 04 Decode bytes into text safely
Define encoding detection policy, byte-order behavior, invalid-sequence replacement, newline normalization, and maximum document size. Gate: split multibyte sequences, malformed bytes, declared/actual encoding conflicts, and large streams cannot panic or bypass limits.

## Step 05 Tokenize HTML incrementally
Implement tokenizer states, entities, comments, raw text, attributes, EOF, and parse errors as a streaming state machine. Do not parse HTML with ad hoc splitting. Gate: tokenizer fixtures and chunk-boundary variants produce the same tokens.

## Step 06 Construct and mutate the document tree
Implement insertion modes, implied elements, foster parenting where supported, stable node identity, attributes, text, mutation, and bounded depth. Gate: malformed nesting, tables, duplicate attributes, deep input, and incremental chunks match the declared subset fixtures.

## Step 07 Parse CSS into typed rules
Tokenize CSS, parse selectors, declarations, values, at-rules, errors, and source order independently. Preserve unknown declarations without executing them. Gate: malformed rules recover at documented boundaries and selector/value corpora cannot hang or allocate without bounds.

## Step 08 Compute cascade and inherited style
Implement selector matching, specificity, origin, importance, source order, inheritance, initial values, units, variables if declared, and computed-value validation. Gate: focused cascade matrices calculate exact winners and cycles or invalid values fail deterministically.

## Step 09 Build layout in isolated formatting contexts
Start with a declared block/inline subset, intrinsic sizing, margins, padding, borders, overflow, line construction, replaced elements, and scrolling. Add flex/grid only as separate recipes with their own tests. Gate: resize, long words, nested boxes, overflow, scrolling, and DPI fixtures match approved geometry.

## Step 10 Produce paint and hit-test lists
Convert layout into ordered backgrounds, borders, text, images, clips, stacking contexts, and hit regions. Keep paint data immutable for a frame. Gate: z-order, clipping, opacity, scroll offsets, selection, and overlapping links agree between pixels and hit testing.

## Step 11 Render through the native 2D engine
Feed the paint list into the dependency-free 2D recipe rather than duplicating raster, typography, input, and animation logic. Gate: browser rendering inherits every 2D production gate and adds page-specific golden documents. [[wiki:recipes:2d-engine-build]]

## Step 12 Implement navigation and lifecycle
Separate requested, fetching, committing, active, failed, stopped, and history states. Cancel prior work, isolate document generations, and prevent stale network/layout callbacks from mutating the active page. Gate: rapid navigation, back/forward, reload, stop, errors, and redirects preserve one active generation.

## Step 13 Add input, focus, selection, and accessibility
Map pointer, keyboard, text, clipboard, focus order, scrolling, selection, caret, links, and semantic accessibility nodes to the document. Gate: keyboard-only navigation, selection/copy, zoom, screen-reader tree inspection, and focus across navigation work in the real window.

## Step 14 Enforce browser security boundaries
Define origin checks, mixed-content policy, navigation permissions, download handling, local-file policy, cookie policy, storage partitioning, CSP scope if supported, and untrusted-content isolation. Gate: adversarial pages cannot read prohibited resources, escape paths, inject privileged bus commands, or persist secrets.

## Step 15 Route subsystems through the Spiderweb Bus
Use L0 for sockets/window transport, L1 for typed fetch/parser/input messages, L2 for navigation-parse-style-layout-paint flows, and L3 for document lifecycle, cancellation, recovery, and proof. Treat page bytes and model output as untrusted data at every ramp. Gate: navigation and interaction produce complete L0-L3 routes without granting content orchestration authority.

## Step 16 Open blocker: script runtime is incomplete
No JavaScript parser, VM, event loop, DOM binding, garbage collector, timers, promises, modules, or Web API compatibility layer is proven here. Do not substitute expression evaluation or provider-generated code. Production browser claims remain blocked until a separately validated runtime and conformance suite exist.

## Step 17 Open blocker: sandbox and process isolation are incomplete
An in-process parser and renderer do not constitute a hardened browser. Site isolation, brokered privileges, renderer sandboxing, IPC validation, crash containment, exploit mitigations, and update response are unresolved. Production browser claims remain blocked.

## Step 18 Open blocker: web-platform coverage is incomplete
Forms, downloads, cookies, cache, accessibility depth, international text, SVG, canvas, media, WebSocket, workers, printing, extensions, devtools, privacy controls, and standards suites are not complete. Each surface requires its own recipe, threat model, limits, and real tests.

## Step 19 Incomplete-recipe verification rule
The current acceptable result is a clearly scoped native document browser that passes its declared subset tests. It must label itself incomplete and cannot be learned as general browser success. Promotion requires resolving every open blocker, passing standards and security suites, running hostile-content campaigns, and verifying real navigation, rendering, input, accessibility, persistence, crash recovery, and long sessions.
