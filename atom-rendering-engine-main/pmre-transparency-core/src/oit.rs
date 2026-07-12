use crate::color::{PremulRgba, Rgb};
use crate::math::finite_or;

pub fn ordered_dither_4x4(x: u32, y: u32) -> f32 {
    const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
    (f32::from(BAYER[((y & 3) * 4 + (x & 3)) as usize]) + 0.5) / 16.0
}

pub fn wboit_weight(alpha: f32, view_depth: f32) -> f32 {
    let a = finite_or(alpha, 0.0).clamp(0.0, 1.0);
    let z = finite_or(view_depth, 0.0).clamp(0.0, 1.0);
    let opacity = a * 8.0 + 0.01;
    let depth = 1.0 - z * 0.9;
    (opacity * opacity * opacity * depth * depth * depth * 1_000.0).clamp(0.01, 3_000.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WboitAccum {
    pub weighted_rgb: Rgb,
    pub weighted_alpha: f32,
    pub revealage: f32,
}

impl Default for WboitAccum {
    fn default() -> Self {
        Self {
            weighted_rgb: Rgb::new(0.0, 0.0, 0.0),
            weighted_alpha: 0.0,
            revealage: 1.0,
        }
    }
}

impl WboitAccum {
    pub fn add(&mut self, layer: PremulRgba, view_depth: f32) {
        let layer = layer.sanitized();
        if layer.a <= 0.0 {
            return;
        }
        let weight = wboit_weight(layer.a, view_depth);
        self.weighted_rgb = self.weighted_rgb.plus(layer.rgb.scale(weight));
        self.weighted_alpha += layer.a * weight;
        self.revealage *= 1.0 - layer.a;
    }

    pub fn resolve_layer(self) -> PremulRgba {
        let alpha = (1.0 - self.revealage).clamp(0.0, 1.0);
        if self.weighted_alpha <= 1.0e-8 || alpha <= 0.0 {
            return PremulRgba::transparent();
        }
        let straight = self.weighted_rgb.scale(1.0 / self.weighted_alpha).clamp01();
        PremulRgba::new(straight.scale(alpha), alpha).sanitized()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dither_thresholds_cover_the_unit_interval_once() {
        let mut seen = [false; 16];
        for y in 0..4 {
            for x in 0..4 {
                let value = ordered_dither_4x4(x, y);
                let index = (value * 16.0 - 0.5).round() as usize;
                seen[index] = true;
            }
        }
        assert!(seen.iter().all(|value| *value));
    }

    #[test]
    fn wboit_is_order_independent() {
        let red = PremulRgba::new(Rgb::new(0.5, 0.0, 0.0), 0.5);
        let blue = PremulRgba::new(Rgb::new(0.0, 0.0, 0.25), 0.25);
        let mut left = WboitAccum::default();
        left.add(red, 0.4);
        left.add(blue, 0.4);
        let mut right = WboitAccum::default();
        right.add(blue, 0.4);
        right.add(red, 0.4);
        assert_eq!(left, right);
        let resolved = left.resolve_layer();
        assert!((resolved.a - 0.625).abs() < 1.0e-6);
        assert!(
            (resolved.rgb.r - 0.461_324_6).abs() < 1.0e-5,
            "unexpected WBOIT red: {resolved:?}"
        );
        assert!((resolved.rgb.b - 0.163_675_4).abs() < 1.0e-5);
        assert_eq!(
            WboitAccum::default().resolve_layer(),
            PremulRgba::transparent()
        );
    }
}
