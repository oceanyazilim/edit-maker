//! Short-time spectral features.
//!
//! Frames are *centered*: frame `i` is centered on sample `i * hop`.
//! FFT size is ~46 ms (2048 @ 44.1/48 kHz), hop = FFT/4 (~11.6 ms).

use rayon::prelude::*;
use realfft::RealFftPlanner;
use studio_core::{CancelToken, StudioResult};

use crate::dsp::{self, db};
use crate::model::{AnalysisCurves, AudioSummary, SilenceRegion};

pub const N_BANDS: usize = 64;

pub struct Features {
    pub sr: u32,
    pub n_fft: usize,
    pub hop: usize,
    pub n_frames: usize,
    pub rms_db: Vec<f32>,
    /// 30–160 Hz (kick fundamental region).
    pub low_db: Vec<f32>,
    /// 160–2000 Hz.
    pub mid_db: Vec<f32>,
    /// 2–11 kHz.
    pub high_db: Vec<f32>,
    /// 1–5 kHz (snare noise / crack region).
    pub snare_db: Vec<f32>,
    /// > 7 kHz (hats / cymbals).
    pub hat_db: Vec<f32>,
    /// Fraction of total power in the low band.
    pub low_share: Vec<f32>,
    /// Broadband log-band spectral flux (onset strength).
    pub onset: Vec<f32>,
    /// Positive rise of low-band level (dB), max over lag 1..2 frames.
    pub low_rise: Vec<f32>,
    /// Bin-wise positive log-magnitude flux within 30–160 Hz: captures low-frequency
    /// spectral change (e.g. a kick pitch reset) even when the band level stays constant.
    pub low_flux: Vec<f32>,
    pub snare_rise: Vec<f32>,
    pub centroid_hz: Vec<f32>,
}

impl Features {
    pub fn fps(&self) -> f64 {
        self.sr as f64 / self.hop as f64
    }
    pub fn frame_to_sample(&self, i: usize) -> i64 {
        (i * self.hop) as i64
    }
    pub fn sample_to_frame(&self, s: i64) -> usize {
        ((s.max(0) as f64 / self.hop as f64).round() as usize).min(self.n_frames.saturating_sub(1))
    }
    pub fn curves(&self) -> AnalysisCurves {
        let r = |v: &Vec<f32>| v.iter().map(|x| (x * 100.0).round() / 100.0).collect();
        AnalysisCurves {
            hop_samples: self.hop as u32,
            rms_db: r(&self.rms_db),
            low_db: r(&self.low_db),
            mid_db: r(&self.mid_db),
            high_db: r(&self.high_db),
            onset: r(&self.onset),
            centroid_hz: self.centroid_hz.iter().map(|x| x.round()).collect(),
        }
    }
}

const MAX_LOW_BINS: usize = 24;

struct FrameOut {
    bands: [f32; N_BANDS],
    low_mag: [f32; MAX_LOW_BINS],
    rms_power: f32,
    low: f32,
    mid: f32,
    high: f32,
    snare: f32,
    hat: f32,
    total: f32,
    centroid: f32,
}

fn hz_to_mel(f: f32) -> f32 {
    2595.0 * (1.0 + f / 700.0).log10()
}

pub fn compute(samples: &[f32], sr: u32, cancel: &CancelToken) -> StudioResult<Features> {
    let n_fft: usize = if sr >= 32000 {
        2048
    } else if sr >= 16000 {
        1024
    } else {
        512
    };
    let hop = n_fft / 4;
    let n_frames = samples.len() / hop + 1;
    let peak = samples.iter().fold(0f32, |m, &s| m.max(s.abs())).max(1e-6);
    let gain = 1.0 / peak;

    let window: Vec<f32> = (0..n_fft)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n_fft as f32).cos())
        .collect();
    let win_sum: f32 = window.iter().sum();
    let win_sq: f32 = window.iter().map(|w| w * w).sum();
    // Power normalization so that band power ≈ mean-square contribution of that band.
    let pnorm = 2.0 / (n_fft as f32 * win_sq);
    let mag_norm = 2.0 / win_sum;

    let bin_hz = sr as f32 / n_fft as f32;
    let n_bins = n_fft / 2 + 1;
    let fmin = 30.0f32;
    let fmax = (sr as f32 / 2.0).min(11025.0);
    let (mmin, mmax) = (hz_to_mel(fmin), hz_to_mel(fmax));
    // Bin → mel band mapping (None = outside range).
    let band_of: Vec<Option<usize>> = (0..n_bins)
        .map(|k| {
            let f = k as f32 * bin_hz;
            if f < fmin || f > fmax {
                None
            } else {
                Some(
                    (((hz_to_mel(f) - mmin) / (mmax - mmin)) * N_BANDS as f32)
                        .min(N_BANDS as f32 - 1.0) as usize,
                )
            }
        })
        .collect();
    let in_range = |k: usize, lo: f32, hi: f32| {
        let f = k as f32 * bin_hz;
        f >= lo && f < hi
    };
    let low_bins: Vec<usize> = (0..n_bins).filter(|&k| in_range(k, 30.0, 160.0)).collect();
    let flux_bins: Vec<usize> = (0..n_bins)
        .filter(|&k| in_range(k, 30.0, 160.0))
        .take(MAX_LOW_BINS)
        .collect();

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n_fft);
    let half = (n_fft / 2) as i64;

    let chunk = 512;
    let mut outs: Vec<FrameOut> = Vec::with_capacity(n_frames);
    for start in (0..n_frames).step_by(chunk * 8) {
        cancel.check()?;
        let end = (start + chunk * 8).min(n_frames);
        let part: Vec<FrameOut> = (start..end)
            .into_par_iter()
            .map_init(
                || {
                    (
                        fft.make_input_vec(),
                        fft.make_output_vec(),
                        fft.make_scratch_vec(),
                    )
                },
                |(inp, out, scratch), i| {
                    let center = (i * hop) as i64;
                    for (j, slot) in inp.iter_mut().enumerate() {
                        let idx = center - half + j as i64;
                        let s = if idx >= 0 && (idx as usize) < samples.len() {
                            samples[idx as usize] * gain
                        } else {
                            0.0
                        };
                        *slot = s * window[j];
                    }
                    let _ = fft.process_with_scratch(inp, out, scratch);
                    let mut bands = [0f32; N_BANDS];
                    let (mut low, mut mid, mut high, mut snare, mut hat, mut total, mut cnum) =
                        (0f32, 0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
                    for (k, c) in out.iter().enumerate() {
                        let p = c.norm_sqr() * pnorm;
                        let f = k as f32 * bin_hz;
                        if let Some(b) = band_of[k] {
                            bands[b] += p;
                        }
                        if f >= 20.0 {
                            total += p;
                            cnum += p * f;
                        }
                        if (160.0..2000.0).contains(&f) {
                            mid += p;
                        } else if (2000.0..11000.0).contains(&f) {
                            high += p;
                        }
                        if (1000.0..5000.0).contains(&f) {
                            snare += p;
                        }
                        if f >= 7000.0 {
                            hat += p;
                        }
                    }
                    for &k in &low_bins {
                        low += out[k].norm_sqr() * pnorm;
                    }
                    let mut low_mag = [0f32; MAX_LOW_BINS];
                    for (j, &k) in flux_bins.iter().enumerate() {
                        low_mag[j] = (1.0 + 1000.0 * out[k].norm() * mag_norm).ln();
                    }
                    // Band magnitudes for log-compressed flux.
                    for b in bands.iter_mut() {
                        *b = (1.0 + 1000.0 * (b.sqrt() * mag_norm / pnorm.sqrt() * 0.5)).ln();
                    }
                    // Time-domain RMS over the hop centered on the frame.
                    let lo = (center - (hop as i64) / 2).max(0) as usize;
                    let hi = ((center + (hop as i64) / 2).max(0) as usize).min(samples.len());
                    let rms_power = if hi > lo {
                        samples[lo..hi].iter().map(|s| s * s).sum::<f32>() / (hi - lo) as f32
                    } else {
                        0.0
                    };
                    FrameOut {
                        bands,
                        low_mag,
                        rms_power,
                        low,
                        mid,
                        high,
                        snare,
                        hat,
                        total,
                        centroid: if total > 1e-10 { cnum / total } else { 0.0 },
                    }
                },
            )
            .collect();
        outs.extend(part);
    }

    let mut f = Features {
        sr,
        n_fft,
        hop,
        n_frames,
        rms_db: Vec::with_capacity(n_frames),
        low_db: Vec::with_capacity(n_frames),
        mid_db: Vec::with_capacity(n_frames),
        high_db: Vec::with_capacity(n_frames),
        snare_db: Vec::with_capacity(n_frames),
        hat_db: Vec::with_capacity(n_frames),
        low_share: Vec::with_capacity(n_frames),
        onset: vec![0.0; n_frames],
        low_rise: vec![0.0; n_frames],
        low_flux: vec![0.0; n_frames],
        snare_rise: vec![0.0; n_frames],
        centroid_hz: Vec::with_capacity(n_frames),
    };
    // Spectral levels are on the peak-normalized signal; add back the gain so dB ≈ dBFS.
    let gdb = 20.0 * peak.log10();
    for o in &outs {
        f.rms_db.push(db(o.rms_power));
        f.low_db.push(db(o.low) + gdb);
        f.mid_db.push(db(o.mid) + gdb);
        f.high_db.push(db(o.high) + gdb);
        f.snare_db.push(db(o.snare) + gdb);
        f.hat_db.push(db(o.hat) + gdb);
        f.low_share.push(if o.total > 1e-10 {
            o.low / o.total
        } else {
            0.0
        });
        f.centroid_hz.push(o.centroid);
    }
    for i in 1..n_frames {
        let (a, b) = (&outs[i - 1].bands, &outs[i].bands);
        f.onset[i] = a.iter().zip(b.iter()).map(|(p, c)| (c - p).max(0.0)).sum();
        f.low_flux[i] = outs[i - 1]
            .low_mag
            .iter()
            .zip(outs[i].low_mag.iter())
            .take(flux_bins.len())
            .map(|(p, c)| (c - p).max(0.0))
            .sum();
        let floor = -80.0;
        let rise = |v: &Vec<f32>| {
            let cur = v[i].max(floor);
            let prev1 = v[i - 1].max(floor);
            let prev2 = if i >= 2 { v[i - 2].max(floor) } else { prev1 };
            (cur - prev1.min(prev2)).max(0.0)
        };
        f.low_rise[i] = rise(&f.low_db);
        f.snare_rise[i] = rise(&f.snare_db);
    }
    Ok(f)
}

/// Contiguous regions where frame RMS is below `thr_db` for at least `min_secs`.
pub fn detect_silence(f: &Features, sr: u32, thr_db: f32, min_secs: f64) -> Vec<SilenceRegion> {
    let min_frames = (min_secs * f.fps()).ceil() as usize;
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for i in 0..=f.n_frames {
        let silent = i < f.n_frames && f.rms_db[i] < thr_db;
        match (silent, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= min_frames {
                    out.push(SilenceRegion {
                        start_sample: f.frame_to_sample(s),
                        end_sample: f.frame_to_sample(i).min((f.n_frames * f.hop) as i64),
                    });
                }
                start = None;
            }
            _ => {}
        }
    }
    let _ = sr;
    out
}

pub fn summary(f: &Features, _sr: u32, n_samples: usize) -> AudioSummary {
    let active: Vec<f32> = f.rms_db.iter().copied().filter(|v| *v > -70.0).collect();
    let mean_power: f32 = if active.is_empty() {
        0.0
    } else {
        active.iter().map(|d| 10f32.powf(d / 10.0)).sum::<f32>() / active.len() as f32
    };
    let peak = f.rms_db.iter().fold(-120f32, |m, v| m.max(*v));
    let onset_thr: Vec<f32> = {
        let (med, mad) = dsp::rolling_median_mad(&f.onset, (f.fps() * 1.0) as usize);
        med.iter()
            .zip(mad.iter())
            .map(|(m, d)| m + 2.0 * d + 1e-3)
            .collect()
    };
    let onsets = dsp::pick_peaks(&f.onset, &onset_thr, (0.05 * f.fps()).max(1.0) as usize);
    let dur = n_samples as f32 / f.sr as f32;
    AudioSummary {
        peak_dbfs: peak,
        rms_dbfs: db(mean_power),
        dynamic_contrast_db: dsp::percentile(&active, 0.95) - dsp::percentile(&active, 0.10),
        rhythmic_density: if dur > 0.0 {
            onsets.len() as f32 / dur
        } else {
            0.0
        },
        mean_centroid_hz: dsp::mean(&f.centroid_hz),
    }
}
