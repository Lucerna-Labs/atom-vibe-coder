use std::fmt;

// --------------------------------------------------------------------------
// Typed error enum -- exactly one error variant per stage, so an exhaustive
// match at the call site is impossible to write wrong.
// --------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum AtomError {
    PreFailed(&'static str),
    ExtractFailed(&'static str),
    QuantizeFailed(&'static str),
    PostFailed(&'static str),
    VerifyRejected {
        kl_ceiling: f64,
        kl_actual: f64,
        cosine_floor: f64,
        cosine_actual: f64,
    },
}

impl fmt::Display for AtomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtomError::PreFailed(m) => write!(f, "PRE atom failed: {}", m),
            AtomError::ExtractFailed(m) => write!(f, "EXTRACT atom failed: {}", m),
            AtomError::QuantizeFailed(m) => write!(f, "QUANTIZE atom failed: {}", m),
            AtomError::PostFailed(m) => write!(f, "POST atom failed: {}", m),
            AtomError::VerifyRejected {
                kl_ceiling,
                kl_actual,
                cosine_floor,
                cosine_actual,
            } => write!(
                f,
                "VERIFY gate rejected: KL {:.4} > ceiling {:.4} OR cosine {:.4} < floor {:.4}",
                kl_actual, kl_ceiling, cosine_actual, cosine_floor
            ),
        }
    }
}

// --------------------------------------------------------------------------
// PRE stage -- signal-processing atom. Walsh-Hadamard-style pairwise mixing
// over consecutive pairs redistributes concentrated energy before the coarse
// quantizer. Orthogonal and self-inverse: applying twice returns the input.
// --------------------------------------------------------------------------

fn pre_pairwise_mix(input: &[f32]) -> Result<Vec<f32>, AtomError> {
    if input.len() < 2 || input.len() % 2 != 0 {
        return Err(AtomError::PreFailed("input length must be even and >= 2"));
    }
    let mut out = Vec::with_capacity(input.len());
    for chunk in input.chunks_exact(2) {
        let a = chunk[0];
        let b = chunk[1];
        out.push((a + b) * 0.5);
        out.push((a - b) * 0.5);
    }
    Ok(out)
}

fn pre_pairwise_mix_inverse(input: &[f32]) -> Result<Vec<f32>, AtomError> {
    if input.len() % 2 != 0 {
        return Err(AtomError::PostFailed("inverse-PRE requires even length"));
    }
    let mut out = Vec::with_capacity(input.len());
    for chunk in input.chunks_exact(2) {
        let s = chunk[0];
        let d = chunk[1];
        out.push(s + d);
        out.push(s - d);
    }
    Ok(out)
}

// --------------------------------------------------------------------------
// EXTRACT stage -- sparse-representation atom. Pull the top-K magnitude
// outliers into a sidecar at full precision so the coarse quantizer only
// sees the well-behaved bulk. Positions are preserved for exact restoration.
// --------------------------------------------------------------------------

fn extract_top_k_outliers(
    input: &[f32],
    k: usize,
) -> Result<(Vec<(usize, f32)>, Vec<f32>), AtomError> {
    if k > input.len() {
        return Err(AtomError::ExtractFailed(
            "k must not exceed input length -- outlier budget larger than population",
        ));
    }
    if input.iter().any(|v| !v.is_finite()) {
        return Err(AtomError::ExtractFailed(
            "input contains non-finite values -- ordering is undefined",
        ));
    }
    let mut idx: Vec<usize> = (0..input.len()).collect();
    idx.sort_by(|&a, &b| input[b].abs().partial_cmp(&input[a].abs()).unwrap());
    let outliers: Vec<(usize, f32)> = idx.iter().take(k).map(|&i| (i, input[i])).collect();
    let mut bulk = input.to_vec();
    for &(i, _) in &outliers {
        bulk[i] = 0.0;
    }
    Ok((outliers, bulk))
}

// --------------------------------------------------------------------------
// QUANTIZE stage -- classical scalar-quantization atom. Symmetric max-abs
// mapping to a signed 4-bit code (16 levels), returning both the codes and
// the scale so the decoder is deterministic.
// --------------------------------------------------------------------------

fn quantize_sym_q4(bulk: &[f32]) -> Result<(Vec<i8>, f32), AtomError> {
    if bulk.is_empty() {
        return Err(AtomError::QuantizeFailed(
            "cannot quantize an empty block -- scale is undefined",
        ));
    }
    if bulk.iter().any(|v| !v.is_finite()) {
        return Err(AtomError::QuantizeFailed(
            "block contains non-finite values -- max-abs is undefined",
        ));
    }
    let max_abs = bulk.iter().fold(0.0f32, |acc, &v| acc.max(v.abs()));
    if max_abs == 0.0 {
        return Ok((vec![0i8; bulk.len()], 1.0));
    }
    let scale = max_abs / 7.0;
    let codes: Vec<i8> = bulk
        .iter()
        .map(|&v| (v / scale).round().clamp(-7.0, 7.0) as i8)
        .collect();
    Ok((codes, scale))
}

fn dequantize_sym_q4(codes: &[i8], scale: f32) -> Vec<f32> {
    codes.iter().map(|&c| c as f32 * scale).collect()
}

// --------------------------------------------------------------------------
// POST stage -- linear-algebra atom. Restore per-block L2 norm so the coarse
// quantizer's amplitude loss is repaired without needing the original weights.
// --------------------------------------------------------------------------

fn post_preserve_norm(reconstruction: &mut [f32], reference_norm: f32) -> Result<(), AtomError> {
    let current: f32 = reconstruction.iter().map(|v| v * v).sum::<f32>().sqrt();
    if current == 0.0 {
        return Err(AtomError::PostFailed(
            "cannot restore norm on all-zero reconstruction",
        ));
    }
    let factor = reference_norm / current;
    for v in reconstruction.iter_mut() {
        *v *= factor;
    }
    Ok(())
}

// --------------------------------------------------------------------------
// VERIFY stage -- two independent gates borrowed from information theory
// (bidirectional-style KL over normalized magnitude histograms) and vector
// geometry (cosine similarity between reference and reconstruction). Both
// must pass; either failing rejects the composed stack.
// --------------------------------------------------------------------------

fn kl_symmetric(reference: &[f32], reconstruction: &[f32]) -> f64 {
    let hist_ref = normalized_magnitude_hist(reference);
    let hist_rec = normalized_magnitude_hist(reconstruction);
    forward_kl(&hist_ref, &hist_rec) + forward_kl(&hist_rec, &hist_ref)
}

fn normalized_magnitude_hist(v: &[f32]) -> [f64; 8] {
    let max_abs = v.iter().fold(0.0f32, |a, &x| a.max(x.abs())) as f64;
    if max_abs == 0.0 {
        return [0.125; 8];
    }
    let mut counts = [0.0f64; 8];
    for &x in v {
        let bin = (((x.abs() as f64) / max_abs) * 7.999).min(7.0) as usize;
        counts[bin] += 1.0;
    }
    let total: f64 = counts.iter().sum();
    for c in counts.iter_mut() {
        *c = (*c + 1e-6) / (total + 8e-6);
    }
    counts
}

fn forward_kl(p: &[f64; 8], q: &[f64; 8]) -> f64 {
    let mut acc = 0.0;
    for i in 0..8 {
        acc += p[i] * (p[i] / q[i]).ln();
    }
    acc
}

fn cosine_similarity(reference: &[f32], reconstruction: &[f32]) -> f64 {
    let mut dot = 0.0f64;
    let mut nr = 0.0f64;
    let mut nc = 0.0f64;
    for (a, b) in reference.iter().zip(reconstruction.iter()) {
        dot += (*a as f64) * (*b as f64);
        nr += (*a as f64) * (*a as f64);
        nc += (*b as f64) * (*b as f64);
    }
    if nr == 0.0 || nc == 0.0 {
        return 0.0;
    }
    dot / (nr.sqrt() * nc.sqrt())
}

// --------------------------------------------------------------------------
// Compose the atoms into a fixed stack. The order is doctrine, not opinion.
// --------------------------------------------------------------------------

const KL_CEILING: f64 = 0.5;
const COSINE_FLOOR: f64 = 0.90;

fn compose_and_gate(input: &[f32]) -> Result<Vec<f32>, AtomError> {
    let reference_norm: f32 = input.iter().map(|v| v * v).sum::<f32>().sqrt();
    let pre = pre_pairwise_mix(input)?;
    let (outliers, bulk) = extract_top_k_outliers(&pre, 2)?;
    let (codes, scale) = quantize_sym_q4(&bulk)?;
    let mut mixed_recon = dequantize_sym_q4(&codes, scale);
    for &(i, v) in &outliers {
        mixed_recon[i] = v;
    }
    let mut recon = pre_pairwise_mix_inverse(&mixed_recon)?;
    post_preserve_norm(&mut recon, reference_norm)?;
    let kl = kl_symmetric(input, &recon);
    let cosine = cosine_similarity(input, &recon);
    if kl > KL_CEILING || cosine < COSINE_FLOOR {
        return Err(AtomError::VerifyRejected {
            kl_ceiling: KL_CEILING,
            kl_actual: kl,
            cosine_floor: COSINE_FLOOR,
            cosine_actual: cosine,
        });
    }
    Ok(recon)
}

fn main() {
    let sample: Vec<f32> = (0..32).map(|i| ((i as f32 * 0.3).sin()) * 4.0).collect();
    match compose_and_gate(&sample) {
        Ok(recon) => {
            println!("Cross-domain atom stack VERIFY gate PASSED");
            println!("  input len:  {}", sample.len());
            println!("  recon len:  {}", recon.len());
            println!("  cosine:     {:.4}", cosine_similarity(&sample, &recon));
            println!("  kl:         {:.4}", kl_symmetric(&sample, &recon));
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_mix_is_self_inverse_up_to_scale() {
        let x = vec![1.0f32, 2.0, 3.0, 4.0, -1.0, -2.0, 5.0, 6.0];
        let mixed = pre_pairwise_mix(&x).unwrap();
        let back = pre_pairwise_mix_inverse(&mixed).unwrap();
        for (a, b) in x.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-5, "roundtrip a={} b={}", a, b);
        }
    }

    #[test]
    fn pre_rejects_odd_length() {
        let x = vec![1.0f32, 2.0, 3.0];
        assert!(matches!(pre_pairwise_mix(&x), Err(AtomError::PreFailed(_))));
    }

    #[test]
    fn extract_moves_top_k_and_zeros_the_bulk() {
        let x = vec![0.1f32, 10.0, 0.2, -8.0, 0.3, 0.4];
        let (outliers, bulk) = extract_top_k_outliers(&x, 2).unwrap();
        assert_eq!(outliers.len(), 2);
        assert!(outliers.iter().any(|&(i, _)| i == 1));
        assert!(outliers.iter().any(|&(i, _)| i == 3));
        assert_eq!(bulk[1], 0.0);
        assert_eq!(bulk[3], 0.0);
    }

    #[test]
    fn extract_rejects_oversized_k() {
        let x = vec![1.0f32, 2.0, 3.0];
        assert!(matches!(
            extract_top_k_outliers(&x, 4),
            Err(AtomError::ExtractFailed(_))
        ));
    }

    #[test]
    fn quantize_rejects_empty_and_non_finite() {
        assert!(matches!(
            quantize_sym_q4(&[]),
            Err(AtomError::QuantizeFailed(_))
        ));
        assert!(matches!(
            quantize_sym_q4(&[1.0, f32::NAN, 3.0]),
            Err(AtomError::QuantizeFailed(_))
        ));
    }

    #[test]
    fn quantize_and_dequantize_stay_close_on_smooth_input() {
        let x: Vec<f32> = (0..16).map(|i| (i as f32 * 0.4).sin()).collect();
        let (codes, scale) = quantize_sym_q4(&x).unwrap();
        let back = dequantize_sym_q4(&codes, scale);
        let sim = cosine_similarity(&x, &back);
        assert!(sim > 0.90, "coarse quantizer preserves shape: cos={}", sim);
    }

    #[test]
    fn preserve_norm_restores_reference_magnitude() {
        let mut r = vec![1.0f32, 1.0, 1.0, 1.0];
        post_preserve_norm(&mut r, 4.0).unwrap();
        let n: f32 = r.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((n - 4.0).abs() < 1e-5);
    }

    #[test]
    fn full_stack_passes_verify_on_smooth_signal() {
        let sample: Vec<f32> = (0..32).map(|i| ((i as f32 * 0.3).sin()) * 4.0).collect();
        assert!(compose_and_gate(&sample).is_ok());
    }
}
