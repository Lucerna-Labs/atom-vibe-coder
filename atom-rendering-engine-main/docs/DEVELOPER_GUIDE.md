# Atom Rendering Engine Developer Guide

How to embed, drive, and extend the Atom Rendering Engine.

Atom Rendering Engine is a zero-external-dependency 2D UI engine: focused crates, all CPU, everything built from a
small set of mathematical primitives. This guide is for developers building apps on the
engine or extending the engine itself. For the project overview, see the
[README](../README.md).

---

## 1. Architecture in one minute

```
pmre-kit           mechanism only â€” no decisions
â”œâ”€ geom            Vec2, Affine (project atom)
â”œâ”€ paint           Rgba, Shape, Paint, DrawCmd, Porter-Duff over (combine atom)
â”œâ”€ raster          SDF coverage rasterizer with analytic anti-aliasing
â”œâ”€ path            scanline polygon/BÃ©zier filler + stroker (nonzero winding)
â”œâ”€ font            TrueType parser + AA glyph rasterizer (+ 5Ã—7 bitmap fallback)
â”œâ”€ text            text runs: advance / wrap / draw onto any Surface
â”œâ”€ ux              UxNode intent tree: Style, Span, no coordinates
â”œâ”€ layout          box-model + flex solver: intent â†’ LaidBox rects â†’ DrawCmds
â”œâ”€ framebuffer     Framebuffer, Surface trait, BandView (row-band for lanes)
â””â”€ post            bloom + parallel/tiled variants

pmre-orchestrator  policy only â€” never touches a pixel directly
â””â”€ lib             draw order, banded parallel painting, interaction state
                   machine (hover/press/click/toggle/scroll/focus), DPI scaling,
                   scrollbars, Quality tiers

pmre-transparency-core  allocation-free no_std optics/material math
pmre-transparency       premultiplied screen-space backdrop compositor
```

The rule that keeps the design honest: **if a `pmre-kit` function grows an `if` that
makes a value judgement, that `if` belongs in the orchestrator.** The kit computes;
the orchestrator decides.

The default renderer crates have **zero external dependencies**. Do not add any. Fonts come from the
OS font directory via `std::fs`; the window in `examples/app.rs` is raw Win32 FFI.

---

## 2. Getting started

Workspace layout â€” add the crates by path (they are not on crates.io):

```toml
[dependencies]
pmre-kit = { path = "../primitive-math-rendering-engine/pmre-kit" }
pmre-orchestrator = { path = "../primitive-math-rendering-engine/pmre-orchestrator" }
```

Render your first frame:

```rust
use pmre_kit::{Dim, Edges, Rgba, Style, UxNode};
use pmre_orchestrator::render_uxi;

fn main() {
    let ui = UxNode::boxed(
        Style::col()
            .w(Dim::Flex(1.0))
            .h(Dim::Flex(1.0))
            .pad(Edges::all(24.0))
            .gap(12.0)
            .bg(Rgba::rgb8(24, 24, 27)),
        vec![
            UxNode::text("Hello from Atom Rendering Engine", 24.0, Rgba::rgb8(250, 250, 250)),
            UxNode::text("laid out, rasterized, and composited on the CPU",
                         14.0, Rgba::rgb8(161, 161, 170)),
        ],
    );
    let fb = render_uxi(&ui, 640, 360, Rgba::rgb8(9, 9, 11));
    std::fs::write("hello.bmp", fb.to_bmp(Rgba::rgb8(9, 9, 11))).unwrap();
}
```

`Framebuffer::to_bmp` needs no image crate â€” the BMP encoder is built in.
`Framebuffer::to_u32` produces `0x00RRGGBB` pixels ready for any OS blit
(`StretchDIBits`, softbuffer-style surfaces, etc.).

### Output gamma and the Renderer Customizer

Interactive `render_ui` surfaces automatically include the renderer-owned Design rail. Its
Gamma slider maps `0.00..1.00` control state to an output gamma of `0.50..2.50`; the neutral
default is `1.00` (slider value `0.25`). The orchestrator stores that value on the returned
`Framebuffer`. Both `to_u32` and `to_bmp` apply `channel.powf(1.0 / gamma)` after straight-alpha
flattening and before 8-bit quantization, so compositing, bloom, and raw RGBA remain unchanged.

Static renderers can opt in directly:

```rust
let mut fb = render_uxi(&ui, 640, 360, clear);
fb.set_output_gamma(2.2);
let pixels = fb.to_u32(clear);
```

### Transparent and translucent materials

Interactive Customizers default to `Legacy`, which injects no material. A frame with no explicit
app-authored material preserves the existing parallel lane render. Selecting a cookbook preset
attaches materials to app surfaces that authored a shadow or use the `Scroll` role; direct
`Style::transparency(...)` works on any box.
Because blur and refraction must read pixels painted earlier, the orchestrator inserts a full-frame
serial backdrop barrier for that frame. Frames without materials stay on the banded parallel path.

Apps can attach a material directly:

```rust
use pmre_kit::{transparency::MaterialPreset, Rgba, Style};

let glass = Style::col()
    .radius(18.0)
    .bg(Rgba::new(0.18, 0.72, 0.82, 0.52))
    .transparency(MaterialPreset::Water.material());
```

Backdrop crops are filtered as premultiplied RGBA, then converted back to PMRE's explicit
straight-alpha framebuffer boundary. That avoids dark blur fringes without silently changing the
public framebuffer representation. DPI scaling applies to every pixel-distance material field.
Rounded geometry and active scroll clips remain authoritative.

The live presets and exact unsupported boundary are listed in
[Transparency Cookbook Coverage](TRANSPARENCY_COOKBOOK.md). Exact dielectric Fresnel, Snell/TIR,
Henyey-Greenstein, cheap thickness translucency, and WBOIT accumulation are reusable math APIs;
their presence does not imply a 3D scene pass or path tracer.

Hosts that expose animation must periodically send `UiEvent::Tick(delta_seconds)` and request a
redraw while the Animation control is nonzero. The native Atom Vibe window and Win32 live example
do this at 30 Hz; static/headless renderers can leave the clock untouched.

---

## 3. Building UIs: `UxNode` + `Style`

A UI is a tree of intent with **no coordinates** â€” the layout solver derives every
position. Three node kinds:

| Node | What it is |
|---|---|
| `UxNode::Box { style, children }` | a styled flex container |
| `UxNode::Text { content, size, color }` | one plain text run (wraps on its own) |
| `UxNode::Rich { spans, align }` | inline flow: mixed bold/underline/color/size spans wrap **together** like an HTML paragraph |

`Style` is a builder:

```rust
Style::row()                       // main axis: Row or Column (Style::col())
    .w(Dim::Px(240.0))             // Auto | Px(f32) | Flex(weight) | Pct(0..100)
    .h(Dim::Auto)
    .pad(Edges::all(12.0))         // padding (also Edges::xy(x, y) / per-side struct)
    .margin(Edges::xy(0.0, 8.0))   // margin â€” outside the border box
    .gap(8.0)                      // space between children on the main axis
    .align(Align::Center)          // cross-axis: Start | Center | End | Stretch
    .justify(Justify::SpaceBetween)// main-axis: Start | Center | End | SpaceBetween
    .bg(Rgba::rgb8(30, 30, 36))
    .radius(10.0)                  // rounded corners
    .border(1.0, Rgba::rgb8(63, 63, 70))
    .shadow(0.0, 4.0, 14.0, Rgba::new(0.0, 0.0, 0.0, 0.35)) // dx, dy, blur, color
    .button(MY_ID)                 // interaction role (see Â§6)
```

Sizing semantics (reduced CSS flexbox):

- `Auto` â€” intrinsic content size (text measures with real font metrics and wraps to
  the available width).
- `Px(v)` â€” fixed border-box size; margins add outside.
- `Flex(w)` â€” `flex-basis: 0; flex-grow: w`: children share leftover main-axis space
  by weight.
- `Pct(p)` â€” percentage of the parent's content extent on that axis.

Rich text:

```rust
use pmre_kit::Span;
UxNode::rich(vec![
    Span::new("Deploy ", 14.0, text_color),
    Span::new("failed", 14.0, red).bold(),
    Span::new(" â€” see the ", 14.0, text_color),
    Span::new("logs", 14.0, blue).underline(),
])
```

All spans flow through one greedy word-wrapper and share a common baseline per line,
so mixed sizes/weights sit on the same line correctly.

---

## 4. Text and fonts

Two tiers, selected automatically at first use:

1. **Vector tier** â€” `pmre_kit::font` finds a system `.ttf` (Segoe UI â†’ Arial â†’
   Tahoma â†’ Calibri â†’ Verdana on Windows; DejaVu/Liberation on Linux; Arial/Helvetica
   on macOS), parses it, and rasterizes anti-aliased glyphs with a font-rs-style
   accumulation buffer. Glyph bitmaps are cached per `(glyph, quarter-px size)`.
2. **Bitmap tier** â€” the built-in 5Ã—7 pixel font when no font file exists (containers,
   bare CI images). Everything still renders.

Overrides: set `PMRE_FONT` / `PMRE_FONT_BOLD` to explicit `.ttf`/`.ttc` paths.

Useful APIs (`pmre_kit::text`):

- `advance(str, size) -> f32` / `advance_styled(str, size, bold)` â€” run width.
- `wrap(str, size, max_width) -> Vec<String>` â€” greedy word wrap, O(n).
- `v_metrics(size) -> (ascent, descent)` â€” for baseline math.
- `draw(surface, str, origin, size, color, clip)` â€” origin is the top of the ascent
  box; the baseline lands at `origin.y + ascent`.

Malformed font files degrade to the bitmap tier â€” parsing is fully bounds-checked and
glyph rasterization caps its bitmap size; it never panics or aborts.

---

## 5. The HTML/CSS front-end

`pmre_orchestrator::render_html(src, w, h, clear)` (or `pmre_kit::html::parse(src)`
for the tree) reduces an HTML fragment with **inline `style` attributes** onto the same
`UxNode` vocabulary. There is no selector engine and no external stylesheet â€” the box
model and property subset are the load-bearing core; the cascade is deliberately out of
scope.

Structure handled: comments, doctype, entities (`&amp;` `&#x2014;` â€¦), `<script>` /
`<style>` content skipping, void tags, malformed-input hardening (depth caps, no
quadratic scans).

| Kind | Supported |
|---|---|
| Block tags | `div p h1â€“h4 ul ol li hr section header footer main nav article` |
| Inline tags (coalesce into one flow) | `b strong i em u a span small code mark br` |
| Layout CSS | `display(flex\|block\|none)`, `flex-direction`, `flex`, `flex-grow`, `width`/`height` (px/%/auto), `padding`/`margin` (+ per-side, 1â€“4 value shorthand), `gap`, `align-items`, `justify-content` |
| Paint CSS | `background(-color)`, `border`, `border-radius`, `box-shadow`, `opacity` |
| Text CSS | `color`, `font-size`, `font-weight`, `text-align`, `text-decoration` |
| Colors | `#rgb #rgba #rrggbb #rrggbbaa`, `rgb()/rgba()`, `hsl()/hsla()`, ~45 named |

Notes that differ from a browser:

- Inline elements coalesce into a `Rich` flow **unless** the parent is a
  `display:flex` row â€” there, per CSS, each child is its own flex item and `gap`
  applies between them.
- `<a>` renders underlined + link-blue but is not clickable by itself; give a
  surrounding box an interaction role if you need clicks.
- No italics (no synthetic shear yet) â€” `i`/`em` currently render upright.

Run `cargo run -p pmre-orchestrator --example html` to see the subset exercised.

---

## 6. Interactive apps

The orchestrator owns an immediate-mode-flavored loop: your app holds domain state and
a `build(&UiState) -> UxNode` function; the engine owns `UiState` (hover, press,
focus, scroll positions) and replays it into your build function every event.

```rust
use pmre_orchestrator::{handle_event, render_ui, UiEvent, UiState};

const SAVE: u32 = 1;
const LIST: u32 = 2;

fn build(ui: &UiState) -> UxNode {
    let save_bg = if ui.is_pressed(SAVE) { pressed_col }
                  else if ui.is_hover(SAVE) { hover_col }
                  else { normal_col };
    UxNode::boxed(Style::col().gap(8.0), vec![
        UxNode::boxed(Style::row().button(SAVE).bg(save_bg) /* â€¦ */, vec![/* â€¦ */]),
        UxNode::boxed(Style::col().scroll(LIST).h(Dim::Flex(1.0)), rows()),
    ])
}

// event loop:
handle_event(&mut ui_state, &build, UiEvent::PointerMove(x, y));
if ui_state.take_click() == Some(SAVE) { /* domain logic */ }
let fb = render_ui(&build, &ui_state, clear);
```

Roles (`Style::interactive`, or the `button/toggle/scroll/input` shorthands):

- **Button** â€” `is_hover` / `is_pressed` for styling; `take_click()` fires once per
  completed press+release on the same widget.
- **Toggle** â€” engine flips `toggle_on(id)` on click.
- **Scroll** â€” the box clips and scrolls its children; wheel and scrollbar-thumb drag
  (with grab offset) are handled for you; offsets re-clamp automatically when content
  shrinks.
- **Input** â€” click to focus; feed `UiEvent::Char/Backspace/Enter`; read
  `input_text(id)`, `take_submit()`.

### DPI contract (important)

- `UiState.width/height` are **physical** pixels; `UiState.scale` is the device pixel
  ratio (`dpi / 96`).
- Layout solves in **logical** units (`width / scale`); painting multiplies back up,
  so glyphs rasterize at native resolution â€” never bitmap-stretched.
- **Feed pointer events in logical units** (divide OS mouse coordinates by `scale`).
- `examples/app.rs` shows the full Win32 wiring: `SetProcessDpiAwarenessContext`,
  `WM_DPICHANGED`, mouse capture, `TrackMouseEvent` for hover-clear on window leave.

### Post-processing

`render_ui_quality` / `render_uxi_quality` take a `Quality` tier: `Fast` (no post),
CPU / parallel / cache-tiled bloom, or GPU bloom (feature-gated; falls back to CPU).
For typical UI work use `Fast` â€” the tiers exist to exercise the lane/bus dispatch
work; run `--example sweep` and `--example bench` for the numbers.

---

## 7. Drawing primitives directly

Skip the UI layer entirely when you just need shapes:

```rust
use pmre_kit::{Affine, DrawCmd, Paint, Rgba, Shape, Vec2};
use pmre_orchestrator::{render, Scene};

let mut scene = Scene::new(640, 360, Rgba::rgb8(18, 18, 26));
scene.push(0.0, DrawCmd::new(
    Shape::RoundedRect { half: Vec2::new(120.0, 70.0), radius: 20.0 },
    Paint::Linear { from: Vec2::new(-120.0, -70.0), to: Vec2::new(120.0, 70.0),
                    c0: Rgba::rgb8(80, 140, 250), c1: Rgba::rgb8(175, 80, 230) },
    Affine::translate(320.0, 180.0),
));
let fb = render(&scene); // painter's algorithm by z
```

- Shapes are **SDF-defined in local space**; the `Affine` places them. Anti-aliasing
  is analytic â€” one smoothstep over the signed distance, no supersampling.
- `DrawCmd.soft` widens the AA band: `0.0` is a crisp edge, larger values give the
  smooth falloff used for drop shadows and glows.
- For arbitrary contours (stars, glyph-like blobs, donuts) use `pmre_kit::path`:
  `fill_cmds` / `stroke_cmds` with `MoveTo/LineTo/Quad/Cubic/Close`, nonzero winding.

---

## 8. Engine invariants (read before changing the kit)

1. **Zero dependencies.** `cargo tree` must show no external crates for the library
   targets. OS access is limited to `std::fs` (fonts) and the examples' raw FFI.
2. **Banded determinism.** `paint_boxes_banded` splits the frame into row bands, one
   thread each, writing disjoint slices â€” output must be **bit-identical** to the
   serial render and independent of thread count (a test enforces this). If you add
   any paint that can touch pixels outside its box rect (shadow bleed, glyph
   overshoot, new effects), extend `paint_y_extent` in `pmre-orchestrator/src/lib.rs`
   or lanes will skip work and seam.
3. **Kit/orchestrator split.** New mechanism (a shape, a coverage generator, a post
   pass) goes in the kit; anything choosing *when/what/in-which-order* goes in the
   orchestrator.
4. **Malformed input degrades, never panics.** The font parser and HTML parser are
   fuzz-minded: bounds-checked reads, recursion caps, no unbounded allocation.
   Keep new parsing code to that standard.
5. **Logical vs physical units.** Only the paint step (and `draw_scrollbars`) knows
   about `scale`. Layout, hit-testing, and events stay logical.

### Adding a new SDF shape
1. Add the variant to `Shape` (`pmre-kit/src/paint.rs`) with local-space fields.
2. Implement its distance in `raster::signed_distance` and its box in
   `Shape::local_bounds` / `is_degenerate`.
3. Done â€” AA, paints, clipping, transforms, and banding all compose automatically.

### Adding a CSS property
1. Parse it in `apply_css` (`pmre-kit/src/html.rs`) into `Style`/`Inherited`.
2. If it needs new intent, extend `Style` (+ builder) in `ux.rs`.
3. Consume it in `layout.rs` (measure/solve) or `cmds_for` (paint).
4. Add a parser test in `html.rs` and, if it paints, eyeball
   `cargo run --example html`.

---

## 9. Testing & benchmarking

```sh
cargo test --workspace                          # unit + determinism tests
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p pmre-orchestrator --example bench --release   # ms/frame per quality tier
cargo run -p pmre-orchestrator --example html --release    # writes html.bmp
cargo run -p pmre-orchestrator --example app --release     # live Win32 window
```

Headless examples write `.bmp` files to the working directory (viewable everywhere,
zero encoder dependencies). The live window prints frame time + fps in its title bar;
keys `1â€“9` switch post-processing tiers.
