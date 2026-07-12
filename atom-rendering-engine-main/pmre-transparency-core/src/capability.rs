#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityStatus {
    /// Implemented in the live PMRE screen-space compositor.
    Live,
    /// Implemented as a documented RGB/screen-space approximation.
    Approximation,
    /// Correct value-level math is available, but PMRE has no corresponding scene pass.
    MathOnly,
    /// Requires geometry, ray traversal, temporal history, or other resources PMRE lacks.
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    pub name: &'static str,
    pub status: CapabilityStatus,
}

pub const COOKBOOK_CAPABILITIES: &[Capability] = &[
    Capability {
        name: "premultiplied filtering and compositing",
        status: CapabilityStatus::Live,
    },
    Capability {
        name: "painter-order alpha blending",
        status: CapabilityStatus::Live,
    },
    Capability {
        name: "additive multiply screen cutout and dither",
        status: CapabilityStatus::Live,
    },
    Capability {
        name: "Schlick Fresnel rim",
        status: CapabilityStatus::Live,
    },
    Capability {
        name: "exact dielectric Fresnel",
        status: CapabilityStatus::MathOnly,
    },
    Capability {
        name: "Snell refraction and total internal reflection",
        status: CapabilityStatus::MathOnly,
    },
    Capability {
        name: "Beer-Lambert absorption",
        status: CapabilityStatus::Live,
    },
    Capability {
        name: "thin-walled transmission",
        status: CapabilityStatus::Live,
    },
    Capability {
        name: "rough backdrop blur",
        status: CapabilityStatus::Live,
    },
    Capability {
        name: "screen-space refraction",
        status: CapabilityStatus::Approximation,
    },
    Capability {
        name: "RGB dispersion",
        status: CapabilityStatus::Approximation,
    },
    Capability {
        name: "RGB thin-film interference",
        status: CapabilityStatus::Approximation,
    },
    Capability {
        name: "cheap thickness translucency",
        status: CapabilityStatus::MathOnly,
    },
    Capability {
        name: "Henyey-Greenstein phase density",
        status: CapabilityStatus::MathOnly,
    },
    Capability {
        name: "weighted blended OIT accumulator",
        status: CapabilityStatus::MathOnly,
    },
    Capability {
        name: "scene depth peeling and A-buffer OIT",
        status: CapabilityStatus::Unsupported,
    },
    Capability {
        name: "scene mips dual-Kawase TAA and retained history",
        status: CapabilityStatus::Unsupported,
    },
    Capability {
        name: "colored transparent shadow maps",
        status: CapabilityStatus::Unsupported,
    },
    Capability {
        name: "IBL probes SSR and ray-traced reflections",
        status: CapabilityStatus::Unsupported,
    },
    Capability {
        name: "nested three-dimensional media",
        status: CapabilityStatus::Unsupported,
    },
    Capability {
        name: "ray-traced subsurface scattering",
        status: CapabilityStatus::Unsupported,
    },
    Capability {
        name: "random-walk scattering and volume tracking",
        status: CapabilityStatus::Unsupported,
    },
    Capability {
        name: "photon and manifold caustics",
        status: CapabilityStatus::Unsupported,
    },
    Capability {
        name: "BDPT VCM and spectral path tracing",
        status: CapabilityStatus::Unsupported,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_registry_is_nonempty_unique_and_explicit() {
        assert!(!COOKBOOK_CAPABILITIES.is_empty());
        for (index, capability) in COOKBOOK_CAPABILITIES.iter().enumerate() {
            assert!(!capability.name.is_empty());
            assert!(!COOKBOOK_CAPABILITIES[..index]
                .iter()
                .any(|prior| prior.name == capability.name));
        }
        assert!(COOKBOOK_CAPABILITIES
            .iter()
            .any(|value| value.status == CapabilityStatus::Unsupported));
    }
}
