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
                    let (asc_p, _) = text::v_metrics_styled(p.size, p.bold);
                    let origin = Vec2::new(x0 + p.x, baseline - asc_p);
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
    paint_boxes(&mut fb, &boxes);
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

#[derive(Clone, Copy)]
struct DesignTokens {
    hue: f32,
    sat: f32,
    light: f32,
    text: f32,
    radius: f32,
    glass: f32,
    animation: f32,
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
        Self {
            hue,
            sat,
            light,
            text,
            radius,
            glass,
            animation,
            phase: state.animation_time,
            typography,
            shape,
            accent,
            accent_2,
            ink,
            panel,
        }
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
    pub fn clear_input(&mut self, id: u32) {
        self.inputs.remove(&id);
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
    /// Enter pressed; marks the focused input as submitted.
    Enter,
    /// Advance renderer-owned animation time by this many seconds.
    Tick(f32),
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
        vec![
            UxNode::text("Renderer Customizer", 18.0, tokens.ink),
            UxNode::text(
                "Dependency-free style controls for generated PMRE apps.",
                11.0,
                tokens.ink,
            ),
            slider_control("Hue", DESIGN_HUE_SLIDER, tokens.hue, tokens),
            slider_control("Saturation", DESIGN_SAT_SLIDER, tokens.sat, tokens),
            slider_control("Light", DESIGN_LIGHT_SLIDER, tokens.light, tokens),
            slider_control("Text", DESIGN_TEXT_SLIDER, tokens.text, tokens),
            slider_control("Radius", DESIGN_RADIUS_SLIDER, tokens.radius, tokens),
            slider_control("Glass", DESIGN_GLASS_SLIDER, tokens.glass, tokens),
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
        ],
    )
}

fn slider_control(label: &str, id: u32, value: f32, tokens: DesignTokens) -> UxNode {
    let pct = value.clamp(0.0, 1.0);
    UxNode::boxed(
        Style::col().gap(5.0),
        vec![
            UxNode::boxed(
                Style::row().justify(Justify::SpaceBetween),
                vec![
                    UxNode::text(label, 11.0, tokens.ink),
                    UxNode::text(
                        format!("{}%", (pct * 100.0).round() as u32),
                        11.0,
                        tokens.ink,
                    ),
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
        _ => 0,
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
            state.hover = layout::hit_test(&boxes, x, y).map(|(id, _)| id);
        }
        UiEvent::PointerDown(x, y) => {
            let boxes = solve_for(build, state);
            state.drag = None;
            state.slider_drag = None;
            for b in &boxes {
                let Some(id) = b.id else { continue };
                if let Some((bar_x, _tt, _th, thumb_y, thumb_h, _max)) =
                    scrollbar_geom(b, state.scroll_of(id), 1.0)
                {
                    if x >= bar_x - 4.0
                        && x <= bar_x + 8.0
                        && y >= thumb_y
                        && y <= thumb_y + thumb_h
                    {
                        state.drag = Some(id);
                        state.drag_grab = y - thumb_y;
                    }
                }
            }
            let hit = layout::hit_test(&boxes, x, y);
            // Clicking a text input focuses it; clicking anything else clears focus.
            state.focused = match hit {
                Some((id, Role::Input)) => Some(id),
                _ => None,
            };
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
                    } else if role == Role::Select {
                        let len = select_cycle_len(up_id);
                        if len > 0 {
                            let next = (state.select_index(up_id) + 1) % len;
                            state.set_select_index(up_id, next);
                        }
                    }
                }
            }
            state.pressed = None;
        }
        UiEvent::Wheel(x, y, delta) => {
            let boxes = solve_for(build, state);
            // Topmost scroll region under the cursor.
            let mut target: Option<(u32, f32, f32)> = None;
            for b in &boxes {
                if b.role == Role::Scroll && rect_contains(b.rect, x, y) {
                    if let Some(id) = b.id {
                        target = Some((id, b.rect.max.y - b.rect.min.y, b.content_len));
                    }
                }
            }
            if let Some((id, view_h, content_len)) = target {
                let max = (content_len - view_h).max(0.0);
                let next = (state.scroll_of(id) + delta).clamp(0.0, max);
                state.scrolls.insert(id, next);
            }
        }
        UiEvent::Char(c) => {
            if let Some(id) = state.focused {
                if !c.is_control() {
                    state.inputs.entry(id).or_default().push(c);
                }
            }
        }
        UiEvent::Backspace => {
            if let Some(id) = state.focused {
                state.inputs.entry(id).or_default().pop();
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

fn fill_rect(fb: &mut Framebuffer, x: f32, y: f32, w: f32, h: f32, color: Rgba, radius: f32) {
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
                .bg(Rgba::rgb8(245, 247, 246)),
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

        let slider = widget_rect(&build, &ui, DESIGN_HUE_SLIDER).unwrap();
        let y = (slider.min.y + slider.max.y) * 0.5;
        handle_event(&mut ui, &build, UiEvent::PointerDown(slider.min.x + 1.0, y));
        handle_event(&mut ui, &build, UiEvent::PointerMove(slider.max.x - 2.0, y));
        handle_event(&mut ui, &build, UiEvent::PointerUp(slider.max.x - 2.0, y));
        assert!(ui.slider_value(DESIGN_HUE_SLIDER, 0.0) > 0.90);
    }

    #[test]
    fn design_select_cycles_typography() {
        let mut ui = UiState::new(920, 620);
        ui.toggles.insert(DESIGN_TAB_ID, true);
        let build = |_: &UiState| customizer_probe();
        let select = widget_rect(&build, &ui, DESIGN_TYPOGRAPHY_SELECT).unwrap();
        let x = (select.min.x + select.max.x) * 0.5;
        let y = (select.min.y + select.max.y) * 0.5;
        handle_event(&mut ui, &build, UiEvent::PointerDown(x, y));
        handle_event(&mut ui, &build, UiEvent::PointerUp(x, y));
        assert_eq!(ui.select_index(DESIGN_TYPOGRAPHY_SELECT), 1);
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
}
