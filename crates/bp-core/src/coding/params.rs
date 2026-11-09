use crate::error::{BpError, BpResult};

pub const MAX_K: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub k: usize,
    pub n: usize,
    pub pe: f64,
}

pub fn recovery_probability(stabilities: &[f64], k: usize) -> f64 {
    if k == 0 {
        return 1.0;
    }
    if stabilities.len() < k {
        return 0.0;
    }
    let mut dist = vec![0.0f64; stabilities.len() + 1];
    dist[0] = 1.0;
    for (reach, &p) in stabilities.iter().enumerate() {
        let p = p.clamp(0.0, 1.0);
        for s in (0..=reach).rev() {
            let here = dist[s];
            dist[s + 1] += here * p;
            dist[s] = here * (1.0 - p);
        }
    }
    dist[k..].iter().sum::<f64>().min(1.0)
}

pub fn plan(stabilities: &[f64], ph: f64) -> BpResult<Plan> {
    if stabilities.is_empty() {
        return Err(BpError::Coding("no Pouch to place fragments on".into()));
    }
    if !(0.0 < ph && ph < 1.0) {
        return Err(BpError::Coding(format!("Ph ({ph}) must be in (0, 1)")));
    }
    let n = stabilities.len();
    let k = (1..=n.min(MAX_K))
        .rev()
        .find(|&k| recovery_probability(stabilities, k) >= ph)
        .unwrap_or(1);
    Ok(Plan {
        k,
        n,
        pe: recovery_probability(stabilities, k),
    })
}

pub fn best_plan(availabilities: &[f64], ph: f64) -> BpResult<(Plan, Vec<usize>)> {
    if availabilities.is_empty() {
        return Err(BpError::Coding("no Pouch to place fragments on".into()));
    }
    let mut order: Vec<usize> = (0..availabilities.len()).collect();
    order.sort_by(|&a, &b| {
        availabilities[b]
            .total_cmp(&availabilities[a])
            .then(a.cmp(&b))
    });
    let mut best: Option<(Plan, usize)> = None;
    for m in 1..=order.len() {
        let chosen: Vec<f64> = order[..m].iter().map(|&i| availabilities[i]).collect();
        let candidate = plan(&chosen, ph)?;
        if candidate.pe < ph {
            continue;
        }
        let better = best
            .as_ref()
            .is_none_or(|(b, _)| candidate.k * b.n >= b.k * candidate.n);
        if better {
            best = Some((candidate, m));
        }
    }
    let (chosen, m) = match best {
        Some(found) => found,
        None => (plan(availabilities, ph)?, availabilities.len()),
    };
    let mut nodes = order[..m].to_vec();
    nodes.sort_unstable();
    Ok((chosen, nodes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn recovery_probability_matches_the_binomial() {
        let nodes = [0.9; 3];
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
    fn recovery_probability_handles_different_nodes() {
        let nodes = [0.5, 0.9];
        assert!(close(recovery_probability(&nodes, 2), 0.45));
        assert!(close(recovery_probability(&nodes, 1), 1.0 - 0.5 * 0.1));
    }

    #[test]
    fn every_node_gets_one_fragment_and_k_is_the_largest_that_meets_the_target() {
        let p = plan(&[0.99; 10], 0.999).unwrap();
        assert_eq!(p.n, 10);
        assert!(p.pe >= 0.999);
        assert!(recovery_probability(&[0.99; 10], p.k + 1) < 0.999);

        let many = plan(&[0.95; 100], 0.999).unwrap();
        assert_eq!(many.n, 100);
        assert!(many.k > 80, "{many:?}");
    }

    #[test]
    fn an_unreachable_target_falls_back_to_full_replication() {
        let p = plan(&[0.5, 0.5], 0.999).unwrap();
        assert_eq!((p.k, p.n), (1, 2));
        assert!(close(p.pe, 0.75));
    }

    #[test]
    fn k_never_exceeds_the_coding_limit() {
        assert_eq!(plan(&[1.0; 300], 0.999).unwrap().k, MAX_K);
    }

    #[test]
    fn a_weak_node_gets_no_fragments_when_it_cannot_raise_k() {
        let mut nodes = vec![0.9; 10];
        nodes.push(0.1);
        let (p, chosen) = best_plan(&nodes, 0.999).unwrap();
        assert!(!chosen.contains(&10));
        assert!(p.pe >= 0.999);
        assert!(p.k * 10 >= 5 * p.n, "{p:?}");
        assert_eq!(chosen.len(), p.n);
    }

    #[test]
    fn a_reliable_newcomer_raises_k_over_n() {
        let (before, _) = best_plan(&[0.9; 10], 0.999).unwrap();
        let mut nodes = vec![0.9; 10];
        nodes.push(0.99);
        let (after, chosen) = best_plan(&nodes, 0.999).unwrap();
        assert!(after.k * before.n >= before.k * after.n);
        assert!(chosen.contains(&10));
        assert!(after.pe >= 0.999);
    }

    #[test]
    fn an_unreachable_target_uses_every_node() {
        let (p, chosen) = best_plan(&[0.03; 4], 0.999).unwrap();
        assert_eq!((p.k, p.n), (1, 4));
        assert_eq!(chosen, vec![0, 1, 2, 3]);
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        assert!(plan(&[], 0.99).is_err());
        assert!(best_plan(&[], 0.99).is_err());
        assert!(plan(&[0.9], 1.0).is_err());
    }
}
