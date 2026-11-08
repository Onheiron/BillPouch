use crate::error::{BpError, BpResult};

pub const MAX_K: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub k: usize,
    pub n: usize,
    pub per_node: Vec<usize>,
    pub pe: f64,
}

impl Plan {
    pub fn q(&self) -> f64 {
        (self.n - self.k) as f64 / self.k as f64
    }
}

pub fn max_per_node(k: usize) -> usize {
    (k / 4).max(1)
}

pub fn recovery_probability(nodes: &[(f64, usize)], k: usize) -> f64 {
    let total: usize = nodes.iter().map(|&(_, m)| m).sum();
    if k == 0 {
        return 1.0;
    }
    if total < k {
        return 0.0;
    }
    let mut dist = vec![0.0f64; total + 1];
    dist[0] = 1.0;
    let mut reach = 0;
    for &(p, m) in nodes {
        if m == 0 {
            continue;
        }
        let p = p.clamp(0.0, 1.0);
        for s in (0..=reach).rev() {
            let here = dist[s];
            dist[s + m] += here * p;
            dist[s] = here * (1.0 - p);
        }
        reach += m;
    }
    dist[k..].iter().sum::<f64>().min(1.0)
}

pub fn spread(nodes: usize, n: usize, cap: usize) -> Option<Vec<usize>> {
    if nodes == 0 || nodes * cap < n {
        return None;
    }
    let mut per_node = vec![0; nodes];
    for i in 0..n {
        per_node[i % nodes] += 1;
    }
    Some(per_node)
}

pub fn plan(ranked: &[f64], ph: f64, q_target: f64) -> BpResult<Plan> {
    if ranked.is_empty() {
        return Err(BpError::Coding("no Pouch to place fragments on".into()));
    }
    if !(0.0 < ph && ph < 1.0) {
        return Err(BpError::Coding(format!("Ph ({ph}) must be in (0, 1)")));
    }
    if q_target <= 0.0 {
        return Err(BpError::Coding(format!(
            "q_target ({q_target}) must be > 0"
        )));
    }
    let candidate = |k: usize, cap: usize, strict: bool| -> Option<Plan> {
        let wanted = ((k as f64 * (1.0 + q_target)).round() as usize).max(k + 1);
        let n = if strict {
            wanted
        } else {
            wanted.min(ranked.len() * cap)
        };
        if n < k {
            return None;
        }
        let per_node = spread(ranked.len(), n, cap)?;
        let nodes: Vec<(f64, usize)> = ranked
            .iter()
            .copied()
            .zip(per_node.iter().copied())
            .collect();
        let pe = recovery_probability(&nodes, k);
        Some(Plan { k, n, per_node, pe })
    };
    let one_each = (1..=ranked.len().min(MAX_K))
        .rev()
        .filter_map(|k| candidate(k, 1, true))
        .find(|p| p.pe >= ph);
    if let Some(plan) = one_each {
        return Ok(plan);
    }
    let spread_out = (1..=MAX_K)
        .rev()
        .filter_map(|k| candidate(k, max_per_node(k), true))
        .find(|p| p.pe >= ph);
    if let Some(plan) = spread_out {
        return Ok(plan);
    }
    (1..=MAX_K)
        .filter_map(|k| candidate(k, max_per_node(k), false))
        .max_by(|a, b| a.pe.total_cmp(&b.pe).then(b.k.cmp(&a.k)))
        .ok_or_else(|| BpError::Coding("no placement possible".into()))
}

pub fn storage_factor(stabilities: &[f64], ph: f64) -> f64 {
    if stabilities.is_empty() || !(0.0 < ph && ph < 1.0) {
        return 0.0;
    }
    let nodes: Vec<(f64, usize)> = stabilities.iter().map(|&p| (p, 1)).collect();
    (1..=stabilities.len())
        .rev()
        .find(|&k| recovery_probability(&nodes, k) >= ph)
        .map_or(0.0, |k| k as f64 / stabilities.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn recovery_probability_matches_the_binomial() {
        let nodes = vec![(0.9, 1); 3];
        assert!(close(recovery_probability(&nodes, 3), 0.729));
        assert!(close(
            recovery_probability(&nodes, 2),
            0.729 + 3.0 * 0.81 * 0.1
        ));
        assert!(close(recovery_probability(&nodes, 1), 1.0 - 0.001));
        assert!(close(recovery_probability(&nodes, 0), 1.0));
        assert!(close(recovery_probability(&nodes, 4), 0.0));
    }

    #[test]
    fn recovery_probability_weights_nodes_by_their_fragments() {
        let nodes = [(0.5, 2), (0.9, 1)];
        assert!(close(recovery_probability(&nodes, 3), 0.45));
        assert!(close(recovery_probability(&nodes, 2), 0.5 + 0.5 * 0.0));
        assert!(close(recovery_probability(&nodes, 1), 1.0 - 0.5 * 0.1));
    }

    #[test]
    fn fragments_are_spread_round_robin_up_to_the_cap() {
        assert_eq!(spread(3, 5, 2), Some(vec![2, 2, 1]));
        assert_eq!(spread(3, 7, 2), None);
        assert_eq!(spread(0, 1, 1), None);
        assert_eq!(max_per_node(3), 1);
        assert_eq!(max_per_node(8), 2);
        assert_eq!(max_per_node(13), 3);
    }

    #[test]
    fn one_fragment_per_node_is_preferred_when_it_meets_the_target() {
        let p = plan(&[0.99; 10], 0.999, 1.0).unwrap();
        assert!(p.per_node.iter().all(|&m| m <= 1));
        assert_eq!(p.n, 10);
        assert_eq!(p.k, 5);
        assert!(p.pe >= 0.999);
    }

    #[test]
    fn few_nodes_get_at_most_a_quarter_of_k_each() {
        let p = plan(&[0.99; 3], 0.999, 1.0).unwrap();
        assert!(p.pe >= 0.999, "{p:?}");
        assert!(p.per_node.iter().all(|&m| m <= max_per_node(p.k)));
    }

    #[test]
    fn an_unreachable_target_returns_the_most_reliable_plan() {
        let p = plan(&[0.5], 0.999, 1.0).unwrap();
        assert_eq!((p.k, p.n), (1, 1));
        assert!(close(p.pe, 0.5));
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        assert!(plan(&[], 0.99, 1.0).is_err());
        assert!(plan(&[0.9], 1.0, 1.0).is_err());
        assert!(plan(&[0.9], 0.99, 0.0).is_err());
    }

    #[test]
    fn storage_factor_is_the_usable_share_of_the_raw_space() {
        assert!(close(storage_factor(&[1.0; 4], 0.999), 1.0));
        assert!(close(storage_factor(&[0.5; 4], 0.999), 0.0));
        let f = storage_factor(&[0.9; 10], 0.99);
        assert!(f > 0.5 && f < 0.9, "{f}");
    }
}
