pub(crate) const PI: f32 = core::f32::consts::PI;
pub(crate) const TAU: f32 = PI * 2.0;
pub(crate) const LN_2: f32 = core::f32::consts::LN_2;

pub(crate) fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

pub(crate) fn sqrt(value: f32) -> f32 {
    if !value.is_finite() || value <= 0.0 {
        return 0.0;
    }
    // Exponent-halving seed keeps Newton convergence stable for tiny and huge magnitudes.
    let mut x = f32::from_bits((value.to_bits() >> 1) + 0x1fc0_0000);
    for _ in 0..6 {
        x = 0.5 * (x + value / x);
    }
    x
}

pub(crate) fn exp_neg(value: f32) -> f32 {
    let x = finite_or(value, 80.0).max(0.0);
    if x >= 80.0 {
        return 0.0;
    }
    // Range-reduce x = k*ln(2) + r. A sixth-order Taylor polynomial only sees
    // r in [0, ln(2)), then an exact IEEE exponent supplies 2^-k.
    let exponent = (x / LN_2) as u32;
    let r = x - exponent as f32 * LN_2;
    let r2 = r * r;
    let polynomial = 1.0 - r + r2 * 0.5 - r2 * r / 6.0 + r2 * r2 / 24.0 - r2 * r2 * r / 120.0
        + r2 * r2 * r2 / 720.0;
    let scale = f32::from_bits((127 - exponent.min(126)) << 23);
    (polynomial * scale).clamp(0.0, 1.0)
}

pub(crate) fn ln(value: f32) -> f32 {
    let mut x = finite_or(value, 1.0).max(1.0e-12);
    let mut exponent = 0i32;
    while x >= 2.0 {
        x *= 0.5;
        exponent += 1;
    }
    while x < 1.0 {
        x *= 2.0;
        exponent -= 1;
    }
    let y = (x - 1.0) / (x + 1.0);
    let y2 = y * y;
    let mut term = y;
    let mut series = 0.0;
    for denominator in [1.0f32, 3.0, 5.0, 7.0, 9.0, 11.0, 13.0] {
        series += term / denominator;
        term *= y2;
    }
    2.0 * series + exponent as f32 * LN_2
}

pub(crate) fn cos(mut value: f32) -> f32 {
    value = finite_or(value, 0.0) % TAU;
    if value > PI {
        value -= TAU;
    } else if value < -PI {
        value += TAU;
    }
    let mut sign = 1.0;
    if value > PI * 0.5 {
        value = PI - value;
        sign = -1.0;
    } else if value < -PI * 0.5 {
        value = -PI - value;
        sign = -1.0;
    }
    let x2 = value * value;
    sign * (1.0 - x2 * 0.5 + x2 * x2 / 24.0 - x2 * x2 * x2 / 720.0 + x2 * x2 * x2 * x2 / 40_320.0)
}

pub(crate) fn pow5(value: f32) -> f32 {
    let x2 = value * value;
    x2 * x2 * value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_math_is_accurate_enough_for_material_work() {
        assert!((sqrt(2.0) - 2.0f32.sqrt()).abs() < 1.0e-5);
        assert!((sqrt(1.0e-10) - 1.0e-5).abs() < 1.0e-9);
        assert!((sqrt(1.0e20) - 1.0e10).abs() < 2_000.0);
        assert!((ln(0.25) - 0.25f32.ln()).abs() < 2.0e-5);
        assert!((exp_neg(LN_2) - 0.5).abs() < 0.002);
        assert!((exp_neg(5.0) - (-5.0f32).exp()).abs() < 2.0e-6);
        assert!((exp_neg(10.0) - (-10.0f32).exp()).abs() < 2.0e-8);
        assert!((cos(PI) + 1.0).abs() < 0.001);
    }
}
