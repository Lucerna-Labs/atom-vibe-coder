# Transparency Cookbook Coverage

This is the renderer's implementation map for the reviewed *Everything Transparent &
Translucent — Rendering Cookbook v2*. It distinguishes live PMRE behavior from reusable math and
from recipes that require a geometry/depth/ray pipeline the 2D renderer does not have.

## Live rendering path

An ordinary frame remains unchanged:

```text
layout -> painter order -> parallel row lanes -> straight-alpha framebuffer -> output gamma
```

When a `Style` contains a `TransparencyMaterial`, that box becomes a backdrop barrier:

```text
capture prior backdrop crop
  -> premultiply RGBA
  -> separable blur
  -> refract + per-channel dispersion
  -> Beer-Lambert absorption + tint/scatter + thin-film RGB
  -> Schlick Fresnel rim + border
  -> selected blend policy
  -> convert to the explicit straight-alpha framebuffer boundary
```

The material path is serial because a row-band view cannot safely sample adjacent rows or pixels
owned by another lane. Frames without a material still use the existing parallel renderer.

## Renderer Customizer presets

`Legacy` is selected by default and attaches no material. The remaining presets are live:

| Preset | Main behavior |
|---|---|
| Clear glass | Low roughness, glass IOR, light refraction and rim |
| Frosted glass | Strong premultiplied backdrop blur and scattering |
| Water | Water IOR, cyan transmission, stronger screen refraction |
| Crystal | Glass refraction, absorption, RGB dispersion and bright rim |
| Soap film | Thin-walled glass with RGB thin-film interference |
| Wax | Warm absorption, broad blur and cheap subsurface-style scatter |
| Smoke | Soft-add policy, blur, absorption, scatter and distortion; animation follows the Animation control |
| Stained glass | Strong colored Beer-Lambert transmission |
| Heat haze | Thin-walled refraction/distortion with no rim; animation follows the Animation control |

The existing `Glass` slider controls material strength. `Advanced optics` is collapsed by default
and exposes frost/blur, refraction, RGB dispersion, and Fresnel-rim overrides. Selecting a new
preset clears those overrides so recipes remain deterministic.

## Programmatic API

```rust
use pmre_kit::{transparency::MaterialPreset, Rgba, Style};

let pane = Style::col()
    .radius(16.0)
    .bg(Rgba::new(0.12, 0.70, 0.84, 0.55))
    .transparency(MaterialPreset::FrostedGlass.material());
```

`pmre-transparency-core` has no Cargo dependencies, forbids unsafe code, uses `#![no_std]`, and
allocates nothing. It exposes explicit `StraightRgba` and `PremulRgba` types so alpha conventions
cannot be mixed accidentally. `pmre-transparency` depends only on internal PMRE crates and owns the
framebuffer crop/filter/composite mechanism.

## Capability matrix

| Cookbook topic | Status | PMRE boundary |
|---|---|---|
| Premultiplied alpha | Live | Material filtering and blend math use explicit premultiplied values |
| Painter sorting | Live | Existing pre-order painter path is deterministic back-to-front order |
| Alpha, additive, multiply, soft-add/screen | Live | Public material blend policy |
| Alpha cutoff and ordered dithering | Live | `Cutout`, `Dither`, and 4x4 Bayer threshold |
| Schlick Fresnel | Live | Drives material rim/reflection weight |
| Exact dielectric Fresnel | Math only | Tested value API; no ray recursion |
| Snell refraction and TIR | Math only | Tested vector API; live path is screen-space offset |
| Beer-Lambert absorption | Live | Colored transmission through material thickness |
| Thin-walled transmission | Live | Shortened absorption path and one-surface screen treatment |
| Rough/frosted transmission | Live approximation | Premultiplied separable backdrop blur |
| RGB dispersion | Live approximation | Three offset backdrop samples, not spectral transport |
| Thin-film interference | Live approximation | Three representative RGB wavelengths |
| Screen-space refraction | Live approximation | Bounded backdrop offsets; no off-screen geometry recovery |
| Thickness translucency | Math only | Tested cheap transmission lobe API |
| Henyey-Greenstein phase | Math only | Tested phase density; no volumetric marching pass |
| Weighted blended OIT | Math only | Order-independent accumulator/resolve API; no scene OIT pass |
| MLAB/MBOIT, depth peeling, A-buffer | Unsupported | Requires scene depth/storage passes |
| Scene mips, dual-Kawase and temporal dither/TAA | Unsupported | Requires retained frame resources/history |
| Colored transparent shadow maps | Unsupported | PMRE has no light/shadow-map pass |
| IBL, probes, SSR and ray-traced reflections | Unsupported | PMRE has no 3D environment/geometry buffers |
| Nested three-dimensional media | Unsupported | No closed-volume boundary stack |
| Random-walk SSS and volume tracking | Unsupported | Requires ray/volume traversal |
| Photon mapping, PPM/SPPM and photon caustics | Unsupported | Requires photon transport/storage |
| BDPT, VCM, MNEE/SMS and path regularization | Unsupported | Requires a path tracer |
| Hero-wavelength/spectral path sampling | Unsupported | RGB renderer only |

`COOKBOOK_CAPABILITIES` in `pmre-transparency-core` provides a machine-readable summary of this
boundary for tools that need to report support without guessing.

## Verification invariants

- Legacy injects no Customizer material; frames without an explicit app material retain the banded path.
- Material-adaptive and explicit serial renders are pixel-identical.
- Blur filters premultiplied values and does not create dark transparent fringes.
- Material writes respect rounded bounds and active clips.
- Preset selection clears advanced overrides.
- Invalid/NaN material inputs sanitize to bounded, fail-closed values.
- Fresnel normal incidence, TIR, Beer-Lambert, phase, thin film, screen refraction, dither, and
  WBOIT all have numeric unit tests.
