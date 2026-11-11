//! Small DSP helpers.

/// RBJ biquad low-pass.
#[derive(Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn lowpass(sr: f32, fc: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * fc / sr;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - c) / 2.0 / a0,
            b1: (1.0 - c) / a0,
            b2: (1.0 - c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// 4th-order (two cascaded Butterworth biquads) low-pass.
pub fn lowpass_twice(x: &[f32], sr: f32, fc: f32) -> Vec<f32> {
    let mut a = Biquad::lowpass(sr, fc, std::f32::consts::FRAC_1_SQRT_2);
    let mut b = Biquad::lowpass(sr, fc, std::f32::consts::FRAC_1_SQRT_2);
    x.iter().map(|&s| b.process(a.process(s))).collect()
}

pub fn moving_average(x: &[f32], radius: usize) -> Vec<f32> {
    let n = x.len();
    let mut prefix = vec![0f64; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + x[i] as f64;
    }
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(radius);
            let hi = (i + radius + 1).min(n);
            ((prefix[hi] - prefix[lo]) / (hi - lo) as f64) as f32
        })
        .collect()
}

/// Median of a slice (copies).
pub fn median(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    let mut v: Vec<f32> = x.to_vec();
    let m = v.len() / 2;
    v.select_nth_unstable_by(m, |a, b| {
        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
    });
    v[m]
}

pub fn percentile(x: &[f32], p: f32) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    let mut v: Vec<f32> = x.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((v.len() - 1) as f32 * p.clamp(0.0, 1.0)).round() as usize;
    v[idx]
}

pub fn mean(x: &[f32]) -> f32 {
    if x.is_empty() {
        0.0
    } else {
        x.iter().sum::<f32>() / x.len() as f32
    }
}

pub fn std_dev(x: &[f32]) -> f32 {
    if x.len() < 2 {
        return 0.0;
    }
    let m = mean(x);
    (x.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / x.len() as f32).sqrt()
}

/// Rolling median (+ median absolute deviation) with a fixed radius, computed on a strided grid
/// and linearly held for speed. Good enough for adaptive thresholds.
pub fn rolling_median_mad(x: &[f32], radius: usize) -> (Vec<f32>, Vec<f32>) {
    let n = x.len();
    let stride = (radius / 4).max(1);
    let mut med = vec![0f32; n];
    let mut mad = vec![0f32; n];
    let mut i = 0;
    while i < n {
        let lo = i.saturating_sub(radius);
        let hi = (i + radius + 1).min(n);
        let w = &x[lo..hi];
        let m = median(w);
        let dev: Vec<f32> = w.iter().map(|v| (v - m).abs()).collect();
        let d = median(&dev);
        for j in i..(i + stride).min(n) {
            med[j] = m;
            mad[j] = d;
        }
        i += stride;
    }
    (med, mad)
}

/// Indices of local maxima (strictly greater than left neighbour, >= right) above `thr[i]`,
/// enforcing a minimum distance (keeps the larger peak).
pub fn pick_peaks(x: &[f32], thr: &[f32], min_dist: usize) -> Vec<usize> {
    let n = x.len();
    let mut cands: Vec<usize> = (1..n.saturating_sub(1))
        .filter(|&i| x[i] > thr[i] && x[i] > x[i - 1] && x[i] >= x[i + 1])
        .collect();
    if min_dist <= 1 {
        return cands;
    }
    // Greedy by height.
    let mut order = cands.clone();
    order.sort_by(|&a, &b| x[b].partial_cmp(&x[a]).unwrap_or(std::cmp::Ordering::Equal));
    let mut taken = vec![false; n];
    let mut keep = Vec::new();
    for i in order {
        let lo = i.saturating_sub(min_dist - 1);
        let hi = (i + min_dist).min(n);
        if (lo..hi).any(|j| taken[j]) {
            continue;
        }
        taken[i] = true;
        keep.push(i);
    }
    keep.sort_unstable();
    cands.clear();
    keep
}

pub fn db(power: f32) -> f32 {
    10.0 * (power + 1e-12).log10()
}

pub fn gaussian_smooth(x: &[f32], sigma: f32) -> Vec<f32> {
    if sigma <= 0.0 {
        return x.to_vec();
    }
    let r = (3.0 * sigma).ceil() as isize;
    let k: Vec<f32> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let ks: f32 = k.iter().sum();
    let n = x.len() as isize;
    (0..n)
        .map(|i| {
            let mut acc = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let idx = (i + j as isize - r).clamp(0, n - 1);
                acc += x[idx as usize] * kv;
            }
            acc / ks
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peaks_respect_distance() {
        let x = [0.0, 1.0, 0.0, 3.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let thr = [0.5; 9];
        assert_eq!(pick_peaks(&x, &thr, 3), vec![3, 7]);
        assert_eq!(pick_peaks(&x, &thr, 1), vec![1, 3, 7]);
    }
    #[test]
    fn lowpass_attenuates_high() {
        let sr = 44100.0;
        let hi: Vec<f32> = (0..44100)
            .map(|i| (2.0 * std::f32::consts::PI * 5000.0 * i as f32 / sr).sin())
            .collect();
        let lo: Vec<f32> = (0..44100)
            .map(|i| (2.0 * std::f32::consts::PI * 60.0 * i as f32 / sr).sin())
            .collect();
        let rms = |v: &[f32]| {
            (v[4410..].iter().map(|x| x * x).sum::<f32>() / (v.len() - 4410) as f32).sqrt()
        };
        assert!(rms(&lowpass_twice(&hi, sr, 150.0)) < 0.001);
        assert!(rms(&lowpass_twice(&lo, sr, 150.0)) > 0.6);
    }
}
