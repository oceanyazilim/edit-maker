//! Kick and snare candidate detection.
//!
//! Kicks are **not** "every bass peak". A candidate must be a local peak of the
//! low-band (30–160 Hz) level *rise* above an adaptive threshold, and is then
//! scored on:
//!   * transient sharpness – attack contrast of the low-passed time-domain
//!     envelope (sustained / distorted basslines have weak attacks),
//!   * band contrast       – share of spectral power in the low band,
//!   * prominence          – height above the adaptive threshold,
//!   * rhythmic context    – alignment with the beat grid (beat / 8th / 16th).
//! Positions are refined to sample accuracy at the steepest envelope rise.

use crate::dsp;
use crate::features::Features;
use crate::model::{AnalysisConfig, BeatEvent, PercussionEvent};

/// Grid alignment score for a sample position given beat positions.
pub fn grid_alignment(sample: i64, beats: &[i64], sr: u32) -> f32 {
    if beats.len() < 2 {
        return 0.5;
    }
    let idx = beats.partition_point(|&b| b <= sample);
    let (a, b) = if idx == 0 {
        (beats[0], beats[1])
    } else if idx >= beats.len() {
        (beats[beats.len() - 2], beats[beats.len() - 1])
    } else {
        (beats[idx - 1], beats[idx])
    };
    let period = (b - a).max(1) as f64;
    let phase = ((sample - a) as f64 / period).rem_euclid(1.0);
    let tol_frac = (0.035 * sr as f64 / period).max(0.02);
    let dist = |grid: f64| {
        let k = (phase / grid).round();
        (phase - k * grid).abs()
    };
    if dist(1.0) <= tol_frac {
        1.0
    } else if dist(0.5) <= tol_frac {
        0.8
    } else if dist(0.25) <= tol_frac {
        0.6
    } else {
        0.15
    }
}

/// Sample index of the steepest rise of `env` within [lo, hi).
fn steepest_rise(env: &[f32], lo: usize, hi: usize, step: usize) -> usize {
    let mut best = lo;
    let mut best_d = f32::MIN;
    let hi = hi.min(env.len().saturating_sub(step));
    for t in lo..hi {
        let d = env[t + step] - env[t];
        if d > best_d {
            best_d = d;
            best = t;
        }
    }
    best
}

struct Attack {
    /// Sample where the envelope crosses 10% of the rise (refined onset).
    onset: usize,
    /// Time from 10% to 50% of the rise.
    rise_ms: f32,
    attack_db: f32,
}

/// Measures the attack around the steepest-rise point `steep`.
fn attack_profile(env: &[f32], steep: usize, sr: u32) -> Option<Attack> {
    let ms = |x: f32| (x * sr as f32 / 1000.0) as usize;
    let lo = steep.saturating_sub(ms(15.0));
    let hi = (steep + ms(50.0)).min(env.len());
    if hi <= steep + 2 || steep <= lo {
        return None;
    }
    let base = env[lo..=steep].iter().fold(f32::MAX, |m, v| m.min(*v));
    let peak = env[steep..hi].iter().fold(0f32, |m, v| m.max(*v));
    if peak <= base * 1.05 || peak < 1e-5 {
        return None;
    }
    let l10 = base + 0.1 * (peak - base);
    let l50 = base + 0.5 * (peak - base);
    // Walk back from the steepest point to the 10% crossing, forward to the 90% crossing.
    let mut t10 = steep;
    while t10 > lo && env[t10] > l10 {
        t10 -= 1;
    }
    let mut t50 = t10;
    while t50 < hi - 1 && env[t50] < l50 {
        t50 += 1;
    }
    Some(Attack {
        onset: t10,
        rise_ms: (t50 - t10) as f32 * 1000.0 / sr as f32,
        attack_db: 20.0 * ((peak + 1e-6) / (base + 1e-6)).log10(),
    })
}

/// Rectified, smoothed envelope (1 ms attack, 10 ms release).
fn envelope(x: &[f32], sr: u32) -> Vec<f32> {
    let att = (-1.0 / (0.001 * sr as f32)).exp();
    let rel = (-1.0 / (0.010 * sr as f32)).exp();
    let mut e = 0f32;
    x.iter()
        .map(|&s| {
            let a = s.abs();
            let c = if a > e { att } else { rel };
            e = c * e + (1.0 - c) * a;
            e
        })
        .collect()
}

pub fn detect_kicks(
    f: &Features,
    lowpassed: &[f32],
    beats: &[i64],
    cfg: &AnalysisConfig,
) -> Vec<PercussionEvent> {
    let sr = f.sr;
    let fps = f.fps();
    let (med, mad) = dsp::rolling_median_mad(&f.low_rise, (fps * 1.0) as usize);
    let sens = cfg.kick_sensitivity.clamp(0.0, 1.0);
    let k = 3.0 - 2.0 * sens; // 3 MAD (strict) … 1 MAD (permissive)
    let thr: Vec<f32> = med
        .iter()
        .zip(mad.iter())
        .map(|(m, d)| m + k * d + 2.5)
        .collect();
    let min_dist = ((0.08 * fps).round() as usize).max(1);
    // Candidate source 1: low-band level rises.
    let mut peaks = dsp::pick_peaks(&f.low_rise, &thr, min_dist);
    // Candidate source 2: low-band spectral change (kicks whose predecessor's tail keeps the
    // band level constant, e.g. long distorted kicks, still reset their pitch).
    let (fmed, fmad) = dsp::rolling_median_mad(&f.low_flux, (fps * 1.0) as usize);
    let fthr: Vec<f32> = fmed
        .iter()
        .zip(fmad.iter())
        .map(|(m, d)| m + (k + 1.0) * d + 0.5)
        .collect();
    let flux_peaks = dsp::pick_peaks(&f.low_flux, &fthr, min_dist);
    for fp in flux_peaks {
        if !peaks.iter().any(|&p| (p as isize - fp as isize).abs() <= 2) {
            peaks.push(fp);
        }
    }
    peaks.sort_unstable();
    let env = envelope(lowpassed, sr);
    let max_low = f.low_db.iter().fold(-120f32, |m, v| m.max(*v));
    let min_conf = cfg.kick_min_confidence.unwrap_or(0.65 - 0.30 * sens);
    // Minimum loudness relative to the loudest low-band moment of the track.
    let intensity_floor = 0.25 - 0.2 * sens;

    let ms = |x: f64| (x * sr as f64 / 1000.0) as i64;
    let mut out: Vec<PercussionEvent> = Vec::new();
    for p in peaks {
        let center = f.frame_to_sample(p);
        // Search the onset in a window around the frame (frames lead the true onset slightly).
        let lo = (center - ms(30.0)).max(0) as usize;
        let hi = ((center + ms(30.0)).max(0) as usize).min(env.len());
        if hi <= lo + 8 {
            continue;
        }
        let step = ms(2.0).max(1) as usize;
        let steep = steepest_rise(&env, lo, hi, step);
        let Some(attack) = attack_profile(&env, steep, sr) else {
            continue;
        };
        // Rise time (10% -> 50% of the local peak): real kicks get there within a few ms even
        // when their pitch sweep keeps the low-band level growing afterwards; amplitude-modulated
        // basslines, kick-tail/bass beating and fades take tens of ms.
        let rise_score = ((16.0 - attack.rise_ms) / 8.0).clamp(0.0, 1.0);
        if rise_score <= 0.0 {
            continue;
        }
        let attack_score = ((attack.attack_db - 3.0) / 12.0).clamp(0.0, 1.0);
        let sharpness = rise_score * (0.4 + 0.6 * attack_score);
        let onset = attack.onset;

        let fr_hi = (p + 2).min(f.n_frames);
        let level = f.low_db[p..fr_hi].iter().fold(-120f32, |m, v| m.max(*v));
        let intensity = ((level - (max_low - 30.0)) / 30.0).clamp(0.0, 1.0);
        if intensity < intensity_floor {
            continue;
        }
        let share = f.low_share[p..fr_hi].iter().fold(0f32, |m, v| m.max(*v));
        let contrast = ((share - 0.15) / 0.5).clamp(0.0, 1.0);
        let prominence = ((f.low_rise[p] - thr[p]) / (thr[p] + 1e-3))
            .max((f.low_flux[p] - fthr[p]) / (fthr[p] + 1e-3))
            .clamp(0.0, 1.0);
        let rhythm = grid_alignment(onset as i64, beats, sr);

        // Transient breadth: a real kick's attack (beater click, pitch sweep) also raises the
        // 160–2000 Hz band. Low-only rises – e.g. a decaying kick tail beating against a
        // sustained bass, or distorted bass modulation – do not. Used as a multiplicative gate.
        let midrise = (p.saturating_sub(1)..(p + 2).min(f.n_frames))
            .map(|i| {
                (f.mid_db[i] - f.mid_db[i.saturating_sub(2)].min(f.mid_db[i.saturating_sub(1)]))
                    .max(0.0)
            })
            .fold(0f32, f32::max);
        let breadth = ((midrise - 1.5) / 6.0).clamp(0.0, 1.0);

        // Product of evidence terms: any single weak cue (no attack, no low-band dominance,
        // no broadband transient, barely above threshold, quiet, off-grid) lowers the score;
        // strong cues cannot compensate for a missing one.
        let confidence = (0.35 + 0.65 * sharpness)
            * (0.65 + 0.35 * breadth)
            * (0.30 + 0.70 * contrast)
            * (0.60 + 0.40 * prominence)
            * (0.40 + 0.60 * intensity)
            * (0.40 + 0.60 * rhythm);
        if std::env::var_os("STUDIO_DEBUG_KICKS").is_some() {
            eprintln!(
                "cand {:.3}s rise {:.1}ms att {:.1}dB share {:.2} prom {:.2} rhy {:.2} int {:.2} midrise {:.1} conf {:.2}",
                onset as f64 / sr as f64, attack.rise_ms, attack.attack_db, share, prominence, rhythm, intensity, midrise, confidence
            );
        }
        // Rhythmic rescue: a loud, low-band dominant onset exactly on the beat grid is a kick
        // even when a sustained bass masks its attack (common with 808-style funk/phonk mixes).
        let rescued = rhythm >= 1.0
            && intensity >= 0.75
            && share >= 0.6
            && attack.rise_ms <= 8.0
            && attack.attack_db >= 4.5
            && confidence >= 0.25;
        if confidence < min_conf && !rescued {
            continue;
        }
        out.push(PercussionEvent {
            sample: onset as i64,
            intensity,
            confidence: confidence.clamp(0.0, 1.0),
        });
    }
    // Refinement can move two candidates together: keep the stronger.
    out.sort_by_key(|e| e.sample);
    let mut dedup: Vec<PercussionEvent> = Vec::with_capacity(out.len());
    for e in out {
        if let Some(last) = dedup.last_mut() {
            if e.sample - last.sample < ms(60.0) {
                if e.confidence > last.confidence {
                    *last = e;
                }
                continue;
            }
        }
        dedup.push(e);
    }
    dedup
}

/// Snare candidates: rises in the 1–5 kHz band, scored multiplicatively on
///   * relative level – distance to the track's loud snare-band moments (99.5th
///     percentile); hi-hats and kick clicks are far quieter in this band,
///   * rhythmic role – back-beat (beats 2 and 4) > other beats > off-grid.
/// These are candidates for editing decisions, not a drum transcription.
pub fn detect_snares(
    f: &Features,
    beats: &[BeatEvent],
    cfg: &AnalysisConfig,
) -> Vec<PercussionEvent> {
    let fps = f.fps();
    let (med, mad) = dsp::rolling_median_mad(&f.snare_rise, (fps * 1.0) as usize);
    let sens = cfg.kick_sensitivity.clamp(0.0, 1.0);
    let thr: Vec<f32> = med
        .iter()
        .zip(mad.iter())
        .map(|(m, d)| m + (3.0 - 1.5 * sens) * d + 3.0)
        .collect();
    let peaks = dsp::pick_peaks(&f.snare_rise, &thr, ((0.1 * fps) as usize).max(1));
    let beat_samples: Vec<i64> = beats.iter().map(|b| b.sample).collect();
    let tol = (0.05 * f.sr as f64) as i64;
    let active: Vec<f32> = f.snare_db.iter().copied().filter(|v| *v > -90.0).collect();
    let reference = dsp::percentile(&active, 0.995);
    let min_conf = 0.55 - 0.2 * sens;
    peaks
        .into_iter()
        .filter_map(|p| {
            let s = f.frame_to_sample(p);
            let hi = (p + 2).min(f.n_frames);
            let level = f.snare_db[p..hi].iter().fold(-120f32, |m, v| m.max(*v)) - reference;
            let level_term = ((level + 18.0) / 10.0).clamp(0.0, 1.0);
            let backbeat = beats
                .iter()
                .find(|b| (b.sample - s).abs() <= tol)
                .map(|b| {
                    if b.bar >= 0 && (b.beat_in_bar == 1 || b.beat_in_bar == 3) {
                        1.0
                    } else {
                        0.6
                    }
                })
                .unwrap_or_else(|| grid_alignment(s, &beat_samples, f.sr) * 0.5);
            let conf = level_term * (0.5 + 0.5 * backbeat);
            if std::env::var_os("STUDIO_DEBUG_SNARES").is_some() {
                eprintln!(
                    "sn {:.3}s level {:.1} back {:.2} conf {:.2}",
                    s as f64 / f.sr as f64,
                    level,
                    backbeat,
                    conf
                );
            }
            (conf >= min_conf).then(|| PercussionEvent {
                sample: s,
                intensity: level_term,
                confidence: conf.clamp(0.0, 1.0),
            })
        })
        .collect()
}
