//! Min/max waveform peaks for UI rendering.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WaveformPeaks {
    pub sample_rate: u32,
    pub samples_per_bucket: u32,
    /// Interleaved [min, max] per bucket, quantized to i8 (-127..127).
    pub peaks: Vec<i8>,
}

pub fn compute_peaks(samples: &[f32], sample_rate: u32, buckets_per_second: u32) -> WaveformPeaks {
    let spb = (sample_rate / buckets_per_second.max(1)).max(1) as usize;
    let mut peaks = Vec::with_capacity(samples.len() / spb * 2 + 2);
    for chunk in samples.chunks(spb) {
        let (mut lo, mut hi) = (0f32, 0f32);
        for &s in chunk {
            lo = lo.min(s);
            hi = hi.max(s);
        }
        peaks.push((lo.clamp(-1.0, 1.0) * 127.0).round() as i8);
        peaks.push((hi.clamp(-1.0, 1.0) * 127.0).round() as i8);
    }
    WaveformPeaks {
        sample_rate,
        samples_per_bucket: spb as u32,
        peaks,
    }
}
