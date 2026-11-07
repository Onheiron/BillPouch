use crate::error::{BpError, BpResult};

#[derive(Debug, Clone)]
pub struct NetworkCodingParams {
    pub ph: f64,
    pub k: usize,
    pub n: usize,
    pub q: f64,
    pub peer_count: usize,
    pub mu: f64,
    pub sigma: f64,
}

impl NetworkCodingParams {}

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

    let mu: f64 = stabilities.iter().sum();
    let sigma_sq: f64 = stabilities.iter().map(|&p| p * (1.0 - p)).sum();
    let sigma = sigma_sq.sqrt();

    let z_ph = probit(ph);
    let k_float = mu - z_ph * sigma + 0.5;

    if k_float < 1.0 {
        return Err(BpError::Coding(format!(
            "Network too small or unreliable to achieve Ph={ph:.4}: \
             computed k={k_float:.2} < 1 (μ={mu:.2}, σ={sigma:.2})"
        )));
    }

    let k = k_float.floor() as usize;

    let n = ((k as f64) * (1.0 + q_target)).round().max(k as f64 + 1.0) as usize;

    let n = n.min(peer_count);

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

pub fn standard_normal_cdf(x: f64) -> f64 {
    let t = x / std::f64::consts::SQRT_2;
    0.5 * erfc_approx(-t)
}

pub fn probit(p: f64) -> f64 {
    debug_assert!(0.0 < p && p < 1.0, "probit: p must be in (0, 1)");

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
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= p_hi {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        -probit(1.0 - p)
    }
}

fn erfc_approx(x: f64) -> f64 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

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

    #[test]
    fn uniform_perfect_peers() {
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
        assert!(p_high.k <= p_low.k || p_high.k == p_low.k);
    }

    #[test]
    fn compute_k_with_mixed_stabilities() {
        let stabilities = vec![0.95, 0.9, 0.8, 0.7, 0.6, 0.95, 0.85, 0.75];
        let params = compute_coding_params(&stabilities, 0.99, 1.0).unwrap();
        assert!(params.k >= 1);
        assert!(params.n >= params.k);
        assert!(params.ph == 0.99);
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
        assert!(pe >= 0.99 - 1e-3, "pe={pe} should be ≈0.99");
    }
}
