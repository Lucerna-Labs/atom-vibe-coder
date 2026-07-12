use crate::color::Rgb;
use crate::math::{exp_neg, finite_or, ln, sqrt, PI};
use crate::optics::Vec3;

pub fn absorption_sigma(color_at_distance: Rgb, at_distance: f32) -> Rgb {
    let color = color_at_distance.clamp01();
    let distance = finite_or(at_distance, 1.0).max(1.0e-6);
    let sigma = |channel: f32| -ln(channel.max(1.0e-6)) / distance;
    Rgb::new(sigma(color.r), sigma(color.g), sigma(color.b))
}

pub fn beer_lambert(sigma: Rgb, path_length: f32) -> Rgb {
    let sigma = sigma.sanitized();
    let distance = finite_or(path_length, 0.0).max(0.0);
    Rgb::new(
        exp_neg(sigma.r * distance),
        exp_neg(sigma.g * distance),
        exp_neg(sigma.b * distance),
    )
}

pub fn transmission_color(color_at_distance: Rgb, at_distance: f32, path_length: f32) -> Rgb {
    beer_lambert(
        absorption_sigma(color_at_distance, at_distance),
        path_length,
    )
}

/// Henyey-Greenstein phase density. The result is a density and intentionally is not
/// clamped to one; strongly forward-scattering media can exceed one.
pub fn henyey_greenstein(cos_theta: f32, asymmetry: f32) -> f32 {
    let cosine = finite_or(cos_theta, 0.0).clamp(-1.0, 1.0);
    let g = finite_or(asymmetry, 0.0).clamp(-0.999, 0.999);
    let base = (1.0 + g * g - 2.0 * g * cosine).max(1.0e-8);
    (1.0 - g * g) / (4.0 * PI * base * sqrt(base))
}

#[allow(clippy::too_many_arguments)]
pub fn cheap_translucency(
    light_direction: Vec3,
    view_direction: Vec3,
    normal: Vec3,
    light_color: Rgb,
    thickness: f32,
    sigma: Rgb,
    distortion: f32,
    power: f32,
) -> Rgb {
    let bent = light_direction
        .normalized()
        .plus(normal.normalized().scale(finite_or(distortion, 0.0)))
        .normalized()
        .scale(-1.0);
    let alignment = view_direction.normalized().dot(bent).max(0.0);
    let exponent = finite_or(power, 1.0).max(0.0);
    let shaped = if exponent <= 0.0 {
        1.0
    } else {
        // Integer-free dependency-free approximation is sufficient for this artistic lobe.
        exp_neg(-crate::math::ln(alignment.max(1.0e-6)) * exponent)
    };
    light_color
        .clamp01()
        .multiply(beer_lambert(sigma, thickness))
        .scale(shaped)
        .clamp01()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beer_lambert_matches_artist_color_reference() {
        let sigma = absorption_sigma(Rgb::new(0.5, 0.25, 1.0), 2.0);
        assert!((sigma.r - 0.346_573_6).abs() < 2.0e-5);
        assert!((sigma.g - core::f32::consts::LN_2).abs() < 2.0e-5);
        let transmitted = beer_lambert(sigma, 4.0);
        assert!((transmitted.r - 0.25).abs() < 0.003);
        assert!((transmitted.g - 0.0625).abs() < 0.003);
        assert!((transmitted.b - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn phase_density_and_translucency_match_reference() {
        assert!((henyey_greenstein(-0.8, 0.0) - 0.079_577_47).abs() < 1.0e-6);
        assert!((henyey_greenstein(1.0, 0.8) - 3.580_986).abs() < 0.002);
        let output = cheap_translucency(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            Rgb::new(1.0, 0.5, 0.25),
            1.0,
            Rgb::new(crate::math::LN_2, 0.0, 0.0),
            0.0,
            1.0,
        );
        assert!((output.r - 0.5).abs() < 0.002);
        assert!((output.g - 0.5).abs() < 0.002);
        assert!((output.b - 0.25).abs() < 0.002);
    }
}
