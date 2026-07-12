#![forbid(unsafe_code)]

//! Screen-space material mechanism for PMRE.
//!
//! The orchestrator decides painter order and when a backdrop barrier is required. This crate
//! performs one barrier: capture the already-painted backdrop, filter it in premultiplied alpha,
//! refract/absorb/scatter it, then composite the material inside a clipped rounded rectangle.

use pmre_kit::{
    framebuffer::Framebuffer,
    geom::{Affine, Vec2 as KitVec2},
    paint::{Bounds, DrawCmd, Paint, Rgba, Shape},
    raster,
    ux::Shadow,
};
pub use pmre_transparency_core::*;

#[derive(Clone, Copy, Debug)]
pub struct MaterialBox {
    pub rect: Bounds,
    pub radius: f32,
    pub clip: Option<Bounds>,
    pub tint: Option<Rgba>,
    pub border: Option<(f32, Rgba)>,
    pub shadow: Option<Shadow>,
    pub material: TransparencyMaterial,
}

pub fn paint_material_box(frame: &mut Framebuffer, request: MaterialBox) {
    let material = request.material.sanitized();
    let width = request.rect.max.x - request.rect.min.x;
    let height = request.rect.max.y - request.rect.min.y;
    if width <= 0.0 || height <= 0.0 || material.strength <= 0.0 {
        return;
    }

    let scale = material.pixel_scale;
    let blur = (material.blur_radius_px.max(material.roughness * 16.0) * scale).min(96.0);
    let optical = (material.ior - 1.0).max(0.0) * 2.0;
    let pad = blur
        + material.refraction_px * scale * optical
        + material.dispersion_px * scale
        + material.distortion_px * scale
        + 3.0;
    let sample_bounds = intersect_bounds(request.rect.pad(pad), request.clip);
    let captured = Snapshot::capture(frame, sample_bounds);
    let filtered_storage = (blur >= 0.5).then(|| captured.blur(blur.round() as usize));
    let filtered = filtered_storage.as_ref().unwrap_or(&captured);

    // The capture happens before this material's shadow, preventing the pane from sampling
    // its own shadow. The shadow still lands beneath the material in painter order.
    paint_shadow(frame, request);
    composite_material(frame, request, material, &captured, filtered);
}

fn paint_shadow(frame: &mut Framebuffer, request: MaterialBox) {
    let Some(shadow) = request.shadow else {
        return;
    };
    let (center, half) = center_half(request.rect);
    if half.x <= 0.0 || half.y <= 0.0 {
        return;
    }
    let radius = request.radius.min(half.x).min(half.y).max(0.0);
    let command = DrawCmd {
        shape: Shape::RoundedRect { half, radius },
        paint: Paint::Solid(shadow.color),
        transform: Affine::translate(center.x + shadow.dx, center.y + shadow.dy),
        soft: shadow.blur.max(0.5),
    };
    raster::scan_convert(&command, frame, request.clip);
}

fn composite_material(
    frame: &mut Framebuffer,
    request: MaterialBox,
    material: TransparencyMaterial,
    original: &Snapshot,
    filtered: &Snapshot,
) {
    let (center, half) = center_half(request.rect);
    let radius = request.radius.min(half.x).min(half.y).max(0.0);
    let shape = Shape::RoundedRect { half, radius };
    let (x0, y0, x1, y1) = output_bounds(frame, request.rect.pad(2.0), request.clip);
    let tint = rgba_rgb(request.tint.unwrap_or(Rgba::new(1.0, 1.0, 1.0, 0.0)));
    let tint_alpha = request
        .tint
        .map(|value| value.a)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let border = request.border.map(|(width, color)| (width.max(0.0), color));
    let scale = material.pixel_scale;
    let rim_width = material.rim_width_px * scale;
    let refraction = material.refraction_px * scale;
    let dispersion = material.dispersion_px * scale;
    let distortion = material.distortion_px * scale;

    for y in y0..y1 {
        for x in x0..x1 {
            let point = KitVec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let local = point - center;
            let distance = raster::signed_distance(&shape, local);
            let mask = raster::coverage(distance, 1.0);
            if mask <= 0.0 {
                continue;
            }

            let normal = pseudo_normal(local, half);
            let edge = edge_factor(distance, rim_width.max(1.0));
            let cos_incident = (1.0 - edge * 0.88).clamp(0.05, 1.0);
            let optical = (material.ior - 1.0).max(0.0) * 2.0;
            let wave_x = (point.x * 0.041 + point.y * 0.019 + material.phase * 2.3).sin();
            let wave_y = (point.y * 0.037 - point.x * 0.013 + material.phase * 1.7).cos();
            let offset_x = normal.x * refraction * optical + wave_x * distortion;
            let offset_y = normal.y * refraction * optical + wave_y * distortion;
            let dispersion_x = normal.x * dispersion;
            let dispersion_y = normal.y * dispersion;

            let red_sample = filtered.sample(
                point.x + offset_x + dispersion_x,
                point.y + offset_y + dispersion_y,
            );
            let green_sample = filtered.sample(point.x + offset_x, point.y + offset_y);
            let blue_sample = filtered.sample(
                point.x + offset_x - dispersion_x,
                point.y + offset_y - dispersion_y,
            );
            let red = red_sample.to_straight().rgb.r;
            let green = green_sample.to_straight().rgb.g;
            let blue = blue_sample.to_straight().rgb.b;
            let sample_alpha = red_sample.a.max(green_sample.a).max(blue_sample.a);
            let mut color = Rgb::new(red, green, blue).clamp01();

            let path = if material.thin_walled {
                material.thickness * 0.25
            } else {
                material.thickness / cos_incident.max(0.15)
            };
            color = color.multiply(transmission_color(
                material.transmission_color,
                material.transmission_distance,
                path,
            ));
            color = color.mix(
                tint,
                (tint_alpha * (0.16 + material.scatter * 0.28)).clamp(0.0, 0.65),
            );

            if material.scatter > 0.0 {
                let haze = tint.mix(Rgb::new(1.0, 1.0, 1.0), 0.22);
                color = color.mix(haze, material.scatter * (0.20 + edge * 0.16));
            }
            if material.thin_film_nm > 0.0 {
                let film = thin_film_rgb(cos_incident, material.film_ior, material.thin_film_nm);
                color = color.mix(film, 0.28 + edge * 0.32);
            }

            let fresnel = fresnel_schlick(cos_incident, IorPair::new(1.0, material.ior));
            let rim = edge_factor(distance, rim_width);
            if rim > 0.0 {
                let rim_color = border
                    .map(|(_, color)| rgba_rgb(color))
                    .unwrap_or_else(|| tint.mix(Rgb::new(1.0, 1.0, 1.0), 0.62));
                color = color.mix(rim_color, rim * (0.30 + fresnel * 0.70));
            }

            let content_alpha = sample_alpha + tint_alpha * (1.0 - sample_alpha);
            let mut coverage = mask * material.strength * content_alpha;
            if let Some((width, border_color)) = border {
                let border_coverage =
                    edge_factor(distance, width) * mask * border_color.a * material.strength;
                color = color.mix(rgba_rgb(border_color), border_coverage);
                coverage = coverage.max(border_coverage);
            }

            // Start from the exact legacy decoration, but fade the filled portion of this box's
            // own shadow toward the pre-shadow capture as optical strength rises. This keeps the
            // Glass=0 boundary continuous and prevents strong glass from sampling itself.
            let pre_shadow = original.sample(point.x, point.y);
            let shadowed = rgba_premul(frame.pixel(x, y));
            let under = premul_mix(shadowed, pre_shadow, material.strength);
            let dst = legacy_base(under, request, local, half, radius);
            let src = StraightRgba::new(color.clamp01(), coverage.clamp(0.0, 1.0)).premultiply();
            let output = match material.blend {
                BlendMode::Over => over(dst, src),
                BlendMode::Additive => additive(dst, src),
                BlendMode::MultiplyTint => multiply_tint(dst, color, coverage),
                BlendMode::SoftAdd => soft_add(dst, src),
                BlendMode::Cutout => {
                    if material.strength >= 0.5 {
                        over(
                            dst,
                            StraightRgba::new(color, mask * content_alpha).premultiply(),
                        )
                    } else {
                        dst
                    }
                }
                BlendMode::Dither => {
                    if material.strength >= ordered_dither_4x4(x, y) {
                        over(
                            dst,
                            StraightRgba::new(color, mask * content_alpha).premultiply(),
                        )
                    } else {
                        dst
                    }
                }
            };
            frame.set_pixel(x, y, premul_rgba(output));
        }
    }
}

fn legacy_base(
    mut dst: PremulRgba,
    request: MaterialBox,
    local: KitVec2,
    half: KitVec2,
    radius: f32,
) -> PremulRgba {
    let outer = raster::coverage(
        raster::signed_distance(&Shape::RoundedRect { half, radius }, local),
        1.0,
    );
    match request.border {
        Some((width, color)) => {
            dst = over_color(dst, color, outer);
            if let Some(background) = request.tint {
                let width = width.max(0.0);
                let inner_half = KitVec2::new((half.x - width).max(0.0), (half.y - width).max(0.0));
                if inner_half.x > 0.0 && inner_half.y > 0.0 {
                    let inner = raster::coverage(
                        raster::signed_distance(
                            &Shape::RoundedRect {
                                half: inner_half,
                                radius: (radius - width).max(0.0),
                            },
                            local,
                        ),
                        1.0,
                    );
                    dst = over_color(dst, background, inner);
                }
            }
        }
        None => {
            if let Some(background) = request.tint {
                dst = over_color(dst, background, outer);
            }
        }
    }
    dst
}

fn over_color(dst: PremulRgba, color: Rgba, coverage: f32) -> PremulRgba {
    over(
        dst,
        StraightRgba::new(rgba_rgb(color), color.a * coverage.clamp(0.0, 1.0)).premultiply(),
    )
}

fn center_half(rect: Bounds) -> (KitVec2, KitVec2) {
    (
        KitVec2::new(
            (rect.min.x + rect.max.x) * 0.5,
            (rect.min.y + rect.max.y) * 0.5,
        ),
        KitVec2::new(
            (rect.max.x - rect.min.x) * 0.5,
            (rect.max.y - rect.min.y) * 0.5,
        ),
    )
}

fn pseudo_normal(local: KitVec2, half: KitVec2) -> KitVec2 {
    let x = local.x / half.x.max(1.0);
    let y = local.y / half.y.max(1.0);
    let length = (x * x + y * y).sqrt();
    if length <= 1.0e-5 {
        KitVec2::new(0.0, 0.0)
    } else {
        KitVec2::new(x / length, y / length)
    }
}

fn edge_factor(distance: f32, width: f32) -> f32 {
    if width <= 0.0 {
        0.0
    } else {
        (1.0 - (-distance / width).clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }
}

fn intersect_bounds(bounds: Bounds, clip: Option<Bounds>) -> Bounds {
    if let Some(clip) = clip {
        Bounds {
            min: KitVec2::new(bounds.min.x.max(clip.min.x), bounds.min.y.max(clip.min.y)),
            max: KitVec2::new(bounds.max.x.min(clip.max.x), bounds.max.y.min(clip.max.y)),
        }
    } else {
        bounds
    }
}

fn output_bounds(frame: &Framebuffer, rect: Bounds, clip: Option<Bounds>) -> (u32, u32, u32, u32) {
    let mut min_x = rect.min.x;
    let mut min_y = rect.min.y;
    let mut max_x = rect.max.x;
    let mut max_y = rect.max.y;
    if let Some(clip) = clip {
        min_x = min_x.max(clip.min.x);
        min_y = min_y.max(clip.min.y);
        max_x = max_x.min(clip.max.x);
        max_y = max_y.min(clip.max.y);
    }
    (
        min_x.floor().max(0.0) as u32,
        min_y.floor().max(0.0) as u32,
        max_x.ceil().max(0.0).min(frame.width as f32) as u32,
        max_y.ceil().max(0.0).min(frame.height as f32) as u32,
    )
}

fn rgba_rgb(value: Rgba) -> Rgb {
    Rgb::new(value.r, value.g, value.b).clamp01()
}

fn rgba_premul(value: Rgba) -> PremulRgba {
    StraightRgba::new(rgba_rgb(value), value.a).premultiply()
}

fn premul_rgba(value: PremulRgba) -> Rgba {
    let straight = value.to_straight();
    Rgba::new(straight.rgb.r, straight.rgb.g, straight.rgb.b, straight.a)
}

#[derive(Clone)]
struct Snapshot {
    x0: u32,
    y0: u32,
    width: usize,
    height: usize,
    pixels: Vec<PremulRgba>,
}

impl Snapshot {
    fn capture(frame: &Framebuffer, bounds: Bounds) -> Self {
        let x0 = bounds.min.x.floor().max(0.0).min(frame.width as f32) as u32;
        let y0 = bounds.min.y.floor().max(0.0).min(frame.height as f32) as u32;
        let x1 = bounds.max.x.ceil().max(0.0).min(frame.width as f32) as u32;
        let y1 = bounds.max.y.ceil().max(0.0).min(frame.height as f32) as u32;
        let width = x1.saturating_sub(x0) as usize;
        let height = y1.saturating_sub(y0) as usize;
        let mut pixels = Vec::with_capacity(width * height);
        for y in y0..y1 {
            for x in x0..x1 {
                pixels.push(rgba_premul(frame.pixel(x, y)));
            }
        }
        Self {
            x0,
            y0,
            width,
            height,
            pixels,
        }
    }

    fn blur(&self, radius: usize) -> Self {
        let radius = radius.min(96);
        if self.width == 0 || self.height == 0 {
            return Self {
                x0: self.x0,
                y0: self.y0,
                width: self.width,
                height: self.height,
                pixels: Vec::new(),
            };
        }
        let mut horizontal = vec![PremulRgba::transparent(); self.pixels.len()];
        for y in 0..self.height {
            let mut sum = ChannelSum::default();
            for offset in -(radius as isize)..=(radius as isize) {
                sum.add(self.at_clamped(offset, y as isize));
            }
            for x in 0..self.width {
                horizontal[y * self.width + x] = sum.average(radius * 2 + 1);
                sum.remove(self.at_clamped(x as isize - radius as isize, y as isize));
                sum.add(self.at_clamped(x as isize + radius as isize + 1, y as isize));
            }
        }

        let mut pixels = vec![PremulRgba::transparent(); self.pixels.len()];
        for x in 0..self.width {
            let mut sum = ChannelSum::default();
            for offset in -(radius as isize)..=(radius as isize) {
                sum.add(at_clamped_slice(
                    &horizontal,
                    self.width,
                    self.height,
                    x as isize,
                    offset,
                ));
            }
            for y in 0..self.height {
                pixels[y * self.width + x] = sum.average(radius * 2 + 1);
                sum.remove(at_clamped_slice(
                    &horizontal,
                    self.width,
                    self.height,
                    x as isize,
                    y as isize - radius as isize,
                ));
                sum.add(at_clamped_slice(
                    &horizontal,
                    self.width,
                    self.height,
                    x as isize,
                    y as isize + radius as isize + 1,
                ));
            }
        }
        Self {
            x0: self.x0,
            y0: self.y0,
            width: self.width,
            height: self.height,
            pixels,
        }
    }

    fn at_clamped(&self, x: isize, y: isize) -> PremulRgba {
        at_clamped_slice(&self.pixels, self.width, self.height, x, y)
    }

    fn sample(&self, device_x: f32, device_y: f32) -> PremulRgba {
        if self.width == 0 || self.height == 0 {
            return PremulRgba::transparent();
        }
        let x = device_x - self.x0 as f32 - 0.5;
        let y = device_y - self.y0 as f32 - 0.5;
        let x0 = x.floor() as isize;
        let y0 = y.floor() as isize;
        let tx = (x - x0 as f32).clamp(0.0, 1.0);
        let ty = (y - y0 as f32).clamp(0.0, 1.0);
        let a = premul_mix(self.at_clamped(x0, y0), self.at_clamped(x0 + 1, y0), tx);
        let b = premul_mix(
            self.at_clamped(x0, y0 + 1),
            self.at_clamped(x0 + 1, y0 + 1),
            tx,
        );
        premul_mix(a, b, ty)
    }
}

#[derive(Default)]
struct ChannelSum {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl ChannelSum {
    fn add(&mut self, value: PremulRgba) {
        self.r += value.rgb.r;
        self.g += value.rgb.g;
        self.b += value.rgb.b;
        self.a += value.a;
    }

    fn remove(&mut self, value: PremulRgba) {
        self.r -= value.rgb.r;
        self.g -= value.rgb.g;
        self.b -= value.rgb.b;
        self.a -= value.a;
    }

    fn average(&self, count: usize) -> PremulRgba {
        let scale = 1.0 / count.max(1) as f32;
        PremulRgba::new(
            Rgb::new(self.r, self.g, self.b).scale(scale),
            self.a * scale,
        )
        .sanitized()
    }
}

fn at_clamped_slice(
    pixels: &[PremulRgba],
    width: usize,
    height: usize,
    x: isize,
    y: isize,
) -> PremulRgba {
    if width == 0 || height == 0 {
        return PremulRgba::transparent();
    }
    let x = x.clamp(0, width as isize - 1) as usize;
    let y = y.clamp(0, height as isize - 1) as usize;
    pixels[y * width + x]
}

fn premul_mix(a: PremulRgba, b: PremulRgba, amount: f32) -> PremulRgba {
    let t = amount.clamp(0.0, 1.0);
    PremulRgba::new(
        a.rgb.scale(1.0 - t).plus(b.rgb.scale(t)),
        a.a * (1.0 - t) + b.a * t,
    )
    .sanitized()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_blur_does_not_create_dark_color_fringe() {
        let snapshot = Snapshot {
            x0: 0,
            y0: 0,
            width: 3,
            height: 1,
            pixels: vec![
                PremulRgba::transparent(),
                StraightRgba::new(Rgb::new(1.0, 0.0, 0.0), 0.5).premultiply(),
                PremulRgba::transparent(),
            ],
        };
        let blurred = snapshot.blur(1);
        let edge = blurred.pixels[0].to_straight();
        assert!(edge.a > 0.0);
        assert!((edge.rgb.r - 1.0).abs() < 1.0e-5);
        assert!(edge.rgb.g < 1.0e-6 && edge.rgb.b < 1.0e-6);
    }

    #[test]
    fn material_respects_rounded_bounds_and_clip() {
        let clear = Rgba::new(0.05, 0.10, 0.20, 1.0);
        let mut frame = Framebuffer::new(16, 16, clear);
        for y in 0..16 {
            for x in 0..16 {
                frame.set_pixel(x, y, Rgba::new(x as f32 / 16.0, y as f32 / 16.0, 0.2, 1.0));
            }
        }
        let before = frame.pixels().to_vec();
        paint_material_box(
            &mut frame,
            MaterialBox {
                rect: Bounds {
                    min: KitVec2::new(2.0, 2.0),
                    max: KitVec2::new(14.0, 14.0),
                },
                radius: 4.0,
                clip: Some(Bounds {
                    min: KitVec2::new(6.0, 4.0),
                    max: KitVec2::new(12.0, 12.0),
                }),
                tint: Some(Rgba::new(0.2, 0.9, 0.8, 0.8)),
                border: Some((1.0, Rgba::new(1.0, 1.0, 1.0, 0.8))),
                shadow: None,
                material: MaterialPreset::FrostedGlass.material(),
            },
        );
        assert_eq!(frame.pixel(3, 8).r, before[8 * 16 + 3].r);
        assert_eq!(frame.pixel(13, 8).r, before[8 * 16 + 13].r);
        assert_ne!(frame.pixel(8, 8).r, before[8 * 16 + 8].r);
    }

    #[test]
    fn material_does_not_sample_or_reveal_its_own_shadow() {
        let white = Rgba::new(1.0, 1.0, 1.0, 1.0);
        let mut frame = Framebuffer::new(24, 24, white);
        let mut material = MaterialPreset::ClearGlass.material();
        material.strength = 1.0;
        material.refraction_px = 0.0;
        material.rim_width_px = 0.0;
        paint_material_box(
            &mut frame,
            MaterialBox {
                rect: Bounds {
                    min: KitVec2::new(5.0, 5.0),
                    max: KitVec2::new(19.0, 19.0),
                },
                radius: 3.0,
                clip: None,
                tint: Some(Rgba::new(1.0, 1.0, 1.0, 0.0)),
                border: None,
                shadow: Some(Shadow {
                    dx: 0.0,
                    dy: 2.0,
                    blur: 5.0,
                    color: Rgba::new(0.0, 0.0, 0.0, 0.85),
                }),
                material,
            },
        );
        let center = frame.pixel(12, 12);
        assert!(center.r > 0.99 && center.g > 0.99 && center.b > 0.99);
        assert!(
            frame.pixel(12, 21).r < 0.99,
            "exterior shadow should remain visible"
        );
    }

    #[test]
    fn blur_sampling_cannot_cross_an_ancestor_clip() {
        let mut frame = Framebuffer::new(20, 8, Rgba::new(0.0, 1.0, 0.0, 1.0));
        for y in 0..8 {
            for x in 10..20 {
                frame.set_pixel(x, y, Rgba::new(1.0, 0.0, 0.0, 1.0));
            }
        }
        let material = TransparencyMaterial {
            strength: 1.0,
            blur_radius_px: 6.0,
            refraction_px: 0.0,
            rim_width_px: 0.0,
            ..TransparencyMaterial::default()
        };
        paint_material_box(
            &mut frame,
            MaterialBox {
                rect: Bounds {
                    min: KitVec2::new(2.0, 1.0),
                    max: KitVec2::new(18.0, 7.0),
                },
                radius: 0.0,
                clip: Some(Bounds {
                    min: KitVec2::new(2.0, 1.0),
                    max: KitVec2::new(10.0, 7.0),
                }),
                tint: None,
                border: None,
                shadow: None,
                material,
            },
        );
        let edge = frame.pixel(9, 4);
        assert!(edge.g > 0.99 && edge.r < 0.01);
        assert_eq!(frame.pixel(10, 4).r, 1.0);
    }

    #[test]
    fn material_keeps_outer_aa_fringe_and_transparent_backdrops_transparent() {
        let mut opaque = Framebuffer::new(12, 8, Rgba::new(0.0, 0.0, 0.0, 1.0));
        let request = MaterialBox {
            rect: Bounds {
                min: KitVec2::new(4.0, 2.0),
                max: KitVec2::new(8.0, 6.0),
            },
            radius: 0.0,
            clip: None,
            tint: Some(Rgba::new(1.0, 0.0, 0.0, 1.0)),
            border: None,
            shadow: None,
            material: TransparencyMaterial {
                strength: 1.0,
                refraction_px: 0.0,
                rim_width_px: 0.0,
                ..TransparencyMaterial::default()
            },
        };
        paint_material_box(&mut opaque, request);
        assert!(opaque.pixel(3, 4).r > 0.0, "outer AA fringe was clipped");

        let mut transparent = Framebuffer::new(12, 8, Rgba::new(0.0, 0.0, 0.0, 0.0));
        paint_material_box(
            &mut transparent,
            MaterialBox {
                tint: None,
                ..request
            },
        );
        assert_eq!(transparent.pixel(6, 4).a, 0.0);
    }
}
