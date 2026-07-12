use crate::color::Rgb;
use crate::math::{cos, finite_or, pow5, sqrt, PI};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }

    pub fn scale(self, value: f32) -> Self {
        Self::new(self.x * value, self.y * value, self.z * value)
    }

    pub fn plus(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }

    pub fn normalized(self) -> Self {
        let length = sqrt(self.dot(self));
        if length <= 1.0e-8 {
            Self::new(0.0, 0.0, 0.0)
        } else {
            self.scale(1.0 / length)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IorPair {
    pub incident: f32,
    pub transmitted: f32,
}

impl IorPair {
    pub const AIR_TO_GLASS: Self = Self {
        incident: 1.0,
        transmitted: 1.5,
    };

    pub fn new(incident: f32, transmitted: f32) -> Self {
        Self {
            incident: valid_ior(incident),
            transmitted: valid_ior(transmitted),
        }
    }

    pub fn eta(self) -> f32 {
        self.incident / self.transmitted
    }
}

impl Default for IorPair {
    fn default() -> Self {
        Self::AIR_TO_GLASS
    }
}

fn valid_ior(value: f32) -> f32 {
    let value = finite_or(value, 1.0);
    if value > 0.0 {
        value
    } else {
        1.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fresnel {
    pub reflectance: f32,
    pub cos_transmitted: f32,
    pub total_internal_reflection: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Refraction {
    Transmitted(Vec3),
    TotalInternalReflection,
}

pub fn fresnel_f0(media: IorPair) -> f32 {
    let media = IorPair::new(media.incident, media.transmitted);
    let ratio = (media.incident - media.transmitted) / (media.incident + media.transmitted);
    ratio * ratio
}

pub fn fresnel_schlick(cos_incident: f32, media: IorPair) -> f32 {
    let cosine = finite_or(cos_incident, 1.0).abs().clamp(0.0, 1.0);
    let f0 = fresnel_f0(media);
    f0 + (1.0 - f0) * pow5(1.0 - cosine)
}

pub fn fresnel_dielectric_exact(cos_incident: f32, media: IorPair) -> Fresnel {
    let media = IorPair::new(media.incident, media.transmitted);
    let cos_i = finite_or(cos_incident, 1.0).abs().clamp(0.0, 1.0);
    let sin_t2 = media.eta() * media.eta() * (1.0 - cos_i * cos_i);
    if sin_t2 >= 1.0 {
        return Fresnel {
            reflectance: 1.0,
            cos_transmitted: 0.0,
            total_internal_reflection: true,
        };
    }
    let cos_t = sqrt(1.0 - sin_t2);
    let rs_num = media.incident * cos_i - media.transmitted * cos_t;
    let rs_den = media.incident * cos_i + media.transmitted * cos_t;
    let rp_num = media.transmitted * cos_i - media.incident * cos_t;
    let rp_den = media.transmitted * cos_i + media.incident * cos_t;
    let rs = if rs_den.abs() <= 1.0e-8 {
        1.0
    } else {
        rs_num / rs_den
    };
    let rp = if rp_den.abs() <= 1.0e-8 {
        1.0
    } else {
        rp_num / rp_den
    };
    Fresnel {
        reflectance: (0.5 * (rs * rs + rp * rp)).clamp(0.0, 1.0),
        cos_transmitted: cos_t,
        total_internal_reflection: false,
    }
}

/// Refract an incident unit vector through a surface whose normal points toward the
/// incident medium. The result is explicit when total internal reflection occurs.
pub fn refract(incident: Vec3, normal_toward_incident: Vec3, media: IorPair) -> Refraction {
    let i = incident.normalized();
    let n = normal_toward_incident.normalized();
    let cos_i = (-i.dot(n)).clamp(0.0, 1.0);
    let eta = IorPair::new(media.incident, media.transmitted).eta();
    let k = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
    if k < 0.0 {
        Refraction::TotalInternalReflection
    } else {
        Refraction::Transmitted(
            i.scale(eta)
                .plus(n.scale(eta * cos_i - sqrt(k)))
                .normalized(),
        )
    }
}

pub fn screen_refraction_uv(
    uv: Vec2,
    normal_xy: Vec2,
    ior: f32,
    thickness: f32,
    positive_view_depth: f32,
) -> Vec2 {
    let depth = finite_or(positive_view_depth, 1.0).max(1.0e-6);
    let strength = (valid_ior(ior) - 1.0) * finite_or(thickness, 0.0).max(0.0) / depth;
    Vec2::new(
        finite_or(uv.x, 0.0) + finite_or(normal_xy.x, 0.0) * strength,
        finite_or(uv.y, 0.0) + finite_or(normal_xy.y, 0.0) * strength,
    )
}

/// Cheap three-wavelength thin-film interference weight. This is an RGB screen-space
/// approximation, not a multilayer spectral path model.
pub fn thin_film_rgb(cos_incident: f32, film_ior: f32, thickness_nm: f32) -> Rgb {
    let n = valid_ior(film_ior);
    let d = finite_or(thickness_nm, 0.0).max(0.0);
    let c = finite_or(cos_incident, 1.0).abs().clamp(0.0, 1.0);
    let response = |wavelength_nm: f32| {
        let phase = 4.0 * PI * n * d * c / wavelength_nm;
        (0.5 + 0.5 * cos(phase)).clamp(0.0, 1.0)
    };
    Rgb::new(response(650.0), response(510.0), response(475.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresnel_vectors_and_tir_match_reference() {
        let pair = IorPair::AIR_TO_GLASS;
        assert!((fresnel_f0(pair) - 0.04).abs() < 1.0e-6);
        assert!((fresnel_schlick(0.5, pair) - 0.07).abs() < 1.0e-6);
        assert!((fresnel_dielectric_exact(1.0, pair).reflectance - 0.04).abs() < 1.0e-6);

        let tir = fresnel_dielectric_exact(0.5, IorPair::new(1.5, 1.0));
        assert!(tir.total_internal_reflection);
        assert_eq!(tir.reflectance, 1.0);
        assert_eq!(
            refract(
                Vec3::new(0.866_025_4, 0.0, -0.5),
                Vec3::new(0.0, 0.0, 1.0),
                IorPair::new(1.5, 1.0),
            ),
            Refraction::TotalInternalReflection
        );
    }

    #[test]
    fn normal_refraction_and_screen_offset_are_stable() {
        let Refraction::Transmitted(ray) = refract(
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            IorPair::AIR_TO_GLASS,
        ) else {
            panic!("normal incidence must transmit");
        };
        assert!((ray.z + 1.0).abs() < 1.0e-6);
        let uv = screen_refraction_uv(Vec2::new(0.5, 0.5), Vec2::new(0.2, -0.1), 1.5, 2.0, 4.0);
        assert!((uv.x - 0.55).abs() < 1.0e-6);
        assert!((uv.y - 0.475).abs() < 1.0e-6);
        assert_eq!(
            screen_refraction_uv(
                Vec2::new(f32::NAN, 0.5),
                Vec2::new(f32::INFINITY, f32::NAN),
                f32::NAN,
                f32::NAN,
                0.0,
            ),
            Vec2::new(0.0, 0.5)
        );
    }

    #[test]
    fn thin_film_reference_is_bounded() {
        let film = thin_film_rgb(1.0, 1.5, 100.0);
        assert!((film.r - 0.014_529).abs() < 0.001);
        assert!((film.g - 0.074_891).abs() < 0.001);
        assert!((film.b - 0.161_359).abs() < 0.001);
        assert_eq!(thin_film_rgb(1.0, 1.5, 0.0), Rgb::new(1.0, 1.0, 1.0));
    }
}
