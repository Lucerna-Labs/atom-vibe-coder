//! RUST_FULL_FILE_REWRITE
//! Native PMRE render of the Math Atoms Coder production surface.
//!
//! This example turns the operator diagram into a local, dependency-light artifact:
//! recipe-first controls, a spiderweb fabric graph, proof gates, and a live atom pane.

use std::fs;

use pmre_kit::{
    geom::{Affine, Vec2},
    paint::{DrawCmd, Paint, Rgba, Shape},
    raster, text, Framebuffer,
};

const W: u32 = 1440;
const H: u32 = 960;

const BG: Rgba = Rgba::new(0.964, 0.976, 0.973, 1.0);
const INK: Rgba = Rgba::new(0.025, 0.045, 0.050, 1.0);
const MUTED: Rgba = Rgba::new(0.340, 0.400, 0.400, 1.0);
const LINE: Rgba = Rgba::new(0.760, 0.820, 0.800, 1.0);
const TEAL: Rgba = Rgba::new(0.025, 0.560, 0.580, 1.0);
const ORANGE: Rgba = Rgba::new(0.840, 0.330, 0.200, 1.0);
const GOLD: Rgba = Rgba::new(0.830, 0.570, 0.020, 1.0);
const INDIGO: Rgba = Rgba::new(0.280, 0.330, 0.650, 1.0);
const WHITE: Rgba = Rgba::new(1.000, 1.000, 1.000, 1.0);

#[derive(Clone, Copy)]
struct Point {
    x: f32,
    y: f32,
}

impl Point {
    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn vec(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

fn color(r: u8, g: u8, b: u8) -> Rgba {
    Rgba::rgb8(r, g, b)
}

fn draw_cmd(shape: Shape, fill: Rgba, x: f32, y: f32) -> DrawCmd {
    DrawCmd::new(shape, Paint::Solid(fill), Affine::translate(x, y))
}

fn soft_cmd(shape: Shape, fill: Rgba, x: f32, y: f32, soft: f32) -> DrawCmd {
    DrawCmd {
        shape,
        paint: Paint::Solid(fill),
        transform: Affine::translate(x, y),
        soft,
    }
}

fn scan(fb: &mut Framebuffer, draw: DrawCmd) {
    raster::scan_convert(&draw, fb, None);
}

fn round_rect(fb: &mut Framebuffer, x: f32, y: f32, w: f32, h: f32, r: f32, fill: Rgba) {
    scan(
        fb,
        draw_cmd(
            Shape::RoundedRect {
                half: Vec2::new(w * 0.5, h * 0.5),
                radius: r,
            },
            fill,
            x + w * 0.5,
            y + h * 0.5,
        ),
    );
}

fn stroke_rect(fb: &mut Framebuffer, x: f32, y: f32, w: f32, h: f32, fill: Rgba) {
    round_rect(fb, x, y, w, 1.0, 0.5, fill);
    round_rect(fb, x, y + h - 1.0, w, 1.0, 0.5, fill);
    round_rect(fb, x, y, 1.0, h, 0.5, fill);
    round_rect(fb, x + w - 1.0, y, 1.0, h, 0.5, fill);
}

fn panel(fb: &mut Framebuffer, x: f32, y: f32, w: f32, h: f32) {
    scan(
        fb,
        soft_cmd(
            Shape::RoundedRect {
                half: Vec2::new(w * 0.5, h * 0.5),
                radius: 8.0,
            },
            Rgba::new(0.03, 0.05, 0.05, 0.10),
            x + w * 0.5,
            y + h * 0.5 + 5.0,
            8.0,
        ),
    );
    round_rect(fb, x, y, w, h, 8.0, WHITE);
    stroke_rect(fb, x, y, w, h, LINE.with_alpha(0.85));
}

fn text_at(fb: &mut Framebuffer, s: &str, x: f32, y: f32, size: f32, fill: Rgba, bold: bool) {
    text::draw_styled(fb, s, Vec2::new(x, y), size, fill, None, bold, false);
}

fn line(fb: &mut Framebuffer, a: Point, b: Point, width: f32, fill: Rgba) {
    scan(
        fb,
        DrawCmd::new(
            Shape::Line {
                a: a.vec(),
                b: b.vec(),
                width,
            },
            Paint::Solid(fill),
            Affine::IDENTITY,
        ),
    );
}

fn circle(fb: &mut Framebuffer, x: f32, y: f32, r: f32, fill: Rgba) {
    scan(fb, draw_cmd(Shape::Circle { radius: r }, fill, x, y));
}

fn curve(fb: &mut Framebuffer, p0: Point, p1: Point, p2: Point, p3: Point, fill: Rgba) {
    let mut prev = p0.vec();
    for i in 1..=32 {
        let t = i as f32 / 32.0;
        let mt = 1.0 - t;
        let p = p0.vec().scale(mt * mt * mt)
            + p1.vec().scale(3.0 * mt * mt * t)
            + p2.vec().scale(3.0 * mt * t * t)
            + p3.vec().scale(t * t * t);
        line(
            fb,
            Point::new(prev.x, prev.y),
            Point::new(p.x, p.y),
            1.6,
            fill,
        );
        prev = p;
    }
}

fn orbit(fb: &mut Framebuffer, cx: f32, cy: f32, rx: f32, ry: f32, angle: f32, fill: Rgba) {
    let mut prev: Option<Point> = None;
    let ca = angle.cos();
    let sa = angle.sin();
    for i in 0..=96 {
        let t = i as f32 / 96.0 * std::f32::consts::TAU;
        let x = rx * t.cos();
        let y = ry * t.sin();
        let p = Point::new(cx + x * ca - y * sa, cy + x * sa + y * ca);
        if let Some(q) = prev {
            line(fb, q, p, 1.1, fill);
        }
        prev = Some(p);
    }
}

fn brand(fb: &mut Framebuffer) {
    round_rect(fb, 18.0, 22.0, 54.0, 61.0, 8.0, WHITE);
    stroke_rect(fb, 18.0, 22.0, 54.0, 61.0, LINE);
    circle(fb, 36.0, 49.0, 6.0, TEAL);
    circle(fb, 55.0, 56.0, 6.0, ORANGE);
    circle(fb, 47.0, 68.0, 6.0, GOLD);
    text_at(fb, "Math Atoms Coder", 86.0, 34.0, 25.0, INK, true);
    text_at(
        fb,
        "Recipe store. Loop harness. Artifact proof.",
        86.0,
        68.0,
        13.0,
        MUTED,
        false,
    );
}

fn metric(fb: &mut Framebuffer, x: f32, value: &str, label: &str) {
    round_rect(fb, x, 22.0, 74.0, 69.0, 7.0, WHITE);
    stroke_rect(fb, x, 22.0, 74.0, 69.0, LINE);
    text_at(fb, value, x + 46.0, 40.0, 19.0, INK, true);
    text_at(fb, label, x + 24.0, 70.0, 9.0, MUTED, false);
}

fn left_rail(fb: &mut Framebuffer) {
    panel(fb, 18.0, 110.0, 364.0, 824.0);
    text_at(fb, "INTENT", 32.0, 130.0, 10.0, MUTED, true);
    text_at(fb, "Build Command", 32.0, 147.0, 16.0, INK, true);
    round_rect(fb, 328.0, 125.0, 38.0, 38.0, 8.0, color(248, 252, 251));
    stroke_rect(fb, 328.0, 125.0, 38.0, 38.0, LINE);
    text_at(fb, "loop", 337.0, 139.0, 10.0, MUTED, false);

    round_rect(fb, 33.0, 176.0, 334.0, 123.0, 6.0, color(254, 255, 255));
    stroke_rect(fb, 33.0, 176.0, 334.0, 123.0, LINE);
    text_at(
        fb,
        "Build a tiny local app with an atom",
        46.0,
        196.0,
        16.0,
        INK,
        false,
    );
    text_at(
        fb,
        "renderer, a self-correcting proof loop, a",
        46.0,
        220.0,
        16.0,
        INK,
        false,
    );
    text_at(
        fb,
        "recipe-first store, and a live artifact pane.",
        46.0,
        244.0,
        16.0,
        INK,
        false,
    );

    round_rect(fb, 33.0, 313.0, 164.0, 41.0, 7.0, color(5, 10, 9));
    text_at(fb, "Run Loop", 75.0, 329.0, 15.0, WHITE, true);
    round_rect(fb, 204.0, 313.0, 163.0, 41.0, 7.0, WHITE);
    stroke_rect(fb, 204.0, 313.0, 163.0, 41.0, LINE);
    text_at(fb, "Capture", 250.0, 329.0, 15.0, TEAL, false);
    line(
        fb,
        Point::new(33.0, 367.0),
        Point::new(367.0, 367.0),
        1.0,
        LINE,
    );

    text_at(fb, "HOOKS", 32.0, 385.0, 10.0, MUTED, true);
    text_at(fb, "Anti Drift Gate", 32.0, 402.0, 16.0, INK, true);
    hook(
        fb,
        33.0,
        427.0,
        "Recipe first",
        "local atoms before generic code",
        "L3",
    );
    hook(
        fb,
        33.0,
        493.0,
        "Proof required",
        "no done state without current evidence",
        "L1",
    );
    hook(
        fb,
        33.0,
        559.0,
        "Artifact visible",
        "preview must reflect the run",
        "L2",
    );
    hook(
        fb,
        33.0,
        625.0,
        "Fail closed",
        "unsupported paths become blockers",
        "L3",
    );

    line(
        fb,
        Point::new(33.0, 697.0),
        Point::new(367.0, 697.0),
        1.0,
        LINE,
    );
    text_at(fb, "STORE", 32.0, 717.0, 10.0, MUTED, true);
    text_at(fb, "Proven Recipes", 32.0, 735.0, 16.0, INK, true);
    round_rect(fb, 33.0, 758.0, 334.0, 41.0, 7.0, color(251, 253, 253));
    stroke_rect(fb, 33.0, 758.0, 334.0, 41.0, LINE);
    text_at(fb, "Search atoms", 70.0, 774.0, 16.0, MUTED, false);
    recipe(
        fb,
        33.0,
        809.0,
        "Atom Renderer Bootstrap",
        "L2 / proven / renderer",
        true,
    );
    recipe(
        fb,
        33.0,
        875.0,
        "Atom 3D Scene Recipe",
        "L2 / draft / renderer",
        false,
    );
}

fn hook(fb: &mut Framebuffer, x: f32, y: f32, title: &str, body: &str, lane: &str) {
    round_rect(fb, x, y, 334.0, 58.0, 7.0, color(254, 255, 255));
    stroke_rect(fb, x, y, 334.0, 58.0, LINE);
    text_at(fb, "ok", x + 13.0, y + 20.0, 12.0, color(13, 140, 62), true);
    text_at(fb, title, x + 46.0, y + 13.0, 14.0, INK, true);
    text_at(fb, body, x + 46.0, y + 36.0, 11.0, MUTED, false);
    round_rect(fb, x + 294.0, y + 16.0, 30.0, 27.0, 13.0, WHITE);
    stroke_rect(fb, x + 294.0, y + 16.0, 30.0, 27.0, LINE);
    text_at(fb, lane, x + 302.0, y + 23.0, 12.0, MUTED, false);
}

fn recipe(fb: &mut Framebuffer, x: f32, y: f32, title: &str, body: &str, selected: bool) {
    let fill = if selected {
        color(245, 253, 253)
    } else {
        WHITE
    };
    let border = if selected { TEAL } else { LINE };
    round_rect(fb, x, y, 317.0, 55.0, 7.0, fill);
    stroke_rect(fb, x, y, 317.0, 55.0, border);
    round_rect(
        fb,
        x + 12.0,
        y + 13.0,
        30.0,
        30.0,
        6.0,
        color(232, 243, 241),
    );
    text_at(fb, "box", x + 18.0, y + 22.0, 8.0, MUTED, true);
    text_at(fb, title, x + 52.0, y + 13.0, 13.0, color(46, 82, 82), true);
    text_at(fb, body, x + 52.0, y + 33.0, 11.0, MUTED, false);
}

fn center_stage(fb: &mut Framebuffer) {
    panel(fb, 398.0, 110.0, 570.0, 824.0);
    text_at(fb, "FABRIC", 412.0, 130.0, 10.0, MUTED, true);
    text_at(fb, "Spiderweb Build Layer", 412.0, 147.0, 16.0, INK, true);
    for (i, label) in ["L0", "L1", "L2", "L3"].iter().enumerate() {
        let x = 800.0 + i as f32 * 40.0;
        round_rect(fb, x, 129.0, 32.0, 29.0, 14.0, color(246, 250, 249));
        text_at(fb, label, x + 9.0, 137.0, 12.0, INK, true);
    }
    fabric_graph(fb);
    step_cards(fb);
}

fn fabric_graph(fb: &mut Framebuffer) {
    let x = 413.0;
    let y = 184.0;
    let w = 540.0;
    let h = 489.0;
    round_rect(fb, x, y, w, h, 7.0, color(251, 253, 253));
    stroke_rect(fb, x, y, w, h, LINE);
    for i in 1..16 {
        let gx = x + i as f32 * (w / 16.0);
        line(
            fb,
            Point::new(gx, y),
            Point::new(gx, y + h),
            1.0,
            color(219, 230, 227),
        );
    }
    for i in 1..14 {
        let gy = y + i as f32 * (h / 14.0);
        line(
            fb,
            Point::new(x, gy),
            Point::new(x + w, gy),
            1.0,
            color(219, 230, 227),
        );
    }

    for (lane, yy) in [
        ("L0 transport", 262.0),
        ("L1 message", 371.0),
        ("L2 flow", 488.0),
        ("L3 orchestration", 596.0),
    ] {
        line(
            fb,
            Point::new(x, yy),
            Point::new(x + w, yy),
            1.0,
            color(187, 202, 197).with_alpha(0.55),
        );
        text_at(fb, lane, x + 12.0, yy - 17.0, 10.0, MUTED, true);
    }

    let proof = Point::new(484.0, 393.0);
    let renderer = Point::new(484.0, 479.0);
    let atom3d = Point::new(618.0, 467.0);
    let self_fix = Point::new(484.0, 598.0);
    let recipe_first = Point::new(618.0, 597.0);
    let anti_drift = Point::new(752.0, 597.0);
    let db = Point::new(751.0, 352.0);
    let gpu = Point::new(885.0, 243.0);
    let side = Point::new(884.0, 488.0);

    for target in [db, gpu, side] {
        curve(
            fb,
            proof,
            Point::new(610.0, 382.0),
            Point::new(736.0, 230.0),
            target,
            color(122, 140, 135).with_alpha(0.42),
        );
        curve(
            fb,
            renderer,
            Point::new(610.0, 468.0),
            Point::new(715.0, 450.0),
            target,
            color(92, 110, 105).with_alpha(0.38),
        );
    }
    curve(
        fb,
        atom3d,
        Point::new(692.0, 470.0),
        Point::new(724.0, 350.0),
        gpu,
        color(87, 107, 102).with_alpha(0.36),
    );
    curve(
        fb,
        recipe_first,
        Point::new(675.0, 608.0),
        Point::new(720.0, 502.0),
        db,
        color(87, 107, 102).with_alpha(0.36),
    );
    curve(
        fb,
        self_fix,
        Point::new(572.0, 596.0),
        Point::new(640.0, 630.0),
        anti_drift,
        color(87, 107, 102).with_alpha(0.36),
    );
    line(
        fb,
        renderer,
        self_fix,
        1.8,
        color(56, 87, 82).with_alpha(0.65),
    );
    for (yy1, yy2) in [(263.0, 371.0), (371.0, 488.0), (488.0, 596.0)] {
        curve(
            fb,
            Point::new(885.0, yy1),
            Point::new(910.0, yy1 + 42.0),
            Point::new(910.0, yy2 - 42.0),
            Point::new(890.0, yy2),
            color(232, 71, 41).with_alpha(0.72),
        );
    }

    graph_node(fb, proof, "L1", "Proof Capture", ORANGE, 13.0);
    graph_node(fb, renderer, "L2", "Atom Renderer", GOLD, 20.0);
    graph_node(fb, atom3d, "L2", "Atom 3D", GOLD, 13.0);
    graph_node(fb, self_fix, "L3", "Self Correction", INDIGO, 13.0);
    graph_node(fb, recipe_first, "L3", "Recipe First", INDIGO, 13.0);
    graph_node(fb, anti_drift, "L3", "Anti Drift", INDIGO, 13.0);
    graph_node(fb, db, "L1", "Hybrid DB", ORANGE, 13.0);
    graph_node(fb, gpu, "L0", "GPU Compatible", TEAL, 13.0);
    graph_node(fb, side, "L2", "Side Artifact", GOLD, 13.0);
    for p in [
        Point::new(913.0, 263.0),
        Point::new(890.0, 371.0),
        Point::new(913.0, 488.0),
        Point::new(890.0, 597.0),
    ] {
        circle(fb, p.x, p.y, 5.8, ORANGE);
    }
}

fn graph_node(fb: &mut Framebuffer, p: Point, lane: &str, label: &str, fill: Rgba, r: f32) {
    circle(fb, p.x, p.y, r, fill);
    text_at(fb, lane, p.x - 8.0, p.y - 7.0, 11.0, WHITE, true);
    text_at(fb, label, p.x - 36.0, p.y + r + 11.0, 11.0, INK, true);
}

fn step_cards(fb: &mut Framebuffer) {
    let cards = [
        (
            "Intent Atom",
            "Normalize the request into atoms, bonds, and molecules.",
        ),
        (
            "Recipe Retrieval",
            "Rank proven local atoms and reject generic fallback.",
        ),
        (
            "Molecule Build",
            "Compose renderer, store, hooks, and harness routes.",
        ),
        (
            "Proof Run",
            "Run checks, surface failures, and capture evidence.",
        ),
        (
            "Self Critique",
            "Find drift, missing proof, stale state, and weak atoms.",
        ),
        (
            "Patch Reaction",
            "Apply the smallest correction and rebind the graph.",
        ),
        (
            "Artifact Preview",
            "Refresh the side artifact as a live proof surface.",
        ),
        (
            "Store Learning",
            "Capture the passing route as a reusable recipe.",
        ),
    ];
    for (i, (title, body)) in cards.iter().enumerate() {
        let col = i % 4;
        let row = i / 4;
        let x = 414.0 + col as f32 * 137.0;
        let y = 686.0 + row as f32 * 122.0;
        round_rect(fb, x, y, 128.0, 112.0, 7.0, WHITE);
        stroke_rect(fb, x, y, 128.0, 112.0, LINE);
        text_at(fb, title, x + 10.0, y + 13.0, 13.0, INK, true);
        for (line_no, wrapped) in text::wrap(body, 11.0, 102.0).iter().take(4).enumerate() {
            text_at(
                fb,
                wrapped,
                x + 10.0,
                y + 40.0 + line_no as f32 * 17.0,
                11.0,
                MUTED,
                false,
            );
        }
        circle(fb, x + 119.0, y + 18.0, 4.0, color(158, 179, 173));
    }
}

fn right_rail(fb: &mut Framebuffer) {
    panel(fb, 984.0, 110.0, 423.0, 824.0);
    tab(fb, 996.0, "view", "Artifact", true);
    tab(fb, 1130.0, "box", "Atom", false);
    tab(fb, 1264.0, "code", "Recipe", false);
    text_at(
        fb,
        "Atom Renderer Bootstrap",
        1001.0,
        196.0,
        20.0,
        INK,
        true,
    );
    text_at(
        fb,
        "Tiny visual surface every generated app can mount.",
        1001.0,
        230.0,
        16.0,
        color(43, 71, 71),
        false,
    );
    atom_renderer(fb, 1001.0, 276.0);
    status_box(fb, 1001.0, 776.0, "STATUS", "proven");
    status_box(fb, 1133.0, 776.0, "PROOFS", "3");
    status_box(fb, 1265.0, 776.0, "BONDS", "2");
    text_at(
        fb,
        "DS4 gate: Q2/Q3/Q4/Q5/Q6/Q8 smallest passing recipe",
        1001.0,
        900.0,
        11.0,
        MUTED,
        false,
    );
    text_at(
        fb,
        "from full-precision evidence",
        1001.0,
        917.0,
        11.0,
        MUTED,
        false,
    );
}

fn tab(fb: &mut Framebuffer, x: f32, icon: &str, label: &str, selected: bool) {
    round_rect(fb, x, 121.0, 129.0, 40.0, 7.0, WHITE);
    stroke_rect(
        fb,
        x,
        121.0,
        129.0,
        40.0,
        if selected { TEAL } else { LINE },
    );
    text_at(
        fb,
        icon,
        x + 22.0,
        135.0,
        11.0,
        if selected { INK } else { MUTED },
        true,
    );
    text_at(
        fb,
        label,
        x + 52.0,
        134.0,
        16.0,
        if selected { INK } else { MUTED },
        false,
    );
}

fn atom_renderer(fb: &mut Framebuffer, x: f32, y: f32) {
    round_rect(fb, x, y, 389.0, 488.0, 7.0, color(254, 255, 255));
    stroke_rect(fb, x, y, 389.0, 488.0, LINE);
    for i in 1..13 {
        let gx = x + i as f32 * (389.0 / 13.0);
        line(
            fb,
            Point::new(gx, y),
            Point::new(gx, y + 488.0),
            1.0,
            color(214, 230, 232).with_alpha(0.72),
        );
    }
    for i in 1..16 {
        let gy = y + i as f32 * (488.0 / 16.0);
        line(
            fb,
            Point::new(x, gy),
            Point::new(x + 389.0, gy),
            1.0,
            color(214, 230, 232).with_alpha(0.72),
        );
    }
    let cx = x + 195.0;
    let cy = y + 244.0;
    orbit(
        fb,
        cx,
        cy,
        118.0,
        44.0,
        0.23,
        color(20, 158, 171).with_alpha(0.55),
    );
    orbit(
        fb,
        cx,
        cy,
        126.0,
        47.0,
        -0.44,
        color(20, 158, 171).with_alpha(0.45),
    );
    circle(fb, cx, cy, 38.0, color(8, 18, 18));
    text_at(fb, "L2", cx - 11.0, cy - 9.0, 17.0, WHITE, true);
    circle(fb, cx - 60.0, cy - 35.0, 9.0, TEAL);
    circle(fb, cx + 70.0, cy + 43.0, 9.0, ORANGE);
    circle(fb, cx + 7.0, cy - 127.0, 9.0, GOLD);
}

fn status_box(fb: &mut Framebuffer, x: f32, y: f32, label: &str, value: &str) {
    round_rect(fb, x, y, 124.0, 111.0, 7.0, WHITE);
    stroke_rect(fb, x, y, 124.0, 111.0, LINE);
    text_at(fb, label, x + 12.0, y + 15.0, 13.0, MUTED, true);
    text_at(fb, value, x + 12.0, y + 42.0, 18.0, INK, true);
}

fn main() -> std::io::Result<()> {
    let mut fb = Framebuffer::new(W, H, BG);
    brand(&mut fb);
    metric(&mut fb, 1169.0, "9", "ATOMS");
    metric(&mut fb, 1251.0, "3", "PROOFS");
    metric(&mut fb, 1333.0, "1", "DRIFT");
    left_rail(&mut fb);
    center_stage(&mut fb);
    right_rail(&mut fb);

    fs::write("math_atoms_coder.bmp", fb.to_bmp(BG))?;
    println!("wrote math_atoms_coder.bmp ({}x{})", W, H);
    Ok(())
}
