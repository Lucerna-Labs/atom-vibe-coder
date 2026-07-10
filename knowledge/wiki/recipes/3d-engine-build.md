# 3D Engine Build Recipe
tags: 3d, engine, renderer, mesh, camera, rasterizer, lighting, animation, dependency-free, production, recipe

Status: reference recipe. The smallest implementation may be a software rasterizer, but proof requires real geometry, camera motion, depth, clipping, lighting, and interactive presentation. Related graph nodes: [[native-atom-renderer]], [[bus:spiderweb]], [[project]], [[compose]], [[combine]], [[flow]], [[preserve]], [[measure]], [[compare]], [[order]].

## Step 01 Freeze capabilities and coordinate conventions
Declare handedness, world and view axes, units, matrix layout, depth range, winding, culling, color space, precision, target backends, and unsupported features. Gate: one canonical triangle and camera transform has an independently calculated expected result.

## Step 02 Implement full-precision math primitives
Build vectors, matrices, quaternions, rays, planes, bounds, interpolation, normalization, and robust comparisons. Detect singular matrices, zero-length normalization, NaN, and overflow. Gate: identity, inverse, composition, projection, and quaternion property tests pass across randomized inputs.

## Step 03 Define generation-safe resource ownership
Create typed handles for meshes, textures, materials, cameras, nodes, and frame resources. Separate CPU ownership, upload state, live use, retirement, and destruction. Gate: stale, double-freed, wrong-type, and out-of-generation handles fail closed under concurrent lifecycle tests.

## Step 04 Validate and store mesh data
Represent positions, normals, tangents, texture coordinates, colors, indices, topology, and bounds with explicit formats and limits. Reject index overflow, mismatched attribute counts, non-finite vertices, and unsupported topology before render. Gate: valid and malformed procedural meshes cover every validation branch.

## Step 05 Build camera and projection paths
Implement view construction, perspective and orthographic projections, viewport mapping, frustum extraction, screen rays, resize, and DPI behavior. Gate: known world points map to expected screen/depth coordinates and unprojected rays intersect known geometry.

## Step 06 Clip before rasterization
Transform vertices through model, view, and clip space; clip complete primitives against the homogeneous frustum before perspective division. Preserve interpolants at generated vertices. Gate: near-plane crossings, behind-camera triangles, huge triangles, and every frustum edge render without cracks or invalid memory access.

## Step 07 Rasterize with depth correctness
Apply viewport transform, winding, culling, edge functions, top-left coverage, perspective-correct interpolation, depth test, and bounded pixel writes. Gate: overlapping geometry, shared edges, subpixel triangles, depth ties, and off-screen primitives match full-precision references.

## Step 08 Add materials and texture sampling
Define immutable material inputs, texture formats, addressing, filtering, mip policy, alpha mode, and fallback behavior. Validate every sample coordinate and resource generation. Gate: UV boundaries, transparent surfaces, missing textures, scaled textures, and color-space conversions match approved images.

## Step 09 Implement a declared lighting model
Start with ambient plus directional and point lights, normalized normals, attenuation, and a documented diffuse/specular model. Do not claim physically based rendering until energy, BRDF, environment, and material gates exist. Gate: light direction, distance, normal transforms, and material extremes produce calculated values.

## Step 10 Compose the scene graph
Store stable node IDs, parent/child relationships, local/world transforms, visibility, bounds, layers, and components. Detect cycles and update dirty subtrees in deterministic order. Gate: reparenting, deep hierarchies, deletion, hidden parents, and transform propagation agree with clean recomputation.

## Step 11 Add animation and skinning in bounded stages
Represent clips, tracks, keyframes, interpolation, playback state, blend weights, skeletons, and inverse bind matrices separately. Validate key order, joint indices, and weight normalization. Gate: fixed timestamps reproduce exact transforms and malformed rigs fail before frame execution.

## Step 12 Wire interaction and picking
Map native input through camera rays to bounded broad-phase and exact intersection tests. Preserve nearest-hit order and stable object identity. Gate: orbit, pan, zoom, resize, selection, occlusion, and overlapping objects work in the actual launched renderer.

## Step 13 Route the engine through the Spiderweb Bus
Use L0 for window/device transport, L1 for typed resource/input/frame messages, L2 for update-cull-render-present flows, and L3 for scene lifecycle, device recovery, and proof orchestration. Gate: load, interactive frame, resource failure, and recovery each produce auditable L0-L3 routes.

## Step 14 Bound performance without weakening quality
Measure transform, cull, clip, raster, shade, and present phases independently. Add tiling, dirty work, cache budgets, and parallel lanes only after deterministic baselines pass. Gate: optimizations are bitwise or tolerance-equivalent to the reference path and cannot reorder externally visible state.

## Step 15 Run production verification
Run math properties, malformed assets, golden frames, camera/resize matrices, depth/clipping suites, long animation sessions, stale resources, memory ceilings, idle CPU, active frame timing, and real interactive workflows. Capture executable and frame hashes plus L0-L3 route evidence. A static triangle smoke test is not functional proof.

