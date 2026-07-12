# Native Atom Renderer
tags: native, renderer, pmre, atom, no-browser

The product runtime renders through PMRE and Win32/GDI. Browser, Chrome, Electron, and Tauri are not product dependencies; the static HTML surface is only a legacy doctrine mirror.

Every interactive PMRE surface receives a renderer-owned native Design rail. The Renderer
Customizer includes output gamma from `0.50` to `2.50` with a neutral `1.00` default. Gamma is
encoded after alpha flattening and before presentation quantization; it does not rewrite raw
framebuffer color or alpha values.

The same Customizer includes opt-in transparency cookbook presets: clear/frosted glass, water,
crystal, soap film, wax, smoke, stained glass, and heat haze. Advanced optics exposes premultiplied
backdrop blur, screen refraction, RGB dispersion, and a Fresnel rim. Legacy remains the default and
injects no Customizer material; a frame without an app-authored material keeps the parallel lane
renderer unchanged. An active material creates a painter-order backdrop
barrier and uses the serial full-frame path so neighboring-row samples cannot seam.

`pmre-transparency-core` provides dependency-free optics/volume/OIT value math, while
`pmre-transparency` owns live 2D compositing. Ray-traced SSS, nested 3D media, photon/manifold
caustics, BDPT/VCM, and spectral path tracing are explicitly unsupported rather than simulated.

[[native-atom-renderer]]
[[renderer:pmre-native]]
