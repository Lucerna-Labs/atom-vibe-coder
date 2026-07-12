#![no_std]
#![forbid(unsafe_code)]

//! Dependency-free math and material recipes for transparent and translucent rendering.
//!
//! The types deliberately distinguish straight and premultiplied alpha. Screen-space PMRE
//! uses these primitives for backdrop materials; the same value-only API can be reused by
//! `no_std` renderers without allocation or platform services.

mod math;

pub mod capability;
pub mod color;
pub mod material;
pub mod oit;
pub mod optics;
pub mod volume;

pub use capability::{Capability, CapabilityStatus, COOKBOOK_CAPABILITIES};
pub use color::{
    additive, multiply_tint, over, soft_add, BlendMode, PremulRgba, Rgb, StraightRgba,
};
pub use material::{MaterialPreset, TransparencyMaterial};
pub use oit::{ordered_dither_4x4, wboit_weight, WboitAccum};
pub use optics::{
    fresnel_dielectric_exact, fresnel_f0, fresnel_schlick, refract, screen_refraction_uv,
    thin_film_rgb, Fresnel, IorPair, Refraction, Vec2, Vec3,
};
pub use volume::{
    absorption_sigma, beer_lambert, cheap_translucency, henyey_greenstein, transmission_color,
};
