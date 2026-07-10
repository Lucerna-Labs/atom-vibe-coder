# 2D Engine Build Recipe
tags: 2d, engine, renderer, graphics, raster, vector, typography, animation, dependency-free, production, recipe

Status: reference recipe. A generated implementation is not proven until every functional gate below passes with a real rendered artifact. Related graph nodes: [[native-atom-renderer]], [[bus:spiderweb]], [[project]], [[compose]], [[flow]], [[preserve]], [[measure]], [[compare]], [[order]].

## Step 01 Freeze the render contract
Define supported targets, pixel formats, alpha convention, color space, coordinate origin, DPI behavior, maximum surface size, frame pacing, and deterministic output rules. Reject an unbounded "support all graphics" scope. Gate: a machine-readable capability table and unsupported-feature errors exist before implementation.

## Step 02 Build numeric and coordinate primitives
Implement points, vectors, rectangles, affine transforms, bounds, interpolation, clamping, and robust float-to-pixel conversion. Normalize NaN, infinity, negative dimensions, and overflow at the boundary. Gate: property tests cover transform composition, inverse behavior, clipping, and extreme coordinates.

## Step 03 Own the surface and frame lifecycle
Create an explicit surface with width, height, stride, pixel format, immutable capacity, clear operation, dirty region, and present lifecycle. Separate drawing memory from display transport. Gate: repeated create-clear-present-destroy cycles show no stale pixels, leaks, or out-of-bounds writes.

## Step 04 Rasterize core primitives
Implement clipped pixels, lines, polylines, rectangles, rounded rectangles, circles, ellipses, arcs, and polygons. Use one coverage and clipping policy across primitives. Gate: golden images cover every edge, off-screen geometry, zero-size geometry, and overlapping shapes at multiple DPI values.

## Step 05 Add paths and arbitrary shapes
Represent move, line, quadratic, cubic, close, fill rule, stroke width, joins, caps, and dash state as bounded path commands. Flatten curves with an error tolerance tied to device scale. Gate: self-intersections, holes, sharp joins, degenerate curves, and very large paths match approved reference pixels.

## Step 06 Implement paint and compositing
Support solid colors, linear and radial gradients, image patterns, opacity, masks, clipping stacks, and declared blend modes. Convert colors into one internal linear representation before blending. Gate: alpha, gradient stops, nested clips, and blend equations are compared against full-precision reference calculations.

## Step 07 Build typography as a separate subsystem
Define font discovery, fallback, metrics, shaping boundary, glyph cache, baseline, wrapping, alignment, selection, caret, and decoration behavior. Platform text APIs may be reached through narrow native FFI; missing shaping support must fail explicitly. Gate: multilingual samples, long words, selection, copy/paste, deletion, and blinking caret behavior are exercised in the real window.

## Step 08 Decode and sample images safely
Keep image decoding outside the raster hot path. Validate dimensions and byte counts before allocation; represent decoded images with explicit format and ownership. Implement nearest and bilinear sampling with source and destination clipping. Gate: malformed, oversized, transparent, scaled, and partially off-screen images cannot panic or escape bounds.

## Step 09 Compose a retained scene
Represent scene nodes, transforms, z-order, clips, opacity, hit regions, and stable IDs independently from drawing commands. Recompute only invalidated bounds and preserve deterministic order. Gate: adding, removing, reordering, and mutating nodes produces the same pixels as a clean full redraw.

## Step 10 Wire input and hit testing
Map native pointer, wheel, keyboard, text, focus, clipboard, and accessibility events into scene coordinates. Hit test in reverse paint order and maintain capture/focus invariants. Gate: dragging, overlapping controls, scroll offsets, keyboard editing, clipboard operations, and focus transitions work in the launched application.

## Step 11 Add deterministic animation
Separate monotonic time, animation state, easing, interpolation, invalidation, and presentation. Pause hidden surfaces and cap catch-up work after stalls. Gate: recorded timelines produce repeatable frame states, no layout shift changes fixed control geometry, and idle CPU remains within the declared budget.

## Step 12 Route work through the Spiderweb Bus
Use L0 for native transport events, L1 for typed render/input messages, L2 for scene-update and frame flows, and L3 for lifecycle, recovery, and proof orchestration. Declare ramps between input, scene, raster, text, and presentation owners. Gate: a real frame and a real interaction both produce complete L0-L3 route evidence.

## Step 13 Bound caches and failure behavior
Give glyph, path, image, and layer caches explicit byte budgets, deterministic eviction, generation-safe handles, and observable hit/miss counters. Allocation failure, invalid handles, and unsupported operations return blockers instead of fallback output. Gate: forced pressure and stale-handle tests preserve correctness.

## Step 14 Run production verification
Run unit and property tests, pixel goldens, malformed-input tests, resize/DPI matrices, long-session memory checks, input editing, accessibility checks, idle and active CPU measurements, and real launched-window workflows. Smoke rendering alone cannot pass. Record exact executable, artifact hashes, test matrix, and captured route evidence before promotion.

