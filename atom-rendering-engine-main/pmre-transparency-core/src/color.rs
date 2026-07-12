use crate::math::finite_or;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    pub fn sanitized(self) -> Self {
        Self::new(
            finite_or(self.r, 0.0).max(0.0),
            finite_or(self.g, 0.0).max(0.0),
            finite_or(self.b, 0.0).max(0.0),
        )
    }

    pub fn clamp01(self) -> Self {
        let c = self.sanitized();
        Self::new(c.r.min(1.0), c.g.min(1.0), c.b.min(1.0))
    }

    pub fn scale(self, value: f32) -> Self {
        let value = finite_or(value, 0.0);
        Self::new(self.r * value, self.g * value, self.b * value)
    }

    pub fn multiply(self, other: Self) -> Self {
        Self::new(self.r * other.r, self.g * other.g, self.b * other.b)
    }

    pub fn plus(self, other: Self) -> Self {
        Self::new(self.r + other.r, self.g + other.g, self.b + other.b)
    }

    pub fn mix(self, other: Self, amount: f32) -> Self {
        let t = finite_or(amount, 0.0).clamp(0.0, 1.0);
        self.scale(1.0 - t).plus(other.scale(t))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StraightRgba {
    pub rgb: Rgb,
    pub a: f32,
}

impl StraightRgba {
    pub const fn new(rgb: Rgb, a: f32) -> Self {
        Self { rgb, a }
    }

    pub fn premultiply(self) -> PremulRgba {
        let alpha = finite_or(self.a, 0.0).clamp(0.0, 1.0);
        PremulRgba::new(self.rgb.clamp01().scale(alpha), alpha)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PremulRgba {
    pub rgb: Rgb,
    pub a: f32,
}

impl PremulRgba {
    pub const fn new(rgb: Rgb, a: f32) -> Self {
        Self { rgb, a }
    }

    pub const fn transparent() -> Self {
        Self::new(Rgb::new(0.0, 0.0, 0.0), 0.0)
    }

    pub fn sanitized(self) -> Self {
        let alpha = finite_or(self.a, 0.0).clamp(0.0, 1.0);
        let rgb = self.rgb.sanitized();
        Self::new(
            Rgb::new(rgb.r.min(alpha), rgb.g.min(alpha), rgb.b.min(alpha)),
            alpha,
        )
    }

    pub fn to_straight(self) -> StraightRgba {
        let value = self.sanitized();
        if value.a <= 1.0e-8 {
            StraightRgba::new(Rgb::new(0.0, 0.0, 0.0), 0.0)
        } else {
            StraightRgba::new(value.rgb.scale(1.0 / value.a).clamp01(), value.a)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Over,
    Additive,
    MultiplyTint,
    SoftAdd,
    Cutout,
    Dither,
}

pub fn over(dst: PremulRgba, src: PremulRgba) -> PremulRgba {
    let dst = dst.sanitized();
    let src = src.sanitized();
    let inv = 1.0 - src.a;
    PremulRgba::new(src.rgb.plus(dst.rgb.scale(inv)), src.a + dst.a * inv).sanitized()
}

pub fn additive(dst: PremulRgba, src: PremulRgba) -> PremulRgba {
    let dst = dst.sanitized();
    let src = src.sanitized();
    let alpha = src.a + dst.a * (1.0 - src.a);
    PremulRgba::new(dst.rgb.plus(src.rgb), alpha).sanitized()
}

pub fn multiply_tint(dst: PremulRgba, tint: Rgb, coverage: f32) -> PremulRgba {
    let dst = dst.sanitized();
    let t = finite_or(coverage, 0.0).clamp(0.0, 1.0);
    let factor = Rgb::new(1.0, 1.0, 1.0).mix(tint.clamp01(), t);
    PremulRgba::new(dst.rgb.multiply(factor), dst.a).sanitized()
}

/// A dependency-free screen blend used as PMRE's documented soft-add policy.
pub fn soft_add(dst: PremulRgba, src: PremulRgba) -> PremulRgba {
    let d = dst.to_straight();
    let s = src.to_straight();
    let screen = Rgb::new(
        1.0 - (1.0 - d.rgb.r) * (1.0 - s.rgb.r),
        1.0 - (1.0 - d.rgb.g) * (1.0 - s.rgb.g),
        1.0 - (1.0 - d.rgb.b) * (1.0 - s.rgb.b),
    );
    let rgb = d
        .rgb
        .scale(d.a * (1.0 - s.a))
        .plus(s.rgb.scale(s.a * (1.0 - d.a)))
        .plus(screen.scale(s.a * d.a));
    PremulRgba::new(rgb, s.a + d.a * (1.0 - s.a)).sanitized()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplication_and_over_match_reference_vectors() {
        let p = StraightRgba::new(Rgb::new(1.0, 0.5, 0.0), 0.25).premultiply();
        assert_eq!(p, PremulRgba::new(Rgb::new(0.25, 0.125, 0.0), 0.25));

        let blue = StraightRgba::new(Rgb::new(0.0, 0.0, 1.0), 1.0).premultiply();
        let red = StraightRgba::new(Rgb::new(1.0, 0.0, 0.0), 0.5).premultiply();
        let mixed = over(blue, red).to_straight();
        assert!((mixed.rgb.r - 0.5).abs() < 1.0e-6);
        assert!((mixed.rgb.b - 0.5).abs() < 1.0e-6);
        assert!((mixed.a - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn invalid_colors_fail_closed() {
        let value = StraightRgba::new(Rgb::new(f32::NAN, -2.0, 4.0), f32::NAN).premultiply();
        assert_eq!(value, PremulRgba::transparent());
    }

    #[test]
    fn soft_add_does_not_apply_source_alpha_twice() {
        let transparent = PremulRgba::transparent();
        let red = StraightRgba::new(Rgb::new(1.0, 0.0, 0.0), 0.5).premultiply();
        assert_eq!(soft_add(transparent, red), red);
        let opaque_blue = StraightRgba::new(Rgb::new(0.0, 0.0, 1.0), 1.0).premultiply();
        let result = soft_add(opaque_blue, red).to_straight();
        assert_eq!(result, StraightRgba::new(Rgb::new(0.5, 0.0, 1.0), 1.0));
    }
}
