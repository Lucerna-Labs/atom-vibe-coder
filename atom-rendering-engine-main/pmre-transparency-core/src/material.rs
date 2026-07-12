use crate::color::{BlendMode, Rgb};
use crate::math::finite_or;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransparencyMaterial {
    pub blend: BlendMode,
    /// Coverage of the optical effect. The box background keeps its own alpha independently.
    pub strength: f32,
    pub ior: f32,
    pub roughness: f32,
    pub thickness: f32,
    pub transmission_color: Rgb,
    pub transmission_distance: f32,
    pub blur_radius_px: f32,
    pub refraction_px: f32,
    pub dispersion_px: f32,
    pub thin_film_nm: f32,
    pub film_ior: f32,
    pub scatter: f32,
    pub distortion_px: f32,
    pub rim_width_px: f32,
    /// Device-pixel scale applied by the layout boundary; authored materials start at 1.0.
    pub pixel_scale: f32,
    pub phase: f32,
    pub thin_walled: bool,
}

impl Default for TransparencyMaterial {
    fn default() -> Self {
        Self {
            blend: BlendMode::Over,
            strength: 0.85,
            ior: 1.5,
            roughness: 0.0,
            thickness: 1.0,
            transmission_color: Rgb::new(1.0, 1.0, 1.0),
            transmission_distance: 10.0,
            blur_radius_px: 0.0,
            refraction_px: 5.0,
            dispersion_px: 0.0,
            thin_film_nm: 0.0,
            film_ior: 1.5,
            scatter: 0.0,
            distortion_px: 0.0,
            rim_width_px: 1.0,
            pixel_scale: 1.0,
            phase: 0.0,
            thin_walled: false,
        }
    }
}

impl TransparencyMaterial {
    pub fn sanitized(self) -> Self {
        Self {
            blend: self.blend,
            strength: finite_or(self.strength, 0.0).clamp(0.0, 1.0),
            ior: finite_or(self.ior, 1.0).clamp(1.0, 3.0),
            roughness: finite_or(self.roughness, 0.0).clamp(0.0, 1.0),
            thickness: finite_or(self.thickness, 0.0).clamp(0.0, 100.0),
            transmission_color: self.transmission_color.clamp01(),
            transmission_distance: finite_or(self.transmission_distance, 1.0)
                .clamp(0.001, 10_000.0),
            blur_radius_px: finite_or(self.blur_radius_px, 0.0).clamp(0.0, 48.0),
            refraction_px: finite_or(self.refraction_px, 0.0).clamp(0.0, 64.0),
            dispersion_px: finite_or(self.dispersion_px, 0.0).clamp(0.0, 16.0),
            thin_film_nm: finite_or(self.thin_film_nm, 0.0).clamp(0.0, 2_000.0),
            film_ior: finite_or(self.film_ior, 1.0).clamp(1.0, 3.0),
            scatter: finite_or(self.scatter, 0.0).clamp(0.0, 1.0),
            distortion_px: finite_or(self.distortion_px, 0.0).clamp(0.0, 32.0),
            rim_width_px: finite_or(self.rim_width_px, 0.0).clamp(0.0, 16.0),
            pixel_scale: finite_or(self.pixel_scale, 1.0).clamp(0.1, 8.0),
            phase: finite_or(self.phase, 0.0),
            thin_walled: self.thin_walled,
        }
    }

    pub fn scale_pixels(mut self, scale: f32) -> Self {
        let scale = finite_or(scale, 1.0).clamp(0.1, 8.0);
        self.pixel_scale *= scale;
        self.sanitized()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialPreset {
    ClearGlass,
    FrostedGlass,
    Water,
    Crystal,
    SoapFilm,
    Wax,
    Smoke,
    StainedGlass,
    HeatHaze,
}

impl MaterialPreset {
    pub const ALL: [Self; 9] = [
        Self::ClearGlass,
        Self::FrostedGlass,
        Self::Water,
        Self::Crystal,
        Self::SoapFilm,
        Self::Wax,
        Self::Smoke,
        Self::StainedGlass,
        Self::HeatHaze,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::ClearGlass => "Clear glass",
            Self::FrostedGlass => "Frosted glass",
            Self::Water => "Water",
            Self::Crystal => "Crystal",
            Self::SoapFilm => "Soap film",
            Self::Wax => "Wax",
            Self::Smoke => "Smoke",
            Self::StainedGlass => "Stained glass",
            Self::HeatHaze => "Heat haze",
        }
    }

    pub fn material(self) -> TransparencyMaterial {
        let mut m = TransparencyMaterial::default();
        match self {
            Self::ClearGlass => {}
            Self::FrostedGlass => {
                m.strength = 0.92;
                m.roughness = 0.78;
                m.blur_radius_px = 14.0;
                m.refraction_px = 3.0;
                m.scatter = 0.18;
            }
            Self::Water => {
                m.strength = 0.78;
                m.ior = 1.333;
                m.roughness = 0.08;
                m.thickness = 1.4;
                m.transmission_color = Rgb::new(0.78, 0.94, 1.0);
                m.transmission_distance = 8.0;
                m.refraction_px = 8.0;
            }
            Self::Crystal => {
                m.strength = 0.90;
                m.ior = 1.55;
                m.roughness = 0.04;
                m.thickness = 2.0;
                m.transmission_color = Rgb::new(0.74, 0.90, 1.0);
                m.refraction_px = 10.0;
                m.dispersion_px = 1.4;
                m.rim_width_px = 1.8;
            }
            Self::SoapFilm => {
                m.strength = 0.70;
                m.ior = 1.33;
                m.thickness = 0.15;
                m.refraction_px = 2.0;
                m.thin_film_nm = 420.0;
                m.film_ior = 1.33;
                m.rim_width_px = 2.0;
                m.thin_walled = true;
            }
            Self::Wax => {
                m.strength = 0.90;
                m.ior = 1.44;
                m.roughness = 0.58;
                m.thickness = 2.5;
                m.transmission_color = Rgb::new(1.0, 0.72, 0.42);
                m.transmission_distance = 5.0;
                m.blur_radius_px = 6.0;
                m.refraction_px = 1.0;
                m.scatter = 0.72;
            }
            Self::Smoke => {
                m.blend = BlendMode::SoftAdd;
                m.strength = 0.48;
                m.ior = 1.01;
                m.roughness = 1.0;
                m.transmission_color = Rgb::new(0.72, 0.76, 0.80);
                m.transmission_distance = 3.0;
                m.blur_radius_px = 12.0;
                m.refraction_px = 1.0;
                m.scatter = 0.82;
                m.distortion_px = 2.0;
                m.rim_width_px = 0.0;
            }
            Self::StainedGlass => {
                m.strength = 0.88;
                m.ior = 1.50;
                m.thickness = 2.2;
                m.transmission_color = Rgb::new(0.24, 0.72, 0.38);
                m.transmission_distance = 2.4;
                m.refraction_px = 5.0;
                m.rim_width_px = 1.6;
            }
            Self::HeatHaze => {
                m.strength = 0.58;
                m.ior = 1.02;
                m.roughness = 0.10;
                m.refraction_px = 9.0;
                m.distortion_px = 7.0;
                m.rim_width_px = 0.0;
                m.thin_walled = true;
            }
        }
        m.sanitized()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_is_finite_and_bounded() {
        for preset in MaterialPreset::ALL {
            let material = preset.material();
            assert!(material.strength.is_finite());
            assert!((0.0..=1.0).contains(&material.strength));
            assert!((1.0..=3.0).contains(&material.ior));
            assert!(!preset.name().is_empty());
        }
    }

    #[test]
    fn invalid_material_values_fail_closed() {
        let bad = TransparencyMaterial {
            strength: f32::NAN,
            ior: -5.0,
            blur_radius_px: f32::INFINITY,
            ..TransparencyMaterial::default()
        }
        .sanitized();
        assert_eq!(bad.strength, 0.0);
        assert_eq!(bad.ior, 1.0);
        assert_eq!(bad.blur_radius_px, 0.0);
    }
}
