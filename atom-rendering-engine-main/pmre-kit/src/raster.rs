//! Rasterization primitives — the cheap, exact-AA coverage generator.
//!
//! `signed_distance` is the `compare` atom (a distance). `coverage` is the graphics
//! `smoothstep` primitive (analytic anti-aliasing). `scan_convert` wires
//! `scan` (pixel grid) · `project` (inverse transform) · `compare` (SDF) · `scale` (AA band)
//! · `combine` (alpha-over) into one shape → pixels operation. Mechanism only: it never
//! decides draw order, clipping, or which generator to use — that is the orchestrator's job.

use crate::framebuffer::Surface;
use crate::geom::Vec2;
use crate::paint::{Bounds, DrawCmd, Shape};

/// Signed distance to the shape boundary in its local space: negative inside, positive outside.
pub fn signed_distance(shape: &Shape, p: Vec2) -> f32 {
    match *shape {
        Shape::Rect { half } => sd_box(p, half),
        Shape::RoundedRect { half, radius } => sd_box(p, half - Vec2::new(radius, radius)) - radius,
        Shape::Circle { radius } => p.length() - radius,
        Shape::Line { a, b, width } => sd_segment(p, a, b) - width * 0.5,
    }
}

fn sd_box(p: Vec2, half: Vec2) -> f32 {
    let d = p.abs() - half;
    d.max_scalar(0.0).length() + d.x.max(d.y).min(0.0)
}

fn sd_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = (pa.dot(ba) / ba.dot(ba)).clamp(0.0, 1.0);
    (pa - ba.scale(h)).length()
}

/// Analytic coverage from a signed distance: 1 inside, 0 outside, Hermite band of half-width `aa`.
pub fn coverage(dist: f32, aa: f32) -> f32 {
    1.0 - smoothstep(-aa, aa, dist)
}

/// Hermite smoothstep: 0 below `edge0`, 1 above `edge1`, C¹-continuous in between.
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Scan-convert one command into `surf` using the SDF coverage generator. Generic over the
/// pixel sink so it can target a whole framebuffer or one row-band of it (see `Surface`).
/// Pure translations (every box the layout solver emits) take a fast path that replaces
/// the per-pixel inverse-matrix multiply with a subtraction.
pub fn scan_convert<S: Surface>(cmd: &DrawCmd, surf: &mut S, clip: Option<Bounds>) {
    // One device pixel measured in local units — the width of the anti-aliasing band.
    // A `soft` command widens the band for smooth falloff (shadows, glows).
    let aa = (1.0 / cmd.transform.scale_factor().max(1e-6))
        .max(1e-4)
        .max(cmd.soft);
    let bounds = device_bounds(cmd, surf.width(), surf.height(), surf.row_range(), clip);
    let t = cmd.transform;
    if t.a == 1.0 && t.d == 1.0 && t.b == 0.0 && t.c == 0.0 {
        convert_rows(cmd, surf, bounds, aa, |x, y| Vec2::new(x - t.e, y - t.f));
    } else {
        let inv = t.inverse();
        convert_rows(cmd, surf, bounds, aa, move |x, y| {
            inv.apply(Vec2::new(x, y))
        });
    }
}

fn convert_rows<S: Surface, M: Fn(f32, f32) -> Vec2>(
    cmd: &DrawCmd,
    surf: &mut S,
    (x0, y0, x1, y1): (u32, u32, u32, u32),
    aa: f32,
    to_local: M,
) {
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        for x in x0..x1 {
            let local = to_local(x as f32 + 0.5, py);
            let d = signed_distance(&cmd.shape, local);
            let cov = coverage(d, aa);
            if cov > 0.0 {
                // Sample the paint at the shape-local point (gradients move with the shape).
                let col = cmd.paint.sample(local);
                surf.blend_over(x, y, col.with_alpha(col.a * cov));
            }
        }
    }
}

/// Device-space pixel bounds the command can touch (its transformed, padded local box),
/// intersected with the optional clip rectangle and the surface's accepted row range `rows`.
fn device_bounds(
    cmd: &DrawCmd,
    w: u32,
    h: u32,
    rows: (u32, u32),
    clip: Option<Bounds>,
) -> (u32, u32, u32, u32) {
    let lb = cmd.shape.local_bounds().pad(2.0 + cmd.soft);
    let corners = [
        Vec2::new(lb.min.x, lb.min.y),
        Vec2::new(lb.max.x, lb.min.y),
        Vec2::new(lb.min.x, lb.max.y),
        Vec2::new(lb.max.x, lb.max.y),
    ];
    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for c in corners {
        let d = cmd.transform.apply(c);
        min = Vec2::new(min.x.min(d.x), min.y.min(d.y));
        max = Vec2::new(max.x.max(d.x), max.y.max(d.y));
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (min.x, min.y, max.x, max.y);
    if let Some(c) = clip {
        minx = minx.max(c.min.x);
        miny = miny.max(c.min.y);
        maxx = maxx.min(c.max.x);
        maxy = maxy.min(c.max.y);
    }
    let (rlo, rhi) = rows;
    let x0 = minx.floor().max(0.0) as u32;
    let y0 = (miny.floor().max(0.0) as u32).max(rlo);
    let x1 = (maxx.ceil().max(0.0) as u32).min(w);
    let y1 = (maxy.ceil().max(0.0) as u32).min(h).min(rhi);
    (x0, y0, x1, y1)
}
