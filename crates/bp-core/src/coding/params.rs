//! Network-aware coding parameters — adaptive `k` computation.
//!
//! ## Problem
//!
//! Given a network of `N` Pouch peers, each with an independent availability
//! probability `p_i` (the **stability score** from [`crate::network::qos`]),
//! and given a target high recovery probability `Ph`, we need to find the
//! largest integer `k` such that:
//!
//! ```text
//! P(X ≥ k) ≥ Ph     where  X = Σ Bernoulli(p_i)
//! ```
//!
//! `X` follows a **Poisson-Binomial** distribution.
//!
//! ## Normal approximation
//!
//! For the sizes encountered in practice (N ≥ 5), the Poisson-Binomial can be
//! approximated by a normal distribution with matching mean and variance:
//!
//! ```text
//! μ  = Σ p_i              (expected recoverable fragments)
//! σ² = Σ p_i·(1−p_i)      (variance)
//!
//! P(X ≥ k) ≈ 1 − Φ((k − 0.5 − μ) / σ)   ← continuity correction
//! ```
//!
//! Solving for k:
//!
//! ```text
//! k = ⌊ μ − z(Ph) · σ + 0.5 ⌋
//! ```
//!
//! where `z(Ph)` is the standard-normal `Ph`-quantile (probit).
//!
//! ## Redundancy overhead `q`
//!
//! The total number of fragments distributed per chunk is:
//!
//! ```text
//! n = round(k · (1 + q_target))
//! q = (n − k) / k              (effective overhead fraction)
//! ```
//!
//! The default `q_target = 1.0` means each chunk generates `~2k` fragments
//! (one copy-equivalent of redundancy on top of the recovery threshold).
//!
//! ## Rolling pe
//!
//! After upload, the actual observed probability `pe` can be recomputed at any
//! time using [`effective_recovery_probability`] with the latest stability scores
//! and the stored `k`.

use crate::error::{BpError, BpResult};

// ── Public parameters record ──────────────────────────────────────────────────

/// Coding parameters computed for a specific network state and target `Ph`.
///
/// Stored in the [`FileManifest`](crate::storage::manifest::FileManifest)
/// at upload time and used by the daemon to determine how many fragments to
/// generate and distribute per chunk.
#[derive(Debug, Clone)]
pub struct NetworkCodingParams {
    /// Target recovery probability requested at computation time.
    pub ph: f64,
    /// Recovery threshold: the minimum fragments needed to reconstruct a chunk.
    pub k: usize,
    /// Total fragments to generate per chunk (= k + redundancy).
    pub n: usize,
    /// Effective redundancy overhead: `(n − k) / k`.
    pub q: f64,
    /// Number of Pouch peers considered in this computation.
    pub peer_count: usize,
    /// Poisson-Binomial mean `μ = Σ p_i`.
    pub mu: f64,
    /// Poisson-Binomial std-dev `σ = sqrt(Σ p_i·(1−p_i))`.
    pub sigma: f64,
}

impl NetworkCodingParams {
    /// Effective recovery probability given the same set of stability scores.
    ///
    /// Useful for re-evaluating `pe` with updated QoS data after upload.
    pub fn effective_probability(&self) -> f64 {
        if self.sigma < 1e-9 {
            return if self.k as f64 <= self.mu { 1.0 } else { 0.0 };
        }
        let z = (self.k as f64 - 0.5 - self.mu) / self.sigma;
        1.0 - standard_normal_cdf(z)
    }
}

// ── Main entry point ──────────────────────────────────────────────────────────

/// Compute the optimal coding parameters for the given network state.
///
/// # Arguments
///
/// - `stabilities` — slice of per-peer stability scores `p_i ∈ [0.0, 1.0]`
///   produced by [`crate::network::qos::QosRegistry::all_stability_scores`].
/// - `ph`          — target recovery probability, e.g. `0.999`.
/// - `q_target`    — desired redundancy overhead fraction, e.g. `1.0` (= 2×k
///   total fragments per chunk).  Must be `> 0.0`.
///
/// # Errors
///
/// - No peers provided (empty `stabilities`).
/// - `ph` outside `(0.0, 1.0)`.
/// - `q_target ≤ 0.0`.
/// - Network too small or too unreliable to satisfy `Ph` (k would be ≤ 0).
pub fn compute_coding_params(
    stabilities: &[f64],
    ph: f64,
    q_target: f64,
) -> BpResult<NetworkCodingParams> {
    if stabilities.is_empty() {
        return Err(BpError::Coding(
            "Cannot compute coding params: no peers available".into(),
        ));
    }
    if !(0.0 < ph && ph < 1.0) {
        return Err(BpError::Coding(format!("Ph ({ph}) must be in (0.0, 1.0)")));
    }
    if q_target <= 0.0 {
        return Err(BpError::Coding(format!(
            "q_target ({q_target}) must be > 0.0"
        )));
    }

    let peer_count = stabilities.len();

    // Poisson-Binomial mean and variance
    let mu: f64 = stabilities.iter().sum();
    let sigma_sq: f64 = stabilities.iter().map(|&p| p * (1.0 - p)).sum();
    let sigma = sigma_sq.sqrt();

    // k = floor(μ − z(Ph) · σ + 0.5)
    // z(Ph) is the probit (inverse standard-normal CDF) at Ph.
    let z_ph = probit(ph);
    let k_float = mu - z_ph * sigma + 0.5;

    if k_float < 1.0 {
        return Err(BpError::Coding(format!(
            "Network too small or unreliable to achieve Ph={ph:.4}: \
             computed k={k_float:.2} < 1 (μ={mu:.2}, σ={sigma:.2})"
        )));
    }

    let k = k_float.floor() as usize;

    // n = round(k * (1 + q_target)), but at least k+1
    let n = ((k as f64) * (1.0 + q_target)).round().max(k as f64 + 1.0) as usize;

    // n must not exceed total peer count (one fragment per Pouch)
    let n = n.min(peer_count);

    // After capping n, recheck that n >= k
    if n < k {
        return Err(BpError::Coding(format!(
            "After capping to peer_count ({peer_count}), n ({n}) < k ({k}). \
             Not enough peers to store the required fragments."
        )));
    }

    let q = (n - k) as f64 / k as f64;

    Ok(NetworkCodingParams {
        ph,
        k,
        n,
        q,
        peer_count,
        mu,
        sigma,
    })
}

/// Compute the **network storage utilisation factor** `k / N`.
///
/// Given the per-peer stability scores and a per-tier target recovery
/// probability `Ph`, this returns the largest fraction `k / N ∈ [0.0, 1.0]`
/// such that:
///
/// ```text
/// P(X ≥ k) ≥ Ph    where X ~ PoissonBinomial(p_1, …, p_N)
/// ```
///
/// Interpretation: for every raw byte stored by a node, only `k/N` bytes
/// correspond to recoverable file content (the rest is redundancy).
///
/// # Arguments
///
/// - `stabilities` — per-Pouch stability scores including the **own node**.
///   Use `0.4` as the default for a new node with no QoS history.
/// - `ph`          — target recovery probability (from
///   [`crate::network::ReputationTier::qos_target_ph`]).
///
/// Returns `0.0` when the network is empty, too small, or too unreliable
/// to satisfy `Ph`.
pub fn compute_network_storage_factor(stabilities: &[f64], ph: f64) -> f64 {
    let n = stabilities.len();
    if n == 0 || !(0.0 < ph && ph < 1.0) {
        return 0.0;
    }
    let mu: f64 = stabilities.iter().sum();
    let sigma: f64 = stabilities
        .iter()
        .map(|&p| p * (1.0 - p))
        .sum::<f64>()
        .sqrt();
    let z_ph = probit(ph);
    let k_float = mu - z_ph * sigma + 0.5;
    if k_float < 1.0 {
        return 0.0;
    }
    let k = (k_float.floor() as usize).min(n);
    k as f64 / n as f64
}

/// Recompute the **rolling effective recovery probability** `Pe` for a file
/// that was uploaded with threshold `k`, given the current stability scores.
///
/// This is called periodically by the daemon to update `pe` in the
/// [`FileManifest`](crate::storage::manifest::FileManifest).
///
/// ```text
/// Pe = P(X ≥ k)  ≈  1 − Φ((k − 0.5 − μ) / σ)
/// ```
pub fn effective_recovery_probability(stabilities: &[f64], k: usize) -> f64 {
    let mu: f64 = stabilities.iter().sum();
    let sigma_sq: f64 = stabilities.iter().map(|&p| p * (1.0 - p)).sum();
    let sigma = sigma_sq.sqrt();
    if sigma < 1e-9 {
        return if k as f64 <= mu { 1.0 } else { 0.0 };
    }
    let z = (k as f64 - 0.5 - mu) / sigma;
    1.0 - standard_normal_cdf(z)
}

// ── Statistics helpers ────────────────────────────────────────────────────────

/// Standard normal CDF `Φ(x)` using a rational approximation.
///
/// Maximum absolute error < 7.5 × 10⁻⁸ (Hart, 1968 / Abramowitz & Stegun 26.2.17).
pub fn standard_normal_cdf(x: f64) -> f64 {
    // Use the complementary error function: Φ(x) = 0.5 · erfc(−x / √2)
    let t = x / std::f64::consts::SQRT_2;
    0.5 * erfc_approx(-t)
}

/// Inverse standard normal CDF (probit) using the Beasley-Springer-Moro
/// rational approximation.  Accurate to ~10⁻⁹ for `p ∈ (10⁻⁶, 1−10⁻⁶)`.
///
/// Reference: Peter J. Acklam, "An algorithm for computing the inverse normal
/// cumulative distribution function", 2003.
pub fn probit(p: f64) -> f64 {
    debug_assert!(0.0 < p && p < 1.0, "probit: p must be in (0, 1)");

    // Coefficients for the rational approximation
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];

    let p_lo = 0.02425_f64;
    let p_hi = 1.0 - p_lo;

    if p < p_lo {
        // Rational approximation for lower tail
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= p_hi {
        // Rational approximation for central region
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        // Rational approximation for upper tail (use symmetry)
        -probit(1.0 - p)
    }
}

/// Complementary error function approximation used internally by `standard_normal_cdf`.
fn erfc_approx(x: f64) -> f64 {
    // Horner-form Chebyshev approximation; error < 1.5e-7 for all real x.
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let approx = poly * (-x * x).exp();
    if x >= 0.0 {
        approx
    } else {
        2.0 - approx
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    // ── Statistics ────────────────────────────────────────────────────────

    #[test]
    fn standard_normal_cdf_known_values() {
        assert!(approx_eq(standard_normal_cdf(0.0), 0.5, 1e-6));
        assert!(approx_eq(standard_normal_cdf(1.0), 0.841_344_7, 1e-5));
        assert!(approx_eq(standard_normal_cdf(-1.0), 0.158_655_3, 1e-5));
        assert!(approx_eq(standard_normal_cdf(3.0), 0.998_650_1, 1e-5));
        assert!(approx_eq(standard_normal_cdf(-3.0), 0.001_349_9, 1e-5));
    }

    #[test]
    fn probit_known_values() {
        // probit(Φ(z)) ≈ z
        for z in [-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0] {
            let p = standard_normal_cdf(z);
            assert!(
                approx_eq(probit(p), z, 1e-4),
                "z={z} p={p} probit={}",
                probit(p)
            );
        }
    }

    #[test]
    fn probit_0_5_is_zero() {
        assert!(probit(0.5).abs() < 1e-9);
    }

    // ── compute_coding_params ─────────────────────────────────────────────

    #[test]
    fn uniform_perfect_peers() {
        // 10 peers, each with stability 1.0, Ph=0.999
        // μ=10, σ=0 → k = floor(10 - 3.09*0 + 0.5) = 10
        // But σ≈0, so k = floor(μ + 0.5) = 10
        let stabilities = vec![1.0_f64; 10];
        let params = compute_coding_params(&stabilities, 0.999, 1.0).unwrap();
        assert!(params.k > 0);
        assert!(params.k <= 10);
        assert!(params.n >= params.k);
        assert!(approx_eq(
            params.q,
            (params.n - params.k) as f64 / params.k as f64,
            1e-9
        ));
    }

    #[test]
    fn compute_k_decreases_with_lower_ph() {
        let stabilities = vec![0.9_f64; 20];
        let p_high = compute_coding_params(&stabilities, 0.999, 1.0).unwrap();
        let p_low = compute_coding_params(&stabilities, 0.9, 1.0).unwrap();
        // Lower Ph allows a higher k (fewer margins needed)
        assert!(p_high.k <= p_low.k || p_high.k == p_low.k);
    }

    #[test]
    fn compute_k_with_mixed_stabilities() {
        // Heterogeneous network: some fast, some unreliable peers
        let stabilities = vec![0.95, 0.9, 0.8, 0.7, 0.6, 0.95, 0.85, 0.75];
        let params = compute_coding_params(&stabilities, 0.99, 1.0).unwrap();
        assert!(params.k >= 1);
        assert!(params.n >= params.k);
        assert!(params.ph == 0.99);
        // Effective probability should be close to or above ph
        assert!(params.effective_probability() >= 0.9);
    }

    #[test]
    fn error_on_empty_stabilities() {
        assert!(compute_coding_params(&[], 0.99, 1.0).is_err());
    }

    #[test]
    fn error_on_invalid_ph() {
        let s = vec![0.9_f64; 5];
        assert!(compute_coding_params(&s, 0.0, 1.0).is_err());
        assert!(compute_coding_params(&s, 1.0, 1.0).is_err());
        assert!(compute_coding_params(&s, -0.1, 1.0).is_err());
    }

    #[test]
    fn error_on_zero_q_target() {
        let s = vec![0.9_f64; 5];
        assert!(compute_coding_params(&s, 0.99, 0.0).is_err());
    }

    #[test]
    fn n_capped_at_peer_count() {
        // 3 peers, q_target=5.0 → n would be huge; must be capped at 3
        let stabilities = vec![0.9_f64; 3];
        let params = compute_coding_params(&stabilities, 0.9, 5.0).unwrap();
        assert!(params.n <= 3);
    }

    #[test]
    fn effective_recovery_probability_k1_all_good() {
        let stabilities = vec![1.0_f64; 5];
        let pe = effective_recovery_probability(&stabilities, 1);
        assert!(approx_eq(pe, 1.0, 1e-6));
    }

    #[test]
    fn effective_recovery_probability_consistent_with_params() {
        let stabilities = vec![0.85_f64; 15];
        let params = compute_coding_params(&stabilities, 0.99, 1.0).unwrap();
        let pe = effective_recovery_probability(&stabilities, params.k);
        // Should be >= Ph (by construction of k)
        assert!(pe >= 0.99 - 1e-3, "pe={pe} should be ≈0.99");
    }
}
