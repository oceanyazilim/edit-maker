//! Event-based evaluation metrics (mir_eval-style one-to-one matching).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventScore {
    pub precision: f64,
    pub recall: f64,
    pub f_measure: f64,
    pub matched: usize,
    pub estimated: usize,
    pub reference: usize,
}

/// Greedy one-to-one matching of event times (seconds) within ±`tolerance` seconds.
/// Both inputs must be sorted ascending.
pub fn f_measure(reference: &[f64], estimated: &[f64], tolerance: f64) -> EventScore {
    let mut used = vec![false; estimated.len()];
    let mut matched = 0usize;
    for &r in reference {
        let mut best: Option<(usize, f64)> = None;
        let start = estimated.partition_point(|&e| e < r - tolerance);
        for (j, &e) in estimated.iter().enumerate().skip(start) {
            if e > r + tolerance {
                break;
            }
            if used[j] {
                continue;
            }
            let d = (e - r).abs();
            if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((j, d));
            }
        }
        if let Some((j, _)) = best {
            used[j] = true;
            matched += 1;
        }
    }
    let p = if estimated.is_empty() {
        0.0
    } else {
        matched as f64 / estimated.len() as f64
    };
    let r = if reference.is_empty() {
        0.0
    } else {
        matched as f64 / reference.len() as f64
    };
    let f = if p + r > 0.0 {
        2.0 * p * r / (p + r)
    } else {
        0.0
    };
    EventScore {
        precision: p,
        recall: r,
        f_measure: f,
        matched,
        estimated: estimated.len(),
        reference: reference.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn perfect_and_partial() {
        let r = [1.0, 2.0, 3.0];
        assert_eq!(f_measure(&r, &r, 0.07).f_measure, 1.0);
        let e = [1.05, 2.2, 3.0, 4.0];
        let s = f_measure(&r, &e, 0.07);
        assert_eq!(s.matched, 2);
        assert!((s.precision - 0.5).abs() < 1e-9);
    }
}
