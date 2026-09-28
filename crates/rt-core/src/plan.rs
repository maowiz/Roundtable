//! The review plan: reviewer i targets (i + r) mod N for r = 1..N-1 (PRD section 4).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReviewPair {
    pub reviewer: u8,
    pub target: u8,
    /// Teaching-metaphor grouping only; the engine never waits on it.
    pub rotation: u8,
}

/// Every off-diagonal (reviewer, target) pair exactly once, grouped by rotation.
pub fn review_plan(n: usize) -> Vec<ReviewPair> {
    assert!((crate::bounds::MIN_SEATS..=crate::bounds::MAX_SEATS).contains(&n), "seat count out of range");
    let mut out = Vec::with_capacity(n * (n - 1));
    for r in 1..n {
        for i in 0..n {
            out.push(ReviewPair { reviewer: i as u8, target: ((i + r) % n) as u8, rotation: r as u8 });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn covers_every_off_diagonal_pair_exactly_once() {
        for n in 2..=8 {
            let plan = review_plan(n);
            assert_eq!(plan.len(), n * (n - 1));
            let set: HashSet<(u8, u8)> = plan.iter().map(|p| (p.reviewer, p.target)).collect();
            assert_eq!(set.len(), plan.len(), "duplicate pair for n={n}");
            for p in &plan {
                assert_ne!(p.reviewer, p.target, "self-review for n={n}");
                assert!(p.rotation >= 1 && (p.rotation as usize) < n);
            }
            for i in 0..n as u8 {
                for j in 0..n as u8 {
                    if i != j {
                        assert!(set.contains(&(i, j)), "missing pair {i}->{j} for n={n}");
                    }
                }
            }
        }
    }

    #[test]
    fn four_seat_example_matches_prd() {
        let plan = review_plan(4);
        let a: Vec<u8> = plan.iter().filter(|p| p.reviewer == 0).map(|p| p.target).collect();
        assert_eq!(a, vec![1, 2, 3]); // A visits B, C, D
        let d: Vec<u8> = plan.iter().filter(|p| p.reviewer == 3).map(|p| p.target).collect();
        assert_eq!(d, vec![0, 1, 2]); // D visits A, B, C
    }
}
