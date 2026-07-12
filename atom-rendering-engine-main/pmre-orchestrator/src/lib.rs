//! pmre-orchestrator — the single orchestrator. ALL policy, no mechanism.
//!
//! It owns draw order, the empty-slot check, the interaction state machine (hover / press /
//! click / toggle / scroll), and resize. It drives `pmre-kit`; it never rasterizes a pixel
//! itself. Two render paths sit on the same kit: `render`/`render_uxi`/`render_html` for
//! static frames, and the stateful `render_ui` + `handle_event` for interactive UIs.

use std::collections::HashMap;

#[cfg(feature = "gpu")]
pub mod gpu_bloom;

/// CPU-fallback shim used when the `gpu` feature is off: the GPU bloom tiers reuse the
/// CPU bloom (identical output), so `Quality::Gpu*` keeps working and the default build
/// stays dependency-free.
#[cfg(not(feature = "gpu"))]
pub mod gpu_bloom {
    use pmre_kit::framebuffer::Framebuffer;
    pub fn gpu_bloom(fb: &mut Framebuffer, threshold: f32, sigma: f32, radius: usize) {
        pmre_kit::post::bloom(fb, threshold, sigma, radius);
    }
    pub fn gpu_backend_name() -> &'static str {
        "cpu (gpu feature disabled)"
    }
}

use pmre_html as html;
use pmre_transparency::{paint_material_box, MaterialBox, MaterialPreset, TransparencyMaterial};

use pmre_kit::{
    atoms,
    bloom_sweep::{bloom_with, Arith, Dispatch, Strategy, Structure},
    framebuffer::{BandView, Framebuffer, Surface},
    geom::{Affine, Vec2},
    layout::{self, LaidBox, Painted},
    paint::{Bounds, Paint, Rgba, Shape},
    post, raster, text,
    ux::{Align, Dim, Edges, Justify, Role, Shadow, Span, Style, UxNode},
    DrawCmd,
};

// ----------------------------------------------------------------------------
// Static shape scene (used by the SDF demo)
// ----------------------------------------------------------------------------

/// A draw command plus its painter depth (`z`): larger `z` is nearer / drawn on top.
pub struct Item {
    pub z: f32,
    pub cmd: DrawCmd,
}

/// An ordered set of draw commands plus the surface to render them onto.
pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub clear: Rgba,
    pub items: Vec<Item>,
}

impl Scene {
    pub fn new(width: u32, height: u32, clear: Rgba) -> Self {
        Self {
            width,
            height,
            clear,
            items: Vec::new(),
        }
    }
    pub fn push(&mut self, z: f32, cmd: DrawCmd) {
        self.items.push(Item { z, cmd });
    }
}

/// Render the scene with the painter's algorithm (the `order` atom, back-to-front).
pub fn render(scene: &Scene) -> Framebuffer {
    let mut fb = Framebuffer::new(scene.width, scene.height, scene.clear);
    for i in atoms::order(&scene.items, |it| -it.z) {
        let item = &scene.items[i];
        if item.cmd.shape.is_degenerate() {
            continue;
        }
        raster::scan_convert(&item.cmd, &mut fb, None);
    }
    fb
}

// ----------------------------------------------------------------------------
// Shared box-tree painting
// ----------------------------------------------------------------------------

/// Paint one laid-out box (its shape commands, or its wrapped text) into `surf`, in
/// absolute device coordinates. Generic over the sink so it targets a full framebuffer or
/// a single row-band of one.
fn paint_one_box<S: Surface>(surf: &mut S, laid: &LaidBox) {
    match &laid.kind {
        Painted::Box { .. } => {
            let mut cmds: Vec<DrawCmd> = Vec::new();
            layout::cmds_for(laid, &mut cmds);
            for cmd in &cmds {
                if !cmd.shape.is_degenerate() {
                    raster::scan_convert(cmd, surf, laid.clip);
                }
            }
        }
        Painted::Text {
            content,
            size,
            color,
        } => {
            let max_w = laid.rect.max.x - laid.rect.min.x;
            let line_h = *size * 1.3;
            let (asc, desc) = text::v_metrics(*size);
            let mut y = laid.rect.min.y;
            for line in text::wrap(content, *size, max_w) {
                // center the font's ascent+descent box inside the line box
                let origin = Vec2::new(laid.rect.min.x, y + (line_h - (asc + desc)) * 0.5);
                text::draw(surf, &line, origin, *size, *color, laid.clip);
                y += line_h;
            }
        }
        Painted::Rich { spans, align } => {
            let max_w = laid.rect.max.x - laid.rect.min.x;
            let (lines, line_h) = layout::rich_lines(spans, Some(max_w));
            let mut y = laid.rect.min.y;
            for line in &lines {
                let x0 = match align {
                    pmre_kit::ux::Align::Center => laid.rect.min.x + (max_w - line.width) * 0.5,
                    pmre_kit::ux::Align::End => laid.rect.max.x - line.width,
                    _ => laid.rect.min.x,
                };
                // All pieces on a line share ONE baseline (set by the tallest piece);
                // mixed sizes must sit on it, not center independently.
                let max_size = line.pieces.iter().map(|p| p.size).fold(1.0f32, f32::max);
                let (asc_l, desc_l) = text::v_metrics(max_size);
                let baseline = y + (line_h - (asc_l + desc_l)) * 0.5 + asc_l;
                for p in &line.pieces {
                    if p.width > 0.0 {
                        if let Some(bg) = p.background {
                            fill_rect(surf, x0 + p.x, y, p.width, line_h, bg, 2.0);
                        }
                    }
                    let (asc_p, _) = text::v_metrics_styled(p.size, p.bold);
                    // zero-width overlay pieces (the caret) center on their flow position,
                    // clamped so a caret at a line start or end is never shaved by the clip
                    let px = if p.width <= 0.0 && !p.text.is_empty() {
                        let adv = text::advance_styled(&p.text, p.size, p.bold);
                        (x0 + p.x - adv * 0.5)
                            .min(laid.rect.max.x - adv)
                            .max(laid.rect.min.x)
                    } else {
                        x0 + p.x
                    };
                    let origin = Vec2::new(px, baseline - asc_p);
                    text::draw_styled(
                        surf,
                        &p.text,
                        origin,
                        p.size,
                        p.color,
                        laid.clip,
                        p.bold,
                        p.underline,
                    );
                }
                y += line_h;
            }
        }
    }
}

fn paint_boxes<S: Surface>(surf: &mut S, boxes: &[LaidBox]) {
    for laid in boxes {
        paint_one_box(surf, laid);
    }
}

/// Paint a full framebuffer in painter order, honoring transparent-material backdrop
/// barriers. Ordinary boxes still use the shared generic raster path.
fn paint_boxes_framebuffer(frame: &mut Framebuffer, boxes: &[LaidBox]) {
    for laid in boxes {
        if let Painted::Box {
            background,
            radius,
            border,
            shadow,
            transparency: Some(material),
        } = &laid.kind
        {
            if material.sanitized().strength > 0.0 {
                paint_material_box(
                    frame,
                    MaterialBox {
                        rect: laid.rect,
                        radius: *radius,
                        clip: laid.clip,
                        tint: *background,
                        border: *border,
                        shadow: *shadow,
                        material: *material,
                    },
                );
                continue;
            }
        }
        paint_one_box(frame, laid);
    }
}

fn has_transparency(boxes: &[LaidBox]) -> bool {
    boxes.iter().any(|laid| {
        matches!(
            laid.kind,
            Painted::Box {
                transparency: Some(material),
                ..
            } if material.sanitized().strength > 0.0
        )
    })
}

fn cpu_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1)
}

/// Coverage anti-aliasing margin (px): a box edge bleeds at most ~2px past its rect, so a
/// lane must rasterize boxes within this margin of its rows to avoid a seam.
const BAND_PAD: f32 = 3.0;

/// The vertical device rows a box can actually touch: its rect, expanded by shadow bleed
/// and by wrapped-text overflow, then narrowed by its clip. Lanes use this (plus
/// `BAND_PAD`) to decide which boxes reach their band — a shadow blurring 14px past a
/// card must still be painted by the lane below the card, or the frame seams.
fn paint_y_extent(b: &LaidBox) -> (f32, f32) {
    let (mut lo, mut hi) = (b.rect.min.y, b.rect.max.y);
    match &b.kind {
        Painted::Box { shadow, .. } => {
            if let Some(sh) = shadow {
                let bleed = sh.blur + sh.dy.abs() + 2.0;
                lo -= bleed;
                hi += bleed;
            }
        }
        Painted::Text { content, size, .. } => {
            let max_w = b.rect.max.x - b.rect.min.x;
            let lines = text::wrap(content, *size, max_w).len().max(1);
            hi = hi.max(b.rect.min.y + lines as f32 * *size * 1.3);
        }
        Painted::Rich { spans, .. } => {
            let max_w = b.rect.max.x - b.rect.min.x;
            let (lines, line_h) = layout::rich_lines(spans, Some(max_w));
            hi = hi.max(b.rect.min.y + lines.len() as f32 * line_h);
        }
    }
    if let Some(c) = b.clip {
        lo = lo.max(c.min.y);
        hi = hi.min(c.max.y);
    }
    (lo, hi)
}

/// Rasterize `boxes` into a fresh framebuffer, parallelized with the MM3E "lane / bus"
/// model: the frame's pixel buffer is split into contiguous row-bands (one per lane), and
/// each lane rasterizes — in absolute device coordinates — straight into its own band slice
/// via a [`BandView`]. No per-band temp buffer, no stitch, no coordinate translation; bands
/// are disjoint so lanes never alias. Output is bit-identical to the serial render and
/// independent of thread count. No locks, no atomics, no `unsafe`.
fn paint_boxes_banded(w: u32, h: u32, clear: Rgba, boxes: &[LaidBox]) -> Framebuffer {
    let n = cpu_threads();
    let mut fb = Framebuffer::new(w, h, clear);
    if has_transparency(boxes) {
        paint_boxes_framebuffer(&mut fb, boxes);
        return fb;
    }
    if n <= 1 || w == 0 || h < 2 * n as u32 {
        paint_boxes(&mut fb, boxes);
        return fb;
    }

    let band = (h as usize).div_ceil(n);
    let wsz = w as usize;
    let extents: Vec<(f32, f32)> = boxes.iter().map(paint_y_extent).collect();
    let extents = &extents;
    std::thread::scope(|s| {
        for (ti, chunk) in fb.pixels_mut().chunks_mut(band * wsz).enumerate() {
            let y0 = (ti * band) as u32;
            let band_h = (chunk.len() / wsz) as u32;
            // A lane only rasterizes boxes whose paint extent (plus AA bleed) reaches it.
            let (lo, hi) = (y0 as f32 - BAND_PAD, (y0 + band_h) as f32 + BAND_PAD);
            s.spawn(move || {
                let mut view = BandView::new(chunk, w, y0, h);
                for (b, &(elo, ehi)) in boxes.iter().zip(extents) {
                    if ehi >= lo && elo <= hi {
                        paint_one_box(&mut view, b);
                    }
                }
            });
        }
    });
    fb
}

fn viewport(w: u32, h: u32) -> Bounds {
    Bounds {
        min: Vec2::new(0.0, 0.0),
        max: Vec2::new(w as f32, h as f32),
    }
}

/// Render a UXI tree (no interaction). Reduced layout → identical raster path.
pub fn render_uxi(root: &UxNode, width: u32, height: u32, clear: Rgba) -> Framebuffer {
    let boxes = layout::solve(root, viewport(width, height), &|_| 0.0);
    paint_boxes_banded(width, height, clear, &boxes)
}

/// Single-threaded variant of [`render_uxi`] — for profiling/baselining the lane render.
/// Output is bit-identical to [`render_uxi`] (a test enforces this).
pub fn render_uxi_serial(root: &UxNode, width: u32, height: u32, clear: Rgba) -> Framebuffer {
    let mut fb = Framebuffer::new(width, height, clear);
    let boxes = layout::solve(root, viewport(width, height), &|_| 0.0);
    paint_boxes_framebuffer(&mut fb, &boxes);
    fb
}

/// Render an HTML/CSS document: the reduced front-end parses it into the same box tree.
pub fn render_html(src: &str, width: u32, height: u32, clear: Rgba) -> Framebuffer {
    render_uxi(&html::parse(src), width, height, clear)
}

/// Post-processing quality tier.
#[derive(Clone, Copy)]
pub enum Quality {
    /// Pure SDF rasterization, no post-processing.
    Fast,
    /// CPU additive Gaussian bloom at σ=3, radius=6.
    Balanced,
    /// CPU additive Gaussian bloom at σ=5, radius=12.
    Full,
    /// GPU-accelerated bloom at σ=3, radius=6 (falls back to CPU if no adapter).
    GpuBalanced,
    /// GPU-accelerated bloom at σ=5, radius=12 (falls back to CPU if no adapter).
    GpuFull,
    /// Parallel CPU bloom at σ=3, radius=6 using FairQueue across all CPU threads.
    ParallelBalanced,
    /// Parallel CPU bloom at σ=5, radius=12 using FairQueue across all CPU threads.
    ParallelFull,
    /// Cache-tiled fused CPU bloom at σ=3, radius=6 (FairQueue over tiles). Fastest CPU tier.
    TiledBalanced,
    /// Cache-tiled fused CPU bloom at σ=5, radius=12 (FairQueue over tiles). Fastest CPU tier.
    TiledFull,
}

/// Render a UXI tree then apply the post-processing pipeline for `quality`.
pub fn render_uxi_quality(
    root: &UxNode,
    width: u32,
    height: u32,
    clear: Rgba,
    quality: Quality,
) -> Framebuffer {
    let mut fb = render_uxi(root, width, height, clear);
    apply_quality(&mut fb, quality);
    fb
}

/// Render an interactive UI tree then apply the post-processing pipeline for `quality`.
pub fn render_ui_quality(
    build: &dyn Fn(&UiState) -> UxNode,
    state: &UiState,
    clear: Rgba,
    quality: Quality,
) -> Framebuffer {
    let mut fb = render_ui(build, state, clear);
    apply_quality(&mut fb, quality);
    fb
}

fn apply_quality(fb: &mut Framebuffer, quality: Quality) {
    match quality {
        Quality::Fast => {}
        Quality::Balanced => post::bloom(fb, 0.45, 3.0, 6),
        Quality::Full => post::bloom(fb, 0.45, 5.0, 12),
        Quality::GpuBalanced => gpu_bloom::gpu_bloom(fb, 0.45, 3.0, 6),
        Quality::GpuFull => gpu_bloom::gpu_bloom(fb, 0.45, 5.0, 12),
        Quality::ParallelBalanced => post::bloom_parallel(fb, 0.45, 3.0, 6),
        Quality::ParallelFull => post::bloom_parallel(fb, 0.45, 5.0, 12),
        Quality::TiledBalanced => bloom_with(fb, 0.45, 3.0, 6, tiled_strategy()),
        Quality::TiledFull => bloom_with(fb, 0.45, 5.0, 12, tiled_strategy()),
    }
}

/// The MM3E "lane / bus" model applied to bloom: each thread owns one contiguous
/// strip of fused cache tiles, writing its own region — no locks, no atomics, no
/// `unsafe` aliasing, deterministic in thread count. Within ~6–12% of the fastest
/// (FairQueue) dispatch in `examples/sweep`, and the only fast path that is fully
/// lock-free and order-independent. SIMD inner loop edges out scalar for this dispatch.
fn tiled_strategy() -> Strategy {
    Strategy::new(Dispatch::Band, Structure::TiledFused, Arith::Simd)
}

// ----------------------------------------------------------------------------
// Interactive UI: state, events, stateful render
// ----------------------------------------------------------------------------

pub const DESIGN_TAB_ID: u32 = 3_900_000_000;
pub const DESIGN_HUE_SLIDER: u32 = 3_900_000_001;
pub const DESIGN_SAT_SLIDER: u32 = 3_900_000_002;
pub const DESIGN_LIGHT_SLIDER: u32 = 3_900_000_003;
pub const DESIGN_TEXT_SLIDER: u32 = 3_900_000_004;
pub const DESIGN_RADIUS_SLIDER: u32 = 3_900_000_005;
pub const DESIGN_GLASS_SLIDER: u32 = 3_900_000_006;
pub const DESIGN_ANIMATION_SLIDER: u32 = 3_900_000_007;
pub const DESIGN_TYPOGRAPHY_SELECT: u32 = 3_900_000_008;
pub const DESIGN_SHAPE_SELECT: u32 = 3_900_000_009;
pub const DESIGN_PANEL_SCROLL: u32 = 3_900_000_010;
pub const DESIGN_MIC_ON_BUTTON: u32 = 3_900_000_011;
pub const DESIGN_MIC_OFF_BUTTON: u32 = 3_900_000_012;
pub const DESIGN_MUTE_TOGGLE: u32 = 3_900_000_013;
pub const DESIGN_RECORD_TOGGLE: u32 = 3_900_000_014;
pub const DESIGN_GAMMA_SLIDER: u32 = 3_900_000_015;
pub const DESIGN_MATERIAL_PRESET_SELECT: u32 = 3_900_000_016;
pub const DESIGN_MATERIAL_ADVANCED_TOGGLE: u32 = 3_900_000_017;
pub const DESIGN_MATERIAL_BLUR_SLIDER: u32 = 3_900_000_018;
pub const DESIGN_MATERIAL_REFRACTION_SLIDER: u32 = 3_900_000_019;
pub const DESIGN_MATERIAL_DISPERSION_SLIDER: u32 = 3_900_000_020;
pub const DESIGN_MATERIAL_RIM_SLIDER: u32 = 3_900_000_021;
pub const DESIGN_MATERIAL_PREVIOUS_BUTTON: u32 = 3_900_000_022;
pub const DESIGN_MATERIAL_RESET_BUTTON: u32 = 3_900_000_023;

const DESIGN_GAMMA_MIN: f32 = 0.50;
const DESIGN_GAMMA_MAX: f32 = 2.50;
const DESIGN_GAMMA_NEUTRAL_SLIDER: f32 = 0.25;

const TYPOGRAPHY_OPTIONS: [&str; 8] = [
    "Clean",
    "Display",
    "Code",
    "Editorial",
    "Dense",
    "Rounded",
    "Readable",
    "Poster",
];
const SHAPE_OPTIONS: [&str; 6] = ["Soft", "Square", "Round", "Pill", "Circle", "Sharp"];
const MATERIAL_OPTIONS: [&str; 10] = [
    "Legacy",
    "Clear glass",
    "Frosted glass",
    "Water",
    "Crystal",
    "Soap film",
    "Wax",
    "Smoke",
    "Stained glass",
    "Heat haze",
];

fn material_for_index(index: usize) -> Option<TransparencyMaterial> {
    index
        .checked_sub(1)
        .and_then(|preset| MaterialPreset::ALL.get(preset).copied())
        .map(MaterialPreset::material)
}

#[derive(Clone, Copy)]
struct DesignTokens {
    hue: f32,
    sat: f32,
    light: f32,
    text: f32,
    radius: f32,
    glass: f32,
    animation: f32,
    gamma_slider: f32,
    material_preset: usize,
    material_blur: f32,
    material_refraction: f32,
    material_dispersion: f32,
    material_rim: f32,
    material: Option<TransparencyMaterial>,
    phase: f32,
    typography: usize,
    shape: usize,
    accent: Rgba,
    accent_2: Rgba,
    ink: Rgba,
    panel: Rgba,
}

impl DesignTokens {
    fn from_state(state: &UiState) -> Self {
        let hue = state.slider_value(DESIGN_HUE_SLIDER, 0.52);
        let sat = state.slider_value(DESIGN_SAT_SLIDER, 0.62);
        let light = state.slider_value(DESIGN_LIGHT_SLIDER, 0.48);
        let text = state.slider_value(DESIGN_TEXT_SLIDER, 0.50);
        let radius = state.slider_value(DESIGN_RADIUS_SLIDER, 0.38);
        let glass = state.slider_value(DESIGN_GLASS_SLIDER, 0.20);
        let animation = state.slider_value(DESIGN_ANIMATION_SLIDER, 0.30);
        let gamma_slider = state.slider_value(DESIGN_GAMMA_SLIDER, DESIGN_GAMMA_NEUTRAL_SLIDER);
        let material_preset =
            state.select_index(DESIGN_MATERIAL_PRESET_SELECT) % MATERIAL_OPTIONS.len();
        let preset_material = material_for_index(material_preset);
        let defaults = preset_material.unwrap_or(TransparencyMaterial {
            blur_radius_px: 0.0,
            refraction_px: 0.0,
            dispersion_px: 0.0,
            rim_width_px: 0.0,
            ..TransparencyMaterial::default()
        });
        let material_blur = state.slider_value(
            DESIGN_MATERIAL_BLUR_SLIDER,
            (defaults.blur_radius_px / 24.0).clamp(0.0, 1.0),
        );
        let material_refraction = state.slider_value(
            DESIGN_MATERIAL_REFRACTION_SLIDER,
            (defaults.refraction_px / 20.0).clamp(0.0, 1.0),
        );
        let material_dispersion = state.slider_value(
            DESIGN_MATERIAL_DISPERSION_SLIDER,
            (defaults.dispersion_px / 4.0).clamp(0.0, 1.0),
        );
        let material_rim = state.slider_value(
            DESIGN_MATERIAL_RIM_SLIDER,
            (defaults.rim_width_px / 4.0).clamp(0.0, 1.0),
        );
        let typography = state.select_index(DESIGN_TYPOGRAPHY_SELECT) % TYPOGRAPHY_OPTIONS.len();
        let shape = state.select_index(DESIGN_SHAPE_SELECT) % SHAPE_OPTIONS.len();
        let accent = hsl_to_rgb(hue, 0.35 + sat * 0.55, 0.26 + light * 0.50);
        let accent_2 = hsl_to_rgb((hue + 0.34) % 1.0, 0.42 + sat * 0.50, 0.30 + light * 0.45);
        let ink = if light > 0.58 {
            Rgba::rgb8(15, 22, 22)
        } else {
            Rgba::rgb8(246, 250, 248)
        };
        let panel = if light > 0.58 {
            Rgba::new(0.98, 0.99, 0.97, 1.0)
        } else {
            Rgba::new(0.08, 0.10, 0.11, 1.0)
        };
        let material = preset_material.map(|mut material| {
            material.blur_radius_px = material_blur * 24.0;
            material.roughness = material.roughness.max(material_blur);
            material.refraction_px = material_refraction * 20.0;
            material.dispersion_px = material_dispersion * 4.0;
            material.rim_width_px = material_rim * 4.0;
            material.strength *= glass;
            material.phase = state.animation_time * animation * 2.25;
            material.sanitized()
        });
        Self {
            hue,
            sat,
            light,
            text,
            radius,
            glass,
            animation,
            gamma_slider,
            material_preset,
            material_blur,
            material_refraction,
            material_dispersion,
            material_rim,
            material,
            phase: state.animation_time,
            typography,
            shape,
            accent,
            accent_2,
            ink,
            panel,
        }
    }

    fn output_gamma(self) -> f32 {
        DESIGN_GAMMA_MIN + self.gamma_slider.clamp(0.0, 1.0) * (DESIGN_GAMMA_MAX - DESIGN_GAMMA_MIN)
    }

    fn text_factor(self, size: f32) -> f32 {
        let slider = 0.82 + self.text * 0.44;
        let family = match TYPOGRAPHY_OPTIONS[self.typography] {
            "Display" => {
                if size >= 18.0 {
                    1.12
                } else {
                    0.98
                }
            }
            "Code" => 0.94,
            "Editorial" => 1.06,
            "Dense" => 0.88,
            "Rounded" => 1.02,
            "Readable" => 1.10,
            "Poster" => {
                if size >= 20.0 {
                    1.20
                } else {
                    0.96
                }
            }
            _ => 1.0,
        };
        (slider * family).clamp(0.75, 1.45)
    }

    fn radius_for(self, base: f32, role: Role, height_hint: f32) -> f32 {
        let range = 2.0 + self.radius * 26.0;
        let shaped = match SHAPE_OPTIONS[self.shape] {
            "Square" => 0.0,
            "Round" => range * 1.2,
            "Pill" => 999.0,
            "Circle" => height_hint.max(range),
            "Sharp" => 2.0,
            _ => base.max(range * 0.55),
        };
        if matches!(
            role,
            Role::Button | Role::Toggle | Role::Input | Role::Slider | Role::Select
        ) {
            shaped.max(2.0)
        } else {
            base.max(range * 0.35)
        }
    }

    fn surface(self, src: Rgba, role: Role) -> Rgba {
        let pulse = if self.animation > 0.01 {
            ((self.phase * std::f32::consts::TAU).sin() * 0.5 + 0.5) * self.animation
        } else {
            0.0
        };
        let accent_mix = if matches!(role, Role::Button | Role::Toggle | Role::Select) {
            0.26 + pulse * 0.12
        } else if src.r + src.g + src.b > 2.15 {
            0.04 + self.glass * 0.08
        } else {
            0.05 + self.glass * 0.05
        };
        let mut out = mix_rgba(src, self.accent, accent_mix.clamp(0.0, 0.45));
        if self.glass > 0.0 {
            out.a = (1.0 - self.glass * 0.36).clamp(0.60, 1.0);
        }
        out
    }

    fn border(self, src: Rgba) -> Rgba {
        mix_rgba(src, self.accent_2, 0.22 + self.glass * 0.16).with_alpha(0.76)
    }
}

/// All UI interaction state. The app's `build(&UiState) -> UxNode` reads this to style
/// widgets (hover/press/toggle) so the tree always reflects current state.
///
/// `width`/`height` are **physical** pixels; `scale` is the DPI factor (1.0 = 96 dpi).
/// Layout solves in logical units (`width / scale`) and painting multiplies back up,
/// so the same tree renders crisply on any monitor. Events are fed in logical units.
pub struct UiState {
    pub width: u32,
    pub height: u32,
    /// Device-pixel ratio: physical px per logical px. Always ≥ a small epsilon.
    pub scale: f32,
    pub hover: Option<u32>,
    pub pressed: Option<u32>,
    pub clicked: Option<u32>,
    pub toggles: HashMap<u32, bool>,
    pub scrolls: HashMap<u32, f32>,
    /// Normalized slider values per slider id.
    pub sliders: HashMap<u32, f32>,
    /// Selected option index per select id.
    pub selects: HashMap<u32, usize>,
    /// Scroll region whose scrollbar thumb is currently being dragged.
    pub drag: Option<u32>,
    /// Slider whose track is currently being dragged.
    pub slider_drag: Option<u32>,
    /// Pointer offset from the thumb's top edge at grab time, so the thumb doesn't
    /// jump to center itself under the cursor.
    pub drag_grab: f32,
    /// The focused text input, if any.
    pub focused: Option<u32>,
    /// Text contents per input field id.
    pub inputs: HashMap<u32, String>,
    /// Caret position per input field id, measured in Unicode scalar positions.
    pub input_carets: HashMap<u32, usize>,
    /// Selection anchor/focus per input field id, measured in Unicode scalar positions.
    pub input_selections: HashMap<u32, (usize, usize)>,
    /// Input selection currently being extended by pointer drag.
    pub input_drag_anchor: Option<(u32, usize)>,
    /// Text copied or cut by the UI since it was last bridged to the host clipboard.
    pub clipboard_out: Option<String>,
    /// Input field that received Enter since it was last polled.
    pub submit: Option<u32>,
    /// Monotonic animation clock in seconds for renderer-owned visual effects.
    pub animation_time: f32,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            scale: 1.0,
            hover: None,
            pressed: None,
            clicked: None,
            toggles: HashMap::new(),
            scrolls: HashMap::new(),
            sliders: HashMap::new(),
            selects: HashMap::new(),
            drag: None,
            slider_drag: None,
            drag_grab: 0.0,
            focused: None,
            inputs: HashMap::new(),
            input_carets: HashMap::new(),
            input_selections: HashMap::new(),
            input_drag_anchor: None,
            clipboard_out: None,
            submit: None,
            animation_time: 0.0,
        }
    }
}

impl UiState {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..Self::default()
        }
    }
    pub fn is_hover(&self, id: u32) -> bool {
        self.hover == Some(id)
    }
    pub fn is_pressed(&self, id: u32) -> bool {
        self.pressed == Some(id)
    }
    pub fn toggle_on(&self, id: u32) -> bool {
        self.toggles.get(&id).copied().unwrap_or(false)
    }
    pub fn scroll_of(&self, id: u32) -> f32 {
        self.scrolls.get(&id).copied().unwrap_or(0.0)
    }
    pub fn slider_value(&self, id: u32, default: f32) -> f32 {
        self.sliders
            .get(&id)
            .copied()
            .unwrap_or(default)
            .clamp(0.0, 1.0)
    }
    pub fn set_slider(&mut self, id: u32, value: f32) {
        self.sliders.insert(id, value.clamp(0.0, 1.0));
    }
    pub fn select_index(&self, id: u32) -> usize {
        self.selects.get(&id).copied().unwrap_or(0)
    }
    pub fn set_select_index(&mut self, id: u32, index: usize) {
        self.selects.insert(id, index);
    }
    /// True exactly once for the widget clicked on the most recent PointerUp.
    pub fn take_click(&mut self) -> Option<u32> {
        self.clicked.take()
    }
    pub fn is_focused(&self, id: u32) -> bool {
        self.focused == Some(id)
    }
    pub fn input_text(&self, id: u32) -> &str {
        self.inputs.get(&id).map(String::as_str).unwrap_or("")
    }
    pub fn input_caret(&self, id: u32) -> usize {
        self.input_carets
            .get(&id)
            .copied()
            .unwrap_or_else(|| char_len(self.input_text(id)))
            .min(char_len(self.input_text(id)))
    }
    pub fn input_selection(&self, id: u32) -> Option<(usize, usize)> {
        let len = char_len(self.input_text(id));
        let (a, b) = self.input_selections.get(&id).copied()?;
        let start = a.min(b).min(len);
        let end = a.max(b).min(len);
        (start < end).then_some((start, end))
    }
    pub fn selected_text(&self) -> Option<String> {
        let id = self.focused?;
        let (start, end) = self.input_selection(id)?;
        Some(slice_chars(self.input_text(id), start, end).to_string())
    }
    pub fn take_clipboard_out(&mut self) -> Option<String> {
        self.clipboard_out.take()
    }
    pub fn clear_input(&mut self, id: u32) {
        self.inputs.remove(&id);
        self.input_carets.remove(&id);
        self.input_selections.remove(&id);
    }
    /// The input field that received Enter since the last poll.
    pub fn take_submit(&mut self) -> Option<u32> {
        self.submit.take()
    }
}

/// Pointer / window events fed to `handle_event`.
pub enum UiEvent {
    Resize(u32, u32),
    PointerMove(f32, f32),
    PointerDown(f32, f32),
    PointerUp(f32, f32),
    /// Vertical wheel: cursor position and a positive-down delta in pixels.
    Wheel(f32, f32, f32),
    /// A typed character routed to the focused input.
    Char(char),
    /// Delete the last character of the focused input.
    Backspace,
    /// Delete the selected range or the character after the caret.
    Delete,
    /// Move the focused input caret one character left.
    MoveLeft {
        shift: bool,
    },
    /// Move the focused input caret one character right.
    MoveRight {
        shift: bool,
    },
    /// Select all text in the focused input.
    SelectAll,
    /// Copy the selected text from the focused input.
    Copy,
    /// Cut the selected text from the focused input.
    Cut,
    /// Paste host clipboard text into the focused input.
    Paste(String),
    /// Enter pressed; marks the focused input as submitted.
    Enter,
    /// Advance renderer-owned animation time by this many seconds.
    Tick(f32),
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .map(|(idx, _)| idx)
        .nth(char_idx)
        .unwrap_or(text.len())
}

fn slice_chars(text: &str, start: usize, end: usize) -> &str {
    let a = char_to_byte(text, start);
    let b = char_to_byte(text, end.max(start));
    &text[a..b]
}

fn sanitize_input_text(text: &str) -> String {
    text.chars()
        .filter_map(|ch| match ch {
            '\0' => None,
            '\r' => Some('\n'),
            '\n' | '\t' => Some(ch),
            _ if ch.is_control() => None,
            _ => Some(ch),
        })
        .collect()
}

fn clamp_input_caret(state: &UiState, id: u32, caret: usize) -> usize {
    caret.min(char_len(state.input_text(id)))
}

fn set_input_caret(state: &mut UiState, id: u32, caret: usize, keep_selection: bool) {
    let caret = clamp_input_caret(state, id, caret);
    state.input_carets.insert(id, caret);
    if !keep_selection {
        state.input_selections.remove(&id);
    }
}

fn set_input_selection(state: &mut UiState, id: u32, anchor: usize, focus: usize) {
    let anchor = clamp_input_caret(state, id, anchor);
    let focus = clamp_input_caret(state, id, focus);
    state.input_carets.insert(id, focus);
    if anchor == focus {
        state.input_selections.remove(&id);
    } else {
        state.input_selections.insert(id, (anchor, focus));
    }
}

fn delete_input_selection(state: &mut UiState, id: u32) -> bool {
    let Some((start, end)) = state.input_selection(id) else {
        return false;
    };
    let text = state.inputs.entry(id).or_default();
    let a = char_to_byte(text, start);
    let b = char_to_byte(text, end);
    text.replace_range(a..b, "");
    state.input_carets.insert(id, start);
    state.input_selections.remove(&id);
    true
}

fn insert_input_text(state: &mut UiState, id: u32, incoming: &str) {
    let incoming = sanitize_input_text(incoming);
    if incoming.is_empty() {
        return;
    }
    delete_input_selection(state, id);
    let caret = clamp_input_caret(state, id, state.input_caret(id));
    let text = state.inputs.entry(id).or_default();
    let at = char_to_byte(text, caret);
    text.insert_str(at, &incoming);
    state.input_carets.insert(id, caret + char_len(&incoming));
}

fn backspace_input(state: &mut UiState, id: u32) {
    if delete_input_selection(state, id) {
        return;
    }
    let caret = state.input_caret(id);
    if caret == 0 {
        return;
    }
    let text = state.inputs.entry(id).or_default();
    let a = char_to_byte(text, caret - 1);
    let b = char_to_byte(text, caret);
    text.replace_range(a..b, "");
    state.input_carets.insert(id, caret - 1);
}

fn delete_forward_input(state: &mut UiState, id: u32) {
    if delete_input_selection(state, id) {
        return;
    }
    let caret = state.input_caret(id);
    let len = char_len(state.input_text(id));
    if caret >= len {
        return;
    }
    let text = state.inputs.entry(id).or_default();
    let a = char_to_byte(text, caret);
    let b = char_to_byte(text, caret + 1);
    text.replace_range(a..b, "");
    state.input_carets.insert(id, caret);
}

fn move_input_caret(state: &mut UiState, id: u32, right: bool, shift: bool) {
    if !shift {
        if let Some((start, end)) = state.input_selection(id) {
            set_input_caret(state, id, if right { end } else { start }, false);
            return;
        }
    }
    let current = state.input_caret(id);
    let len = char_len(state.input_text(id));
    let next = if right {
        (current + 1).min(len)
    } else {
        current.saturating_sub(1)
    };
    if shift {
        let anchor = state
            .input_selections
            .get(&id)
            .map(|(anchor, _)| *anchor)
            .unwrap_or(current);
        set_input_selection(state, id, anchor, next);
    } else {
        set_input_caret(state, id, next, false);
    }
}

fn select_all_input(state: &mut UiState, id: u32) {
    set_input_selection(state, id, 0, char_len(state.input_text(id)));
}

fn selected_input_text(state: &UiState, id: u32) -> Option<String> {
    let (start, end) = state.input_selection(id)?;
    Some(slice_chars(state.input_text(id), start, end).to_string())
}

/// Caret index for a pointer position over the input `id`, line-aware for wrapped text.
/// The input's laid text child supplies the true text origin, size, and wrap width.
fn input_caret_from_pointer(state: &UiState, boxes: &[LaidBox], id: u32, x: f32, y: f32) -> usize {
    let Some(bi) = boxes.iter().position(|b| b.id == Some(id)) else {
        return state.input_caret(id);
    };
    let owner = boxes[bi].rect;
    let text = state.input_text(id);
    let len = char_len(text);
    if len == 0 {
        return 0;
    }
    // The input's text child is the first Text/Rich box after it in pre-order whose
    // horizontal span sits inside the input and whose rect still intersects it — an
    // inner scroll region shifts the child's top ABOVE the input when scrolled, so a
    // strict rect.min containment test would miss it and misplace the caret by lines.
    let child = boxes[bi + 1..].iter().find(|b| {
        matches!(b.kind, Painted::Text { .. } | Painted::Rich { .. })
            && b.rect.min.x >= owner.min.x - 0.5
            && b.rect.max.x <= owner.max.x + 0.5
            && b.rect.min.y < owner.max.y
            && b.rect.max.y > owner.min.y
    });
    let (origin, size, width) = match child {
        Some(c) => {
            let size = match &c.kind {
                Painted::Text { size, .. } => *size,
                Painted::Rich { spans, .. } => spans.first().map(|s| s.size).unwrap_or(14.0),
                Painted::Box { .. } => 14.0,
            };
            (c.rect.min, size, c.rect.max.x - c.rect.min.x)
        }
        None => (
            Vec2::new(owner.min.x + 10.0, owner.min.y),
            14.0,
            owner.max.x - owner.min.x - 20.0,
        ),
    };
    let ranges = text::wrap_ranges(text, size, width.max(1.0));
    let line = (((y - origin.y) / (size * 1.3)).floor().max(0.0) as usize)
        .min(ranges.len().saturating_sub(1));
    let (start, end) = ranges[line];
    let target = (x - origin.x).max(0.0);
    let mut best = start;
    let mut best_dist = f32::INFINITY;
    for idx in start..=end {
        let prefix = slice_chars(text, start, idx);
        let dist = (text::advance(prefix, size) - target).abs();
        if dist < best_dist {
            best = idx;
            best_dist = dist;
        }
    }
    best
}

fn with_design_customizer(root: UxNode, state: &UiState) -> UxNode {
    let tokens = DesignTokens::from_state(state);
    let open = state.toggle_on(DESIGN_TAB_ID);
    let mut children = vec![
        UxNode::boxed(
            Style::col().w(Dim::Flex(1.0)).h(Dim::Flex(1.0)),
            vec![apply_design(root, &tokens)],
        ),
        design_rail(open, tokens),
    ];
    if open {
        children.push(design_panel(state, tokens));
    }
    UxNode::boxed(
        Style::row().w(Dim::Flex(1.0)).h(Dim::Flex(1.0)).gap(8.0),
        children,
    )
}

fn apply_design(node: UxNode, tokens: &DesignTokens) -> UxNode {
    match node {
        UxNode::Box {
            mut style,
            children,
        } => {
            style = themed_style(style, *tokens);
            UxNode::boxed(
                style,
                children
                    .into_iter()
                    .map(|child| apply_design(child, tokens))
                    .collect(),
            )
        }
        UxNode::Text {
            content,
            size,
            color,
        } => UxNode::Text {
            content,
            size: (size * tokens.text_factor(size)).clamp(7.0, 72.0),
            color: themed_text_color(color, *tokens),
        },
        UxNode::Rich { spans, align } => UxNode::Rich {
            spans: spans
                .into_iter()
                .map(|span| themed_span(span, *tokens))
                .collect(),
            align,
        },
    }
}

fn themed_style(mut style: Style, tokens: DesignTokens) -> Style {
    let material_surface = style.shadow.is_some() || style.role == Role::Scroll;
    let height_hint = match style.height {
        Dim::Px(v) => v,
        _ => 38.0,
    };
    if let Some(bg) = style.background {
        style.background = Some(tokens.surface(bg, style.role));
        style.radius = tokens.radius_for(style.radius, style.role, height_hint);
        style.border = match style.border {
            Some((w, c)) => Some((w.max(1.0), tokens.border(c))),
            None if tokens.glass > 0.05 => Some((1.0, tokens.border(tokens.accent))),
            None => None,
        };
        if style.shadow.is_some()
            || tokens.glass > 0.15
            || matches!(style.role, Role::Button | Role::Toggle | Role::Select)
        {
            style.shadow = Some(Shadow {
                dx: 0.0,
                dy: 4.0 + tokens.glass * 8.0,
                blur: 10.0 + tokens.glass * 18.0,
                color: Rgba::new(0.0, 0.0, 0.0, 0.10 + tokens.glass * 0.20),
            });
        }
    } else if matches!(
        style.role,
        Role::Button | Role::Toggle | Role::Input | Role::Slider | Role::Select
    ) {
        style.radius = tokens.radius_for(style.radius, style.role, height_hint);
    }
    if material_surface
        && !matches!(
            style.role,
            Role::Button | Role::Toggle | Role::Input | Role::Slider | Role::Select
        )
    {
        if let Some(material) = tokens.material {
            style.transparency = Some(material);
        }
    }
    style
}

fn themed_text_color(color: Rgba, tokens: DesignTokens) -> Rgba {
    let readable = if tokens.light > 0.58 {
        Rgba::rgb8(20, 28, 28)
    } else {
        Rgba::rgb8(238, 244, 244)
    };
    mix_rgba(color, readable, 0.18)
}

fn themed_span(mut span: Span, tokens: DesignTokens) -> Span {
    span.size = (span.size * tokens.text_factor(span.size)).clamp(7.0, 72.0);
    span.color = themed_text_color(span.color, tokens);
    if matches!(TYPOGRAPHY_OPTIONS[tokens.typography], "Display" | "Poster") {
        span.bold = true;
    }
    span
}

fn design_rail(open: bool, tokens: DesignTokens) -> UxNode {
    UxNode::boxed(
        Style::col()
            .w(Dim::Px(78.0))
            .h(Dim::Flex(1.0))
            .gap(8.0)
            .pad(Edges::xy(8.0, 10.0))
            .radius(14.0)
            .bg(tokens.panel.with_alpha(0.94))
            .border(1.0, tokens.border(tokens.accent)),
        vec![
            UxNode::boxed(
                Style::row()
                    .toggle(DESIGN_TAB_ID)
                    .h(Dim::Px(42.0))
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .radius(if open { 16.0 } else { 10.0 })
                    .bg(if open { tokens.accent } else { tokens.panel })
                    .border(1.0, tokens.border(tokens.accent)),
                vec![UxNode::text(
                    "Design",
                    12.0,
                    if open {
                        Rgba::rgb8(255, 255, 255)
                    } else {
                        tokens.ink
                    },
                )],
            ),
            palette_strip(tokens),
        ],
    )
}

fn palette_strip(tokens: DesignTokens) -> UxNode {
    UxNode::boxed(
        Style::col().gap(5.0),
        (0..6)
            .map(|i| {
                let hue = (tokens.hue + i as f32 / 6.0) % 1.0;
                UxNode::boxed(
                    Style::row()
                        .h(Dim::Px(18.0))
                        .radius(9.0)
                        .bg(hsl_to_rgb(
                            hue,
                            0.45 + tokens.sat * 0.45,
                            0.35 + tokens.light * 0.38,
                        ))
                        .border(1.0, tokens.border(tokens.accent)),
                    vec![],
                )
            })
            .collect(),
    )
}

fn design_panel(state: &UiState, tokens: DesignTokens) -> UxNode {
    let mut controls = vec![
        UxNode::text("Renderer Customizer", 18.0, tokens.ink),
        UxNode::text(
            "Dependency-free style controls for generated PMRE apps.",
            11.0,
            tokens.ink,
        ),
        UxNode::text("Transparency cookbook", 12.0, tokens.ink),
        material_select_control(tokens),
        UxNode::text(
            "Targets authored shadow/scroll surfaces; ray-only recipes remain unavailable.",
            10.0,
            tokens.ink,
        ),
    ];
    if tokens.material_preset == 0 {
        controls.push(UxNode::text(
            "Choose a material to enable Advanced optics.",
            10.0,
            tokens.ink,
        ));
    } else {
        controls.push(material_advanced_toggle(
            state.toggle_on(DESIGN_MATERIAL_ADVANCED_TOGGLE),
            tokens,
        ));
    }
    if tokens.material_preset != 0 && state.toggle_on(DESIGN_MATERIAL_ADVANCED_TOGGLE) {
        controls.extend([
            material_slider_control(
                "Frost / blur",
                DESIGN_MATERIAL_BLUR_SLIDER,
                tokens.material_blur,
                24.0,
                tokens,
            ),
            material_slider_control(
                "Refraction",
                DESIGN_MATERIAL_REFRACTION_SLIDER,
                tokens.material_refraction,
                20.0,
                tokens,
            ),
            material_slider_control(
                "RGB dispersion",
                DESIGN_MATERIAL_DISPERSION_SLIDER,
                tokens.material_dispersion,
                4.0,
                tokens,
            ),
            material_slider_control(
                "Fresnel rim",
                DESIGN_MATERIAL_RIM_SLIDER,
                tokens.material_rim,
                4.0,
                tokens,
            ),
            material_reset_button(tokens),
        ]);
    }
    controls.extend([
        slider_control("Hue", DESIGN_HUE_SLIDER, tokens.hue, tokens),
        slider_control("Saturation", DESIGN_SAT_SLIDER, tokens.sat, tokens),
        slider_control("Light", DESIGN_LIGHT_SLIDER, tokens.light, tokens),
        slider_control("Text", DESIGN_TEXT_SLIDER, tokens.text, tokens),
        slider_control("Radius", DESIGN_RADIUS_SLIDER, tokens.radius, tokens),
        slider_control("Glass", DESIGN_GLASS_SLIDER, tokens.glass, tokens),
        gamma_control(tokens),
        slider_control(
            "Animation",
            DESIGN_ANIMATION_SLIDER,
            tokens.animation,
            tokens,
        ),
        select_control(
            "Typography",
            DESIGN_TYPOGRAPHY_SELECT,
            state.select_index(DESIGN_TYPOGRAPHY_SELECT),
            &TYPOGRAPHY_OPTIONS,
            tokens,
        ),
        select_control(
            "Control shape",
            DESIGN_SHAPE_SELECT,
            state.select_index(DESIGN_SHAPE_SELECT),
            &SHAPE_OPTIONS,
            tokens,
        ),
        UxNode::text("Palette", 12.0, tokens.ink),
        palette_grid(tokens),
        UxNode::text("Buttons and toggles", 12.0, tokens.ink),
        preview_controls(state, tokens),
    ]);
    UxNode::boxed(
        Style::col()
            .scroll(DESIGN_PANEL_SCROLL)
            .w(Dim::Px(326.0))
            .h(Dim::Flex(1.0))
            .gap(12.0)
            .pad(Edges::all(14.0))
            .radius(14.0)
            .bg(tokens.panel.with_alpha(0.94))
            .border(1.0, tokens.border(tokens.accent))
            .shadow(0.0, 8.0, 22.0, Rgba::new(0.0, 0.0, 0.0, 0.18)),
        controls,
    )
}

fn material_select_control(tokens: DesignTokens) -> UxNode {
    UxNode::boxed(
        Style::row().h(Dim::Px(36.0)).gap(6.0),
        vec![
            UxNode::boxed(
                Style::row()
                    .button(DESIGN_MATERIAL_PREVIOUS_BUTTON)
                    .w(Dim::Px(36.0))
                    .h(Dim::Flex(1.0))
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .radius(10.0)
                    .bg(mix_rgba(tokens.panel, tokens.accent, 0.10))
                    .border(1.0, tokens.border(tokens.accent)),
                vec![UxNode::text("<", 14.0, tokens.accent)],
            ),
            select_control(
                "Material",
                DESIGN_MATERIAL_PRESET_SELECT,
                tokens.material_preset,
                &MATERIAL_OPTIONS,
                tokens,
            ),
        ],
    )
}

fn material_reset_button(tokens: DesignTokens) -> UxNode {
    UxNode::boxed(
        Style::row()
            .button(DESIGN_MATERIAL_RESET_BUTTON)
            .h(Dim::Px(34.0))
            .align(Align::Center)
            .justify(Justify::Center)
            .radius(10.0)
            .bg(mix_rgba(tokens.panel, tokens.accent, 0.10))
            .border(1.0, tokens.border(tokens.accent)),
        vec![UxNode::text("Reset material overrides", 11.0, tokens.ink)],
    )
}

fn material_advanced_toggle(open: bool, tokens: DesignTokens) -> UxNode {
    UxNode::boxed(
        Style::row()
            .toggle(DESIGN_MATERIAL_ADVANCED_TOGGLE)
            .h(Dim::Px(36.0))
            .align(Align::Center)
            .justify(Justify::SpaceBetween)
            .pad(Edges::xy(10.0, 0.0))
            .radius(10.0)
            .bg(mix_rgba(
                tokens.panel,
                tokens.accent,
                if open { 0.22 } else { 0.10 },
            ))
            .border(1.0, tokens.border(tokens.accent)),
        vec![
            UxNode::text("Advanced optics", 12.0, tokens.ink),
            UxNode::text(if open { "-" } else { "+" }, 14.0, tokens.accent),
        ],
    )
}

fn slider_control(label: &str, id: u32, value: f32, tokens: DesignTokens) -> UxNode {
    let pct = value.clamp(0.0, 1.0);
    slider_control_with_value(
        label,
        id,
        pct,
        format!("{}%", (pct * 100.0).round() as u32),
        tokens,
    )
}

fn material_slider_control(
    label: &str,
    id: u32,
    value: f32,
    maximum_px: f32,
    tokens: DesignTokens,
) -> UxNode {
    slider_control_with_value(
        label,
        id,
        value,
        format!("{:.1}px", value.clamp(0.0, 1.0) * maximum_px),
        tokens,
    )
}

fn gamma_control(tokens: DesignTokens) -> UxNode {
    slider_control_with_value(
        "Gamma",
        DESIGN_GAMMA_SLIDER,
        tokens.gamma_slider,
        format!("{:.2}", tokens.output_gamma()),
        tokens,
    )
}

fn slider_control_with_value(
    label: &str,
    id: u32,
    value: f32,
    display_value: String,
    tokens: DesignTokens,
) -> UxNode {
    let pct = value.clamp(0.0, 1.0);
    UxNode::boxed(
        Style::col().gap(5.0),
        vec![
            UxNode::boxed(
                Style::row().justify(Justify::SpaceBetween),
                vec![
                    UxNode::text(label, 11.0, tokens.ink),
                    UxNode::text(display_value, 11.0, tokens.ink),
                ],
            ),
            UxNode::boxed(
                Style::row()
                    .slider(id)
                    .h(Dim::Px(20.0))
                    .pad(Edges::all(3.0))
                    .radius(999.0)
                    .bg(mix_rgba(tokens.panel, tokens.accent, 0.10))
                    .border(1.0, tokens.border(tokens.accent)),
                vec![UxNode::boxed(
                    Style::row()
                        .w(Dim::Pct((pct * 100.0).max(1.0)))
                        .h(Dim::Flex(1.0))
                        .radius(999.0)
                        .bg(mix_rgba(tokens.accent, tokens.accent_2, pct * 0.45)),
                    vec![],
                )],
            ),
        ],
    )
}

fn select_control(
    label: &str,
    id: u32,
    index: usize,
    options: &[&str],
    tokens: DesignTokens,
) -> UxNode {
    let value = options[index % options.len()];
    UxNode::boxed(
        Style::row()
            .select(id)
            .w(Dim::Flex(1.0))
            .h(Dim::Px(36.0))
            .align(Align::Center)
            .justify(Justify::SpaceBetween)
            .pad(Edges::xy(10.0, 0.0))
            .radius(10.0)
            .bg(mix_rgba(tokens.panel, tokens.accent, 0.10))
            .border(1.0, tokens.border(tokens.accent)),
        vec![
            UxNode::text(format!("{label}: {value}"), 12.0, tokens.ink),
            UxNode::text("v", 12.0, tokens.accent),
        ],
    )
}

fn palette_grid(tokens: DesignTokens) -> UxNode {
    UxNode::boxed(
        Style::col().gap(6.0),
        (0..4)
            .map(|row| {
                UxNode::boxed(
                    Style::row().gap(6.0).h(Dim::Px(28.0)),
                    (0..6)
                        .map(|col| {
                            let i = row * 6 + col;
                            let hue = (tokens.hue + i as f32 / 24.0) % 1.0;
                            let sat =
                                (0.35 + tokens.sat * 0.60 - row as f32 * 0.04).clamp(0.2, 1.0);
                            let light =
                                (0.30 + tokens.light * 0.45 + col as f32 * 0.025).clamp(0.18, 0.82);
                            UxNode::boxed(
                                Style::row()
                                    .w(Dim::Flex(1.0))
                                    .h(Dim::Flex(1.0))
                                    .radius(tokens.radius_for(6.0, Role::None, 28.0))
                                    .bg(hsl_to_rgb(hue, sat, light))
                                    .border(1.0, tokens.border(tokens.accent)),
                                vec![],
                            )
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

fn preview_controls(state: &UiState, tokens: DesignTokens) -> UxNode {
    UxNode::boxed(
        Style::col().gap(8.0),
        vec![
            UxNode::boxed(
                Style::row().gap(8.0).h(Dim::Px(38.0)),
                vec![
                    preview_button(DESIGN_MIC_ON_BUTTON, "MIC ON", false, state, tokens),
                    preview_button(DESIGN_MIC_OFF_BUTTON, "MIC OFF", false, state, tokens),
                ],
            ),
            UxNode::boxed(
                Style::row().gap(8.0).h(Dim::Px(38.0)),
                vec![
                    preview_button(DESIGN_MUTE_TOGGLE, "MUTE", true, state, tokens),
                    preview_button(DESIGN_RECORD_TOGGLE, "REC", true, state, tokens),
                ],
            ),
        ],
    )
}

fn preview_button(
    id: u32,
    label: &str,
    toggle: bool,
    state: &UiState,
    tokens: DesignTokens,
) -> UxNode {
    let on = toggle && state.toggle_on(id);
    let active = state.is_hover(id) || state.is_pressed(id) || on;
    let style = Style::row()
        .w(Dim::Flex(1.0))
        .h(Dim::Flex(1.0))
        .align(Align::Center)
        .justify(Justify::Center)
        .radius(tokens.radius_for(10.0, Role::Button, 38.0))
        .bg(if active {
            mix_rgba(tokens.accent, tokens.accent_2, 0.34)
        } else {
            mix_rgba(tokens.panel, tokens.accent, 0.16)
        })
        .border(1.0, tokens.border(tokens.accent));
    let style = if toggle {
        style.toggle(id)
    } else {
        style.button(id)
    };
    UxNode::boxed(style, vec![UxNode::text(label, 12.0, tokens.ink)])
}

fn select_cycle_len(id: u32) -> usize {
    match id {
        DESIGN_TYPOGRAPHY_SELECT => TYPOGRAPHY_OPTIONS.len(),
        DESIGN_SHAPE_SELECT => SHAPE_OPTIONS.len(),
        DESIGN_MATERIAL_PRESET_SELECT => MATERIAL_OPTIONS.len(),
        _ => 0,
    }
}

fn clear_material_overrides(state: &mut UiState) {
    for id in [
        DESIGN_MATERIAL_BLUR_SLIDER,
        DESIGN_MATERIAL_REFRACTION_SLIDER,
        DESIGN_MATERIAL_DISPERSION_SLIDER,
        DESIGN_MATERIAL_RIM_SLIDER,
    ] {
        state.sliders.remove(&id);
    }
}

fn update_slider_from_x(state: &mut UiState, boxes: &[LaidBox], id: u32, x: f32) {
    if let Some(b) = boxes.iter().find(|b| b.id == Some(id)) {
        let w = (b.rect.max.x - b.rect.min.x).max(1.0);
        state.set_slider(id, (x - b.rect.min.x) / w);
    }
}

fn mix_rgba(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    Rgba::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Rgba {
    let h = h.rem_euclid(1.0);
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = l - c * 0.5;
    Rgba::new(r1 + m, g1 + m, b1 + m, 1.0)
}

/// Solve layout in **logical** units (physical size divided by the DPI scale).
fn solve_for(build: &dyn Fn(&UiState) -> UxNode, state: &UiState) -> Vec<LaidBox> {
    let s = state.scale.max(0.1);
    let tree = with_design_customizer(build(state), state);
    let vp = Bounds {
        min: Vec2::new(0.0, 0.0),
        max: Vec2::new(state.width as f32 / s, state.height as f32 / s),
    };
    layout::solve(&tree, vp, &|id| state.scroll_of(id))
}

fn scale_bounds(b: Bounds, s: f32) -> Bounds {
    Bounds {
        min: Vec2::new(b.min.x * s, b.min.y * s),
        max: Vec2::new(b.max.x * s, b.max.y * s),
    }
}

/// Multiply solved logical boxes up to physical pixels for painting.
fn scale_boxes(boxes: &mut [LaidBox], s: f32) {
    if (s - 1.0).abs() < 1e-6 {
        return;
    }
    for b in boxes {
        b.rect = scale_bounds(b.rect, s);
        b.clip = b.clip.map(|c| scale_bounds(c, s));
        b.content_len *= s;
        match &mut b.kind {
            Painted::Box {
                radius,
                border,
                shadow,
                transparency,
                ..
            } => {
                *radius *= s;
                if let Some((w, _)) = border {
                    *w *= s;
                }
                if let Some(sh) = shadow {
                    sh.dx *= s;
                    sh.dy *= s;
                    sh.blur *= s;
                }
                if let Some(material) = transparency {
                    *material = material.scale_pixels(s);
                }
            }
            Painted::Text { size, .. } => *size *= s,
            Painted::Rich { spans, .. } => {
                for sp in spans {
                    sp.size *= s;
                }
            }
        }
    }
}

fn rect_contains(b: Bounds, x: f32, y: f32) -> bool {
    x >= b.min.x && x < b.max.x && y >= b.min.y && y < b.max.y
}

/// Scrollbar track + thumb geometry for a scroll region: `(bar_x, track_top, track_h,
/// thumb_y, thumb_h, max_scroll)`. `None` when there is nothing to scroll. `s` is the
/// unit scale of `b` (1.0 for logical boxes, the DPI factor for painted boxes) so the
/// fixed insets stay proportional and the two spaces agree exactly.
fn scrollbar_geom(b: &LaidBox, scroll: f32, s: f32) -> Option<(f32, f32, f32, f32, f32, f32)> {
    if b.role != Role::Scroll {
        return None;
    }
    let view_h = b.rect.max.y - b.rect.min.y;
    let max = (b.content_len - view_h).max(0.0);
    if max <= 0.0 {
        return None;
    }
    let track_top = b.rect.min.y + 4.0 * s;
    let track_h = (view_h - 8.0 * s).max(1.0);
    let bar_x = b.rect.max.x - 7.0 * s;
    let thumb_h = (view_h / b.content_len * track_h).clamp((24.0 * s).min(track_h), track_h);
    let t = (scroll / max).clamp(0.0, 1.0);
    let thumb_y = track_top + t * (track_h - thumb_h);
    Some((bar_x, track_top, track_h, thumb_y, thumb_h, max))
}

/// The solved rectangle of the box with the given id under the current state.
/// Useful for placing synthetic events and for tests.
pub fn widget_rect(build: &dyn Fn(&UiState) -> UxNode, state: &UiState, id: u32) -> Option<Bounds> {
    solve_for(build, state)
        .into_iter()
        .find(|b| b.id == Some(id))
        .map(|b| b.rect)
}

/// Advance the interaction state machine by one event. `build` produces the current tree.
pub fn handle_event(state: &mut UiState, build: &dyn Fn(&UiState) -> UxNode, ev: UiEvent) {
    match ev {
        UiEvent::Resize(w, h) => {
            state.width = w;
            state.height = h;
        }
        UiEvent::PointerMove(x, y) => {
            if let Some(id) = state.slider_drag {
                let boxes = solve_for(build, state);
                update_slider_from_x(state, &boxes, id, x);
                return;
            }
            if let Some(id) = state.drag {
                let boxes = solve_for(build, state);
                if let Some(b) = boxes.iter().find(|b| b.id == Some(id)) {
                    if let Some((_bx, track_top, track_h, _ty, thumb_h, max)) =
                        scrollbar_geom(b, state.scroll_of(id), 1.0)
                    {
                        let denom = (track_h - thumb_h).max(1e-3);
                        let t = ((y - track_top - state.drag_grab) / denom).clamp(0.0, 1.0);
                        state.scrolls.insert(id, t * max);
                    }
                }
                return;
            }
            let boxes = solve_for(build, state);
            if let Some((id, anchor)) = state.input_drag_anchor {
                let focus = input_caret_from_pointer(state, &boxes, id, x, y);
                set_input_selection(state, id, anchor, focus);
                state.hover = layout::hit_test(&boxes, x, y).map(|(id, _)| id);
                return;
            }
            state.hover = layout::hit_test(&boxes, x, y).map(|(id, _)| id);
        }
        UiEvent::PointerDown(x, y) => {
            let boxes = solve_for(build, state);
            state.drag = None;
            state.slider_drag = None;
            state.input_drag_anchor = None;
            let mut over_scrollbar = false;
            for b in &boxes {
                let Some(id) = b.id else { continue };
                if b.clip.is_some_and(|c| !rect_contains(c, x, y)) {
                    continue;
                }
                if let Some((bar_x, _tt, _th, thumb_y, thumb_h, _max)) =
                    scrollbar_geom(b, state.scroll_of(id), 1.0)
                {
                    if x >= bar_x - 4.0 && x <= bar_x + 8.0 && rect_contains(b.rect, x, y) {
                        over_scrollbar = true;
                        if y >= thumb_y && y <= thumb_y + thumb_h {
                            state.drag = Some(id);
                            state.drag_grab = y - thumb_y;
                        }
                    }
                }
            }
            let hit = layout::hit_test(&boxes, x, y);
            // Clicking inside a text input focuses it (even when an inner region — e.g.
            // the input's own scroll box — is the topmost hit); anywhere else clears focus.
            let input_hit = boxes
                .iter()
                .rfind(|b| {
                    b.role == Role::Input
                        && b.id.is_some()
                        && rect_contains(b.rect, x, y)
                        && b.clip.is_none_or(|c| rect_contains(c, x, y))
                })
                .and_then(|b| b.id);
            state.focused = input_hit;
            if let Some(id) = input_hit {
                // A press on a scrollbar column (thumb or track) must not place the
                // caret or arm a drag-selection.
                if state.drag.is_none() && !over_scrollbar {
                    let caret = input_caret_from_pointer(state, &boxes, id, x, y);
                    set_input_caret(state, id, caret, false);
                    state.input_drag_anchor = Some((id, caret));
                }
            }
            if let Some((id, Role::Slider)) = hit {
                state.slider_drag = Some(id);
                update_slider_from_x(state, &boxes, id, x);
            }
            if state.drag.is_some() {
                state.pressed = None;
            } else {
                state.pressed = hit.map(|(id, _)| id);
            }
        }
        UiEvent::PointerUp(x, y) => {
            state.input_drag_anchor = None;
            if state.drag.is_some() {
                state.drag = None;
                state.pressed = None;
                return;
            }
            if state.slider_drag.is_some() {
                state.slider_drag = None;
                state.pressed = None;
                state.clicked = None;
                return;
            }
            let boxes = solve_for(build, state);
            state.clicked = None;
            if let (Some((up_id, role)), Some(pressed)) =
                (layout::hit_test(&boxes, x, y), state.pressed)
            {
                if up_id == pressed {
                    state.clicked = Some(up_id);
                    if role == Role::Toggle {
                        let now = state.toggle_on(up_id);
                        state.toggles.insert(up_id, !now);
                    } else if role == Role::Button && up_id == DESIGN_MATERIAL_PREVIOUS_BUTTON {
                        let len = MATERIAL_OPTIONS.len();
                        let previous =
                            (state.select_index(DESIGN_MATERIAL_PRESET_SELECT) + len - 1) % len;
                        state.set_select_index(DESIGN_MATERIAL_PRESET_SELECT, previous);
                        clear_material_overrides(state);
                    } else if role == Role::Button && up_id == DESIGN_MATERIAL_RESET_BUTTON {
                        clear_material_overrides(state);
                    } else if role == Role::Select {
                        let len = select_cycle_len(up_id);
                        if len > 0 {
                            let next = (state.select_index(up_id) + 1) % len;
                            state.set_select_index(up_id, next);
                            if up_id == DESIGN_MATERIAL_PRESET_SELECT {
                                clear_material_overrides(state);
                            }
                        }
                    }
                }
            }
            state.pressed = None;
        }
        UiEvent::Wheel(x, y, delta) => {
            let boxes = solve_for(build, state);
            // Topmost scroll region under the cursor that actually has overflow, so a
            // fitted inner region (e.g. a short input) doesn't swallow page scrolling.
            let mut target: Option<(u32, f32, f32)> = None;
            for b in &boxes {
                if b.role == Role::Scroll
                    && rect_contains(b.rect, x, y)
                    && b.clip.is_none_or(|c| rect_contains(c, x, y))
                {
                    let view_h = b.rect.max.y - b.rect.min.y;
                    if b.content_len > view_h + 0.5 {
                        if let Some(id) = b.id {
                            target = Some((id, view_h, b.content_len));
                        }
                    }
                }
            }
            if let Some((id, view_h, content_len)) = target {
                let max = (content_len - view_h).max(0.0);
                let next = (state.scroll_of(id).clamp(0.0, max) + delta).clamp(0.0, max);
                state.scrolls.insert(id, next);
            }
        }
        UiEvent::Char(c) => {
            if let Some(id) = state.focused {
                if !c.is_control() {
                    insert_input_text(state, id, &c.to_string());
                }
            }
        }
        UiEvent::Backspace => {
            if let Some(id) = state.focused {
                backspace_input(state, id);
            }
        }
        UiEvent::Delete => {
            if let Some(id) = state.focused {
                delete_forward_input(state, id);
            }
        }
        UiEvent::MoveLeft { shift } => {
            if let Some(id) = state.focused {
                move_input_caret(state, id, false, shift);
            }
        }
        UiEvent::MoveRight { shift } => {
            if let Some(id) = state.focused {
                move_input_caret(state, id, true, shift);
            }
        }
        UiEvent::SelectAll => {
            if let Some(id) = state.focused {
                select_all_input(state, id);
            }
        }
        UiEvent::Copy => {
            if let Some(id) = state.focused {
                state.clipboard_out = selected_input_text(state, id);
            }
        }
        UiEvent::Cut => {
            if let Some(id) = state.focused {
                state.clipboard_out = selected_input_text(state, id);
                delete_input_selection(state, id);
            }
        }
        UiEvent::Paste(text) => {
            if let Some(id) = state.focused {
                insert_input_text(state, id, &text);
            }
        }
        UiEvent::Enter => {
            state.submit = state.focused;
        }
        UiEvent::Tick(dt) => {
            state.animation_time = (state.animation_time + dt.max(0.0)).rem_euclid(3600.0);
        }
    }
}

/// Render the interactive UI for the current state, including scrollbars. Layout is
/// solved in logical units and painted at `state.scale` physical pixels per unit.
pub fn render_ui(build: &dyn Fn(&UiState) -> UxNode, state: &UiState, clear: Rgba) -> Framebuffer {
    let s = state.scale.max(0.1);
    let mut boxes = solve_for(build, state);
    scale_boxes(&mut boxes, s);
    let mut fb = paint_boxes_banded(state.width, state.height, clear, &boxes);
    draw_scrollbars(&mut fb, &boxes, state, s);
    fb.set_output_gamma(DesignTokens::from_state(state).output_gamma());
    fb
}

/// `boxes` are already in physical pixels here; scroll offsets are logical, so they are
/// scaled up to match before the thumb geometry is computed.
fn draw_scrollbars(fb: &mut Framebuffer, boxes: &[LaidBox], state: &UiState, s: f32) {
    for b in boxes {
        let Some(id) = b.id else { continue };
        if let Some((bar_x, track_top, track_h, thumb_y, thumb_h, _max)) =
            scrollbar_geom(b, state.scroll_of(id) * s, s)
        {
            let thumb_col = if state.drag == Some(id) {
                Rgba::rgb8(113, 113, 122) // zinc-500 — brighter while dragging
            } else {
                Rgba::rgb8(82, 82, 91) // zinc-600 — subtle at rest
            };
            fill_rect(
                fb,
                bar_x,
                track_top,
                4.0 * s,
                track_h,
                Rgba::rgb8(39, 39, 42), // zinc-800 — nearly invisible track
                2.0 * s,
            );
            fill_rect(fb, bar_x, thumb_y, 4.0 * s, thumb_h, thumb_col, 2.0 * s);
        }
    }
}

fn fill_rect<S: Surface>(fb: &mut S, x: f32, y: f32, w: f32, h: f32, color: Rgba, radius: f32) {
    let cmd = DrawCmd::new(
        Shape::RoundedRect {
            half: Vec2::new(w / 2.0, h / 2.0),
            radius,
        },
        Paint::Solid(color),
        Affine::translate(x + w / 2.0, y + h / 2.0),
    );
    raster::scan_convert(&cmd, fb, None);
}

#[cfg(test)]
mod banded_render_tests {
    use super::*;
    use pmre_kit::ux::{Dim, Edges, Style, UxNode};

    fn probe_scene() -> UxNode {
        // shadows bleed far past their rect and lowercase glyphs cross band edges —
        // both must land in the right lanes or the banded render seams.
        let panel = |c: Rgba, label: &str| {
            UxNode::boxed(
                Style::col()
                    .w(Dim::Flex(1.0))
                    .h(Dim::Px(70.0))
                    .pad(Edges::all(10.0))
                    .gap(6.0)
                    .radius(12.0)
                    .bg(Rgba::rgb8(20, 20, 28))
                    .border(2.0, c)
                    .shadow(0.0, 6.0, 14.0, Rgba::new(0.0, 0.0, 0.0, 0.5)),
                vec![UxNode::text(label, 18.0, c)],
            )
        };
        UxNode::boxed(
            Style::col()
                .w(Dim::Flex(1.0))
                .h(Dim::Flex(1.0))
                .pad(Edges::all(14.0))
                .gap(12.0)
                .bg(Rgba::rgb8(8, 8, 12)),
            vec![
                UxNode::text("lane seam probe — gjpqy", 16.0, Rgba::rgb8(200, 200, 220)),
                panel(Rgba::rgb8(0, 220, 180), "alpha jumping quickly"),
                panel(Rgba::rgb8(255, 120, 80), "bravo gyrating deeply"),
                panel(Rgba::rgb8(160, 90, 255), "charlie playing jazz"),
            ],
        )
    }

    /// The lane/bus render must be bit-identical to the single-threaded render — each
    /// pixel is produced by exactly one lane, so band boundaries leave no seam.
    #[test]
    fn banded_render_matches_serial() {
        let (w, h) = (260u32, 320u32);
        let clear = Rgba::rgb8(8, 8, 12);
        let boxes = layout::solve(&probe_scene(), viewport(w, h), &|_| 0.0);

        let mut serial = Framebuffer::new(w, h, clear);
        paint_boxes(&mut serial, &boxes);
        let banded = paint_boxes_banded(w, h, clear, &boxes);

        let mut maxd = 0f32;
        let mut worst = (0u32, 0u32);
        for i in 0..(w * h) as usize {
            let (a, b) = (serial.pixels()[i], banded.pixels()[i]);
            let d = (a.r - b.r)
                .abs()
                .max((a.g - b.g).abs())
                .max((a.b - b.b).abs())
                .max((a.a - b.a).abs());
            if d > maxd {
                maxd = d;
                worst = (i as u32 % w, i as u32 / w);
            }
        }
        assert!(
            maxd < 1e-6,
            "lane render diverged from serial at {worst:?}: max diff {maxd}"
        );
    }

    fn customizer_probe() -> UxNode {
        UxNode::boxed(
            Style::col()
                .w(Dim::Flex(1.0))
                .h(Dim::Flex(1.0))
                .gap(10.0)
                .pad(Edges::all(14.0))
                .radius(8.0)
                .bg(Rgba::rgb8(245, 247, 246))
                .shadow(0.0, 6.0, 18.0, Rgba::new(0.0, 0.0, 0.0, 0.24)),
            vec![
                UxNode::text("Dashboard", 22.0, Rgba::rgb8(20, 28, 28)),
                UxNode::boxed(
                    Style::row()
                        .button(42)
                        .h(Dim::Px(40.0))
                        .align(Align::Center)
                        .justify(Justify::Center)
                        .radius(7.0)
                        .bg(Rgba::rgb8(0, 132, 142)),
                    vec![UxNode::text("Run", 13.0, Rgba::rgb8(255, 255, 255))],
                ),
            ],
        )
    }

    #[test]
    fn design_tab_is_auto_injected() {
        let ui = UiState::new(920, 620);
        let build = |_: &UiState| customizer_probe();
        assert!(widget_rect(&build, &ui, DESIGN_TAB_ID).is_some());
        assert!(widget_rect(&build, &ui, 42).is_some());
        assert!(widget_rect(&build, &ui, DESIGN_HUE_SLIDER).is_none());
    }

    #[test]
    fn design_panel_opens_and_slider_drag_updates_state() {
        let mut ui = UiState::new(920, 620);
        let build = |_: &UiState| customizer_probe();
        let tab = widget_rect(&build, &ui, DESIGN_TAB_ID).unwrap();
        let tx = (tab.min.x + tab.max.x) * 0.5;
        let ty = (tab.min.y + tab.max.y) * 0.5;
        handle_event(&mut ui, &build, UiEvent::PointerDown(tx, ty));
        handle_event(&mut ui, &build, UiEvent::PointerUp(tx, ty));
        assert!(ui.toggle_on(DESIGN_TAB_ID));
        assert!(widget_rect(&build, &ui, DESIGN_GAMMA_SLIDER).is_some());
        assert!(widget_rect(&build, &ui, DESIGN_MATERIAL_PRESET_SELECT).is_some());
        assert!(widget_rect(&build, &ui, DESIGN_MATERIAL_ADVANCED_TOGGLE).is_none());
        assert!(widget_rect(&build, &ui, DESIGN_MATERIAL_BLUR_SLIDER).is_none());

        let slider = widget_rect(&build, &ui, DESIGN_HUE_SLIDER).unwrap();
        let y = (slider.min.y + slider.max.y) * 0.5;
        handle_event(&mut ui, &build, UiEvent::PointerDown(slider.min.x + 1.0, y));
        handle_event(&mut ui, &build, UiEvent::PointerMove(slider.max.x - 2.0, y));
        handle_event(&mut ui, &build, UiEvent::PointerUp(slider.max.x - 2.0, y));
        assert!(ui.slider_value(DESIGN_HUE_SLIDER, 0.0) > 0.90);

        let gamma = widget_rect(&build, &ui, DESIGN_GAMMA_SLIDER).unwrap();
        let gamma_y = (gamma.min.y + gamma.max.y) * 0.5;
        handle_event(
            &mut ui,
            &build,
            UiEvent::PointerDown(gamma.min.x + 1.0, gamma_y),
        );
        handle_event(
            &mut ui,
            &build,
            UiEvent::PointerMove(gamma.max.x - 2.0, gamma_y),
        );
        handle_event(
            &mut ui,
            &build,
            UiEvent::PointerUp(gamma.max.x - 2.0, gamma_y),
        );
        assert!(ui.slider_value(DESIGN_GAMMA_SLIDER, 0.0) > 0.90);
    }

    #[test]
    fn gamma_customizer_changes_presentation_encoding_not_raw_framebuffer() {
        let build = |_: &UiState| customizer_probe();
        let mut neutral = UiState::new(920, 620);
        neutral.set_slider(DESIGN_GAMMA_SLIDER, DESIGN_GAMMA_NEUTRAL_SLIDER);
        let mut bright = UiState::new(920, 620);
        bright.set_slider(DESIGN_GAMMA_SLIDER, 0.85);

        let neutral_frame = render_ui(&build, &neutral, Rgba::rgb8(8, 8, 10));
        let bright_frame = render_ui(&build, &bright, Rgba::rgb8(8, 8, 10));
        assert!(neutral_frame
            .pixels()
            .iter()
            .zip(bright_frame.pixels())
            .all(|(left, right)| {
                (left.r - right.r).abs() < f32::EPSILON
                    && (left.g - right.g).abs() < f32::EPSILON
                    && (left.b - right.b).abs() < f32::EPSILON
                    && (left.a - right.a).abs() < f32::EPSILON
            }));
        assert!((neutral_frame.output_gamma() - 1.0).abs() < f32::EPSILON);
        assert!((bright_frame.output_gamma() - 2.2).abs() < 1e-6);

        let neutral_pixels = neutral_frame.to_u32(Rgba::rgb8(8, 8, 10));
        let bright_pixels = bright_frame.to_u32(Rgba::rgb8(8, 8, 10));
        assert_ne!(neutral_pixels, bright_pixels);
        let channel_sum = |pixels: &[u32]| -> u64 {
            pixels
                .iter()
                .map(|pixel| {
                    u64::from((pixel >> 16) & 0xff)
                        + u64::from((pixel >> 8) & 0xff)
                        + u64::from(pixel & 0xff)
                })
                .sum()
        };
        assert!(channel_sum(&bright_pixels) > channel_sum(&neutral_pixels));
    }

    #[test]
    fn design_select_cycles_typography() {
        let mut ui = UiState::new(920, 620);
        ui.toggles.insert(DESIGN_TAB_ID, true);
        ui.scrolls.insert(DESIGN_PANEL_SCROLL, 160.0);
        let build = |_: &UiState| customizer_probe();
        let select = widget_rect(&build, &ui, DESIGN_TYPOGRAPHY_SELECT).unwrap();
        let x = (select.min.x + select.max.x) * 0.5;
        let y = (select.min.y + select.max.y) * 0.5;
        handle_event(&mut ui, &build, UiEvent::PointerDown(x, y));
        handle_event(&mut ui, &build, UiEvent::PointerUp(x, y));
        assert_eq!(ui.select_index(DESIGN_TYPOGRAPHY_SELECT), 1);
    }

    #[test]
    fn transparency_material_is_opt_in_and_advanced_controls_expand() {
        let build = |_: &UiState| customizer_probe();
        let mut ui = UiState::new(920, 620);
        assert!(!has_transparency(&solve_for(&build, &ui)));

        ui.toggles.insert(DESIGN_TAB_ID, true);
        assert!(widget_rect(&build, &ui, DESIGN_MATERIAL_PRESET_SELECT).is_some());
        assert!(widget_rect(&build, &ui, DESIGN_MATERIAL_BLUR_SLIDER).is_none());
        ui.set_select_index(DESIGN_MATERIAL_PRESET_SELECT, 1);
        let advanced = widget_rect(&build, &ui, DESIGN_MATERIAL_ADVANCED_TOGGLE).unwrap();
        let x = (advanced.min.x + advanced.max.x) * 0.5;
        let y = (advanced.min.y + advanced.max.y) * 0.5;
        handle_event(&mut ui, &build, UiEvent::PointerDown(x, y));
        handle_event(&mut ui, &build, UiEvent::PointerUp(x, y));
        assert!(ui.toggle_on(DESIGN_MATERIAL_ADVANCED_TOGGLE));
        assert!(widget_rect(&build, &ui, DESIGN_MATERIAL_BLUR_SLIDER).is_some());
        assert!(widget_rect(&build, &ui, DESIGN_MATERIAL_REFRACTION_SLIDER).is_some());
        assert!(widget_rect(&build, &ui, DESIGN_MATERIAL_DISPERSION_SLIDER).is_some());
        assert!(widget_rect(&build, &ui, DESIGN_MATERIAL_RIM_SLIDER).is_some());

        let blur = widget_rect(&build, &ui, DESIGN_MATERIAL_BLUR_SLIDER).unwrap();
        let blur_y = (blur.min.y + blur.max.y) * 0.5;
        handle_event(
            &mut ui,
            &build,
            UiEvent::PointerDown(blur.max.x - 1.0, blur_y),
        );
        handle_event(
            &mut ui,
            &build,
            UiEvent::PointerUp(blur.max.x - 1.0, blur_y),
        );
        let material = DesignTokens::from_state(&ui).material.unwrap();
        assert!(material.blur_radius_px > 23.0);
    }

    #[test]
    fn material_preset_changes_frame_and_new_selection_clears_overrides() {
        let build = |_: &UiState| customizer_probe();
        let mut legacy = UiState::new(920, 620);
        let legacy_frame = render_ui(&build, &legacy, Rgba::rgb8(8, 8, 10));

        legacy.set_slider(DESIGN_MATERIAL_BLUR_SLIDER, 0.99);
        legacy.toggles.insert(DESIGN_TAB_ID, true);
        let select = widget_rect(&build, &legacy, DESIGN_MATERIAL_PRESET_SELECT).unwrap();
        let x = (select.min.x + select.max.x) * 0.5;
        let y = (select.min.y + select.max.y) * 0.5;
        handle_event(&mut legacy, &build, UiEvent::PointerDown(x, y));
        handle_event(&mut legacy, &build, UiEvent::PointerUp(x, y));

        assert_eq!(legacy.select_index(DESIGN_MATERIAL_PRESET_SELECT), 1);
        assert!(!legacy.sliders.contains_key(&DESIGN_MATERIAL_BLUR_SLIDER));
        assert!(has_transparency(&solve_for(&build, &legacy)));
        let material_frame = render_ui(&build, &legacy, Rgba::rgb8(8, 8, 10));
        assert_ne!(
            legacy_frame.to_u32(Rgba::rgb8(8, 8, 10)),
            material_frame.to_u32(Rgba::rgb8(8, 8, 10))
        );
    }

    #[test]
    fn material_previous_reset_and_zero_glass_are_deterministic() {
        let build = |_: &UiState| customizer_probe();
        let mut ui = UiState::new(920, 620);
        ui.toggles.insert(DESIGN_TAB_ID, true);
        let previous = widget_rect(&build, &ui, DESIGN_MATERIAL_PREVIOUS_BUTTON).unwrap();
        let x = (previous.min.x + previous.max.x) * 0.5;
        let y = (previous.min.y + previous.max.y) * 0.5;
        handle_event(&mut ui, &build, UiEvent::PointerDown(x, y));
        handle_event(&mut ui, &build, UiEvent::PointerUp(x, y));
        assert_eq!(
            ui.select_index(DESIGN_MATERIAL_PRESET_SELECT),
            MATERIAL_OPTIONS.len() - 1
        );

        ui.toggles.insert(DESIGN_MATERIAL_ADVANCED_TOGGLE, true);
        ui.set_slider(DESIGN_MATERIAL_BLUR_SLIDER, 0.99);
        ui.scrolls.insert(DESIGN_PANEL_SCROLL, 180.0);
        let reset = widget_rect(&build, &ui, DESIGN_MATERIAL_RESET_BUTTON).unwrap();
        let x = (reset.min.x + reset.max.x) * 0.5;
        let y = (reset.min.y + reset.max.y) * 0.5;
        handle_event(&mut ui, &build, UiEvent::PointerDown(x, y));
        handle_event(&mut ui, &build, UiEvent::PointerUp(x, y));
        assert!(!ui.sliders.contains_key(&DESIGN_MATERIAL_BLUR_SLIDER));

        let mut legacy = UiState::new(920, 620);
        legacy.set_slider(DESIGN_GLASS_SLIDER, 0.0);
        let legacy_frame = render_ui(&build, &legacy, Rgba::rgb8(8, 8, 10));
        legacy.set_select_index(DESIGN_MATERIAL_PRESET_SELECT, 1);
        assert!(!has_transparency(&solve_for(&build, &legacy)));
        let zero_frame = render_ui(&build, &legacy, Rgba::rgb8(8, 8, 10));
        assert_eq!(
            legacy_frame.to_u32(Rgba::rgb8(8, 8, 10)),
            zero_frame.to_u32(Rgba::rgb8(8, 8, 10))
        );
    }

    #[test]
    fn every_material_preset_maps_to_its_recipe_without_overrides() {
        let legacy = DesignTokens::from_state(&UiState::new(320, 240));
        assert!(legacy.material.is_none());
        assert_eq!(legacy.material_blur, 0.0);
        assert_eq!(legacy.material_refraction, 0.0);
        assert_eq!(legacy.material_dispersion, 0.0);
        assert_eq!(legacy.material_rim, 0.0);

        for (index, preset) in MaterialPreset::ALL.iter().copied().enumerate() {
            let mut ui = UiState::new(320, 240);
            ui.set_select_index(DESIGN_MATERIAL_PRESET_SELECT, index + 1);
            let tokens = DesignTokens::from_state(&ui);
            let actual = tokens.material.unwrap();
            let expected = preset.material();
            assert!((actual.blur_radius_px - expected.blur_radius_px).abs() < 1.0e-6);
            assert!((actual.refraction_px - expected.refraction_px).abs() < 1.0e-6);
            assert!((actual.dispersion_px - expected.dispersion_px).abs() < 1.0e-6);
            assert!((actual.rim_width_px - expected.rim_width_px).abs() < 1.0e-6);
            assert!((actual.strength - expected.strength * 0.20).abs() < 1.0e-6);
        }
    }

    #[test]
    fn material_adaptive_render_matches_explicit_serial_render() {
        let material = MaterialPreset::Crystal.material();
        let root = UxNode::boxed(
            Style::col()
                .w(Dim::Flex(1.0))
                .h(Dim::Flex(1.0))
                .pad(Edges::all(18.0))
                .bg(Rgba::rgb8(12, 20, 32)),
            vec![UxNode::boxed(
                Style::col()
                    .w(Dim::Flex(1.0))
                    .h(Dim::Flex(1.0))
                    .radius(18.0)
                    .bg(Rgba::new(0.2, 0.8, 0.9, 0.5))
                    .transparency(material),
                vec![],
            )],
        );
        let adaptive = render_uxi(&root, 180, 120, Rgba::rgb8(5, 6, 8));
        let serial = render_uxi_serial(&root, 180, 120, Rgba::rgb8(5, 6, 8));
        assert!(adaptive.pixels().iter().zip(serial.pixels()).all(|(a, b)| {
            (a.r - b.r).abs() < 1.0e-6
                && (a.g - b.g).abs() < 1.0e-6
                && (a.b - b.b).abs() < 1.0e-6
                && (a.a - b.a).abs() < 1.0e-6
        }));
    }

    #[test]
    fn zero_to_tiny_material_strength_is_continuous() {
        let scene = |strength: f32| {
            let mut material = MaterialPreset::FrostedGlass.material();
            material.strength = strength;
            UxNode::boxed(
                Style::col()
                    .w(Dim::Flex(1.0))
                    .h(Dim::Flex(1.0))
                    .pad(Edges::all(16.0))
                    .bg(Rgba::rgb8(18, 24, 38)),
                vec![UxNode::boxed(
                    Style::col()
                        .w(Dim::Flex(1.0))
                        .h(Dim::Flex(1.0))
                        .radius(12.0)
                        .bg(Rgba::new(0.12, 0.62, 0.78, 0.72))
                        .border(2.0, Rgba::new(0.7, 0.9, 1.0, 0.8))
                        .shadow(0.0, 4.0, 10.0, Rgba::new(0.0, 0.0, 0.0, 0.4))
                        .transparency(material),
                    vec![],
                )],
            )
        };
        let zero_tree = scene(0.0);
        let tiny_tree = scene(1.0e-5);
        let zero_boxes = layout::solve(&zero_tree, viewport(160, 100), &|_| 0.0);
        let tiny_boxes = layout::solve(&tiny_tree, viewport(160, 100), &|_| 0.0);
        assert!(!has_transparency(&zero_boxes));
        assert!(has_transparency(&tiny_boxes));
        let zero = render_uxi(&zero_tree, 160, 100, Rgba::rgb8(5, 6, 8));
        let tiny = render_uxi(&tiny_tree, 160, 100, Rgba::rgb8(5, 6, 8));
        let max_delta = zero
            .pixels()
            .iter()
            .zip(tiny.pixels())
            .map(|(a, b)| {
                (a.r - b.r)
                    .abs()
                    .max((a.g - b.g).abs())
                    .max((a.b - b.b).abs())
                    .max((a.a - b.a).abs())
            })
            .fold(0.0f32, f32::max);
        assert!(max_delta < 1.0e-3, "strength discontinuity: {max_delta}");
    }

    #[test]
    fn render_ui_includes_customizer_pixels() {
        let mut ui = UiState::new(920, 620);
        ui.toggles.insert(DESIGN_TAB_ID, true);
        let build = |_: &UiState| customizer_probe();
        let fb = render_ui(&build, &ui, Rgba::rgb8(8, 8, 10));
        assert!(fb
            .pixels()
            .iter()
            .any(|p| p.r > 0.10 || p.g > 0.10 || p.b > 0.10));
    }

    #[test]
    fn scrolled_input_click_places_the_caret_on_the_clicked_line() {
        const INPUT_ID: u32 = 71;
        const INPUT_SCROLL_ID: u32 = 72;
        let build = |state: &UiState| {
            UxNode::boxed(
                Style::col()
                    .input(INPUT_ID)
                    .w(Dim::Px(220.0))
                    .h(Dim::Px(64.0))
                    .pad(Edges::all(10.0)),
                vec![UxNode::boxed(
                    Style::col().scroll(INPUT_SCROLL_ID).h(Dim::Flex(1.0)),
                    vec![UxNode::text(
                        state.input_text(INPUT_ID),
                        14.0,
                        Rgba::rgb8(240, 240, 240),
                    )],
                )],
            )
        };
        let words: Vec<String> = (0..40).map(|i| format!("word{i:02}")).collect();
        let text = words.join(" ");
        let mut ui = UiState::new(320, 120);
        ui.inputs.insert(INPUT_ID, text.clone());
        // pin the inner scroll to the bottom so the visible lines are the LAST ones
        ui.scrolls.insert(INPUT_SCROLL_ID, 1.0e9);
        let rect = widget_rect(&build, &ui, INPUT_ID).unwrap();
        // click near the bottom of the input: this is one of the final wrapped lines
        let x = rect.min.x + 20.0;
        let y = rect.max.y - 14.0;
        handle_event(&mut ui, &build, UiEvent::PointerDown(x, y));
        handle_event(&mut ui, &build, UiEvent::PointerUp(x, y));
        assert_eq!(ui.focused, Some(INPUT_ID));
        let caret = ui.input_caret(INPUT_ID);
        let len = text.chars().count();
        // with the scroll offset ignored (the old bug) the caret landed on one of the
        // FIRST lines; line-aware mapping must land it deep in the scrolled tail
        assert!(
            caret > len / 2,
            "caret {caret} of {len} landed on an early line despite the scrolled view"
        );
    }

    #[test]
    fn input_editor_supports_caret_selection_delete_and_clipboard() {
        const INPUT_ID: u32 = 7;
        let build = |_: &UiState| {
            UxNode::boxed(
                Style::col()
                    .input(INPUT_ID)
                    .w(Dim::Px(240.0))
                    .h(Dim::Px(38.0)),
                vec![UxNode::text("field", 14.0, Rgba::rgb8(240, 240, 240))],
            )
        };
        let mut ui = UiState::new(320, 80);
        ui.focused = Some(INPUT_ID);
        ui.inputs.insert(INPUT_ID, "abcd".to_string());
        ui.input_carets.insert(INPUT_ID, 2);

        handle_event(&mut ui, &build, UiEvent::Char('X'));
        assert_eq!(ui.input_text(INPUT_ID), "abXcd");
        assert_eq!(ui.input_caret(INPUT_ID), 3);

        handle_event(&mut ui, &build, UiEvent::SelectAll);
        assert_eq!(ui.input_selection(INPUT_ID), Some((0, 5)));
        handle_event(&mut ui, &build, UiEvent::Copy);
        assert_eq!(ui.take_clipboard_out().as_deref(), Some("abXcd"));

        handle_event(&mut ui, &build, UiEvent::Cut);
        assert_eq!(ui.take_clipboard_out().as_deref(), Some("abXcd"));
        assert_eq!(ui.input_text(INPUT_ID), "");
        assert_eq!(ui.input_caret(INPUT_ID), 0);

        handle_event(&mut ui, &build, UiEvent::Paste("xy".to_string()));
        assert_eq!(ui.input_text(INPUT_ID), "xy");
        handle_event(&mut ui, &build, UiEvent::MoveLeft { shift: false });
        handle_event(&mut ui, &build, UiEvent::Char('Z'));
        assert_eq!(ui.input_text(INPUT_ID), "xZy");
        handle_event(&mut ui, &build, UiEvent::Backspace);
        assert_eq!(ui.input_text(INPUT_ID), "xy");
        handle_event(&mut ui, &build, UiEvent::Delete);
        assert_eq!(ui.input_text(INPUT_ID), "x");
    }
}
