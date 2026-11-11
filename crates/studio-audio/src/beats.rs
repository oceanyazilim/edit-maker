//! Tempo estimation, beat tracking and downbeat assignment.
//!
//! * Tempo: autocorrelation of the detrended onset envelope with a metrical
//!   reinforcement term and a log-normal tempo prior (1 octave std).
//! * Beats: dynamic-programming tracker (Ellis, 2007), with weak-beat trimming.
//! * Downbeats: phase selection assuming a fixed meter (default 4/4), scoring
//!   low-frequency onsets, kick presence and broadband accents per phase.

use crate::dsp;
use crate::features::Features;
use crate::model::{AnalysisConfig, BeatEvent, PercussionEvent, TempoAlternative, TempoEstimate};

fn normalized_onset(f: &Features) -> Vec<f32> {
    let sd = dsp::std_dev(&f.onset).max(1e-6);
    f.onset.iter().map(|v| v / sd).collect()
}

fn tempo_salience(ac: &[f32], lag: f64) -> f32 {
    // Linear interpolation into the autocorrelation.
    let l0 = lag.floor() as usize;
    if l0 + 1 >= ac.len() {
        return 0.0;
    }
    let t = (lag - l0 as f64) as f32;
    ac[l0] * (1.0 - t) + ac[l0 + 1] * t
}

pub fn estimate_tempo(f: &Features, cfg: &AnalysisConfig) -> TempoEstimate {
    let fps = f.fps();
    if let Some(bpm) = cfg.bpm_override.filter(|b| *b > 20.0 && *b < 400.0) {
        return TempoEstimate {
            bpm,
            confidence: 1.0,
            alternatives: vec![],
            overridden: true,
        };
    }
    let o = normalized_onset(f);
    let trend = dsp::moving_average(&o, (fps * 0.5) as usize);
    let d: Vec<f32> = o
        .iter()
        .zip(trend.iter())
        .map(|(a, b)| (a - b).max(0.0))
        .collect();
    let min_lag = (60.0 * fps / cfg.max_bpm).floor().max(1.0) as usize;
    let max_lag = (60.0 * fps / cfg.min_bpm).ceil() as usize;
    let ac_len = (max_lag * 2 + 2).min(d.len().saturating_sub(1));
    if ac_len <= min_lag + 2 {
        return TempoEstimate {
            bpm: cfg.prior_bpm,
            confidence: 0.0,
            alternatives: vec![],
            overridden: false,
        };
    }
    let n = d.len();
    let ac: Vec<f32> = (0..ac_len)
        .map(|lag| {
            let mut s = 0f64;
            for i in 0..n - lag {
                s += (d[i] * d[i + lag]) as f64;
            }
            (s / (n - lag) as f64) as f32
        })
        .collect();

    let prior = |bpm: f64| (-0.5 * ((bpm / cfg.prior_bpm).log2()).powi(2)).exp() as f32;
    let raw = |lag: f64| tempo_salience(&ac, lag) + 0.5 * tempo_salience(&ac, lag * 2.0);
    let mut best_lag = min_lag as f64;
    let mut best = f32::MIN;
    let mut scores = Vec::new();
    for lag in min_lag..=max_lag.min(ac_len - 1) {
        let bpm = 60.0 * fps / lag as f64;
        let s = raw(lag as f64) * prior(bpm);
        scores.push(s);
        if s > best {
            best = s;
            best_lag = lag as f64;
        }
    }
    // Parabolic refinement on the weighted score.
    let li = best_lag as usize;
    if li > min_lag && li < max_lag.min(ac_len - 2) {
        let y0 = raw((li - 1) as f64) * prior(60.0 * fps / (li - 1) as f64);
        let y1 = best;
        let y2 = raw((li + 1) as f64) * prior(60.0 * fps / (li + 1) as f64);
        let denom = y0 - 2.0 * y1 + y2;
        if denom.abs() > 1e-9 {
            let delta = (0.5 * (y0 - y2) / denom).clamp(-0.5, 0.5);
            best_lag += delta as f64;
        }
    }
    let bpm = 60.0 * fps / best_lag;
    let med = dsp::median(&scores);
    let confidence = if best > 0.0 {
        ((best - med) / best).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let base = raw(best_lag).max(1e-9);
    let mut alternatives = Vec::new();
    for mult in [0.5, 2.0, 2.0 / 3.0, 1.5] {
        let alt = bpm * mult;
        if alt >= 40.0 && alt <= 300.0 {
            let lag = 60.0 * fps / alt;
            if lag + 1.0 < ac_len as f64 {
                alternatives.push(TempoAlternative {
                    bpm: alt,
                    salience: (raw(lag) / base).clamp(0.0, 2.0),
                });
            }
        }
    }
    TempoEstimate {
        bpm,
        confidence,
        alternatives,
        overridden: false,
    }
}

pub fn track_beats(f: &Features, bpm: f64, cfg: &AnalysisConfig) -> Vec<BeatEvent> {
    let n = f.n_frames;
    if n < 4 || bpm <= 0.0 {
        return vec![];
    }
    let fps = f.fps();
    let period = 60.0 * fps / bpm;
    let o = normalized_onset(f);
    let local = dsp::gaussian_smooth(&o, (period / 32.0) as f32);
    let max_local = local.iter().fold(0f32, |m, v| m.max(*v));
    if max_local <= 0.0 {
        return vec![];
    }
    let thresh = 0.01 * max_local;
    let tight = cfg.beat_tightness;
    let lo_off = (2.0 * period).round() as usize;
    let hi_off = (period / 2.0).round().max(1.0) as usize;

    let mut cum = vec![0f32; n];
    let mut back = vec![-1i64; n];
    let mut started = false;
    for i in 0..n {
        let mut best = f32::MIN;
        let mut arg = -1i64;
        if i >= hi_off {
            let lo = i.saturating_sub(lo_off);
            let hi = i - hi_off;
            for prev in lo..=hi {
                let r = ((i - prev) as f64 / period).ln();
                let s = cum[prev] - (tight * r * r) as f32;
                if s > best {
                    best = s;
                    arg = prev as i64;
                }
            }
        }
        if started && arg >= 0 {
            cum[i] = local[i] + best;
            back[i] = arg;
        } else {
            cum[i] = local[i];
            back[i] = -1;
        }
        if !started && local[i] > thresh {
            started = true;
        }
    }
    // Last beat: last local max of cum whose value is >= half the median of local maxima.
    let maxes: Vec<usize> = (1..n - 1)
        .filter(|&i| cum[i] > cum[i - 1] && cum[i] >= cum[i + 1])
        .collect();
    if maxes.is_empty() {
        return vec![];
    }
    let med = dsp::median(&maxes.iter().map(|&i| cum[i]).collect::<Vec<_>>());
    let last = *maxes
        .iter()
        .rev()
        .find(|&&i| cum[i] >= 0.5 * med)
        .unwrap_or(&maxes[maxes.len() - 1]);
    let mut frames = vec![last];
    let mut cur = last as i64;
    while back[cur as usize] >= 0 {
        cur = back[cur as usize];
        frames.push(cur as usize);
    }
    frames.reverse();

    // Trim weak leading / trailing beats.
    let strengths: Vec<f32> = frames.iter().map(|&i| local[i]).collect();
    let smooth = {
        let w = [0.25f32, 0.75, 1.0, 0.75, 0.25];
        (0..strengths.len())
            .map(|i| {
                let mut acc = 0.0;
                for (k, wv) in w.iter().enumerate() {
                    let j = i as isize + k as isize - 2;
                    if j >= 0 && (j as usize) < strengths.len() {
                        acc += strengths[j as usize] * wv;
                    }
                }
                acc
            })
            .collect::<Vec<f32>>()
    };
    // Only trims beats in (near-)silent edges; quiet but rhythmic intros are kept.
    let thr =
        0.15 * (smooth.iter().map(|v| v * v).sum::<f32>() / smooth.len().max(1) as f32).sqrt();
    let start = smooth.iter().position(|v| *v >= thr).unwrap_or(0);
    let end = smooth
        .iter()
        .rposition(|v| *v >= thr)
        .map(|e| e + 1)
        .unwrap_or(frames.len());
    let frames = &frames[start..end.max(start)];

    let smax = frames
        .iter()
        .map(|&i| local[i])
        .fold(0f32, f32::max)
        .max(1e-6);
    frames
        .iter()
        .map(|&i| BeatEvent {
            sample: f.frame_to_sample(i),
            strength: (local[i] / smax).clamp(0.0, 1.0),
            is_downbeat: false,
            bar: -1,
            beat_in_bar: 0,
        })
        .collect()
}

/// Assigns bar positions; returns a heuristic confidence (0..1).
pub fn assign_downbeats(
    beats: &mut [BeatEvent],
    f: &Features,
    kicks: &[PercussionEvent],
    bpb: u32,
) -> f32 {
    let bpb = bpb as usize;
    if beats.is_empty() {
        return 0.0;
    }
    if bpb == 1 || beats.len() < bpb * 2 {
        for (i, b) in beats.iter_mut().enumerate() {
            b.is_downbeat = i % bpb == 0;
            b.bar = (i / bpb) as i32;
            b.beat_in_bar = (i % bpb) as u8;
        }
        return 0.0;
    }
    let tol = (0.05 * f.sr as f64) as i64;
    let low_max = f.low_rise.iter().fold(0f32, |m, v| m.max(*v)).max(1e-6);
    let on_max = f.onset.iter().fold(0f32, |m, v| m.max(*v)).max(1e-6);
    let feature = |b: &BeatEvent| -> f32 {
        let fr = f.sample_to_frame(b.sample);
        let lo = fr.saturating_sub(2);
        let hi = (fr + 3).min(f.n_frames);
        let low = f.low_rise[lo..hi].iter().fold(0f32, |m, v| m.max(*v)) / low_max;
        let on = f.onset[lo..hi].iter().fold(0f32, |m, v| m.max(*v)) / on_max;
        let kick = kicks
            .iter()
            .filter(|k| (k.sample - b.sample).abs() <= tol)
            .map(|k| k.intensity * k.confidence)
            .fold(0f32, f32::max);
        low + 0.5 * kick + 0.7 * on
    };
    let accents: Vec<f32> = beats.iter().map(feature).collect();

    // Structural evidence: arrangement changes (layers entering/leaving) happen on bar
    // lines. Beat-level novelty = squared distance between mean features of the
    // `bpb` beats after and before each beat. A change aligned with a downbeat yields
    // one full-height peak; misaligned phases see only partial (squared → much smaller) peaks.
    let nb = beats.len();
    let beat_feat = |i: usize, v: &Vec<f32>| -> f32 {
        let a = f.sample_to_frame(beats[i].sample);
        let b = if i + 1 < nb {
            f.sample_to_frame(beats[i + 1].sample)
        } else {
            (a + 20).min(f.n_frames)
        };
        dsp::mean(
            &v[a..b.max(a + 1).min(f.n_frames)]
                .iter()
                .map(|x| x.max(-80.0))
                .collect::<Vec<_>>(),
        )
    };
    let series: Vec<Vec<f32>> = [&f.low_db, &f.rms_db, &f.high_db, &f.mid_db]
        .iter()
        .map(|v| {
            let raw: Vec<f32> = (0..nb).map(|i| beat_feat(i, v)).collect();
            let m = dsp::mean(&raw);
            let s = dsp::std_dev(&raw).max(0.5);
            raw.iter().map(|x| (x - m) / s).collect()
        })
        .collect();
    let mut struct_scores = vec![0f32; bpb];
    for i in bpb..nb.saturating_sub(bpb) {
        let mut d2 = 0f32;
        for s in &series {
            let after = dsp::mean(&s[i..i + bpb]);
            let before = dsp::mean(&s[i - bpb..i]);
            d2 += (after - before).powi(2);
        }
        struct_scores[i % bpb] += d2 * d2;
    }
    let mut accent_scores = vec![0f32; bpb];
    let mut counts = vec![0usize; bpb];
    for (i, a) in accents.iter().enumerate() {
        accent_scores[i % bpb] += a;
        counts[i % bpb] += 1;
    }
    for p in 0..bpb {
        accent_scores[p] /= counts[p].max(1) as f32;
    }
    let smax = struct_scores.iter().fold(0f32, |m, v| m.max(*v));
    let amax = accent_scores.iter().fold(0f32, |m, v| m.max(*v)).max(1e-6);
    let scores: Vec<f32> = (0..bpb)
        .map(|p| {
            0.4 * accent_scores[p] / amax
                + if smax > 1e-6 {
                    struct_scores[p] / smax
                } else {
                    0.0
                }
        })
        .collect();
    let mut order: Vec<usize> = (0..bpb).collect();
    order.sort_by(|a, b| scores[*b].partial_cmp(&scores[*a]).unwrap());
    let phase = order[0];
    let conf = if scores[order[0]] > 0.0 {
        ((scores[order[0]] - scores[order[1]]) / scores[order[0]]).clamp(0.0, 1.0)
    } else {
        0.0
    };
    for (i, b) in beats.iter_mut().enumerate() {
        let rel = (i + bpb - phase) % bpb;
        b.beat_in_bar = rel as u8;
        b.is_downbeat = rel == 0;
        b.bar = ((i + bpb - phase) / bpb) as i32 - if phase > 0 { 1 } else { 0 };
    }
    conf
}

/// Half-beat phase correction.
///
/// The DP tracker follows the strongest broadband onsets, which in some genres
/// (e.g. Brazilian funk / phonk with sub-heavy kicks and bright percussion) sit on
/// the off-beats. Beats conventionally sit on the kick pulse. We take strong
/// low-frequency onsets (kick proxies, independent of the kick classifier) and shift
/// the grid by half a period only when they form a *regular pulse at the beat period*
/// whose phase is concentrated around the half-beat:
///   * circular resultant length R of their phases > 0.6,
///   * mean phase within 0.5 ± 0.15,
///   * > 50 % of inter-onset intervals within ±10 % of the beat period.
/// Syncopated patterns (trap: kicks on 0, 1.5, 2.5 beats) give R ≈ 0.3 and are left alone.
/// Returns (R, mean phase, shifted).
pub fn correct_half_beat_phase(beats: &mut [BeatEvent], f: &Features) -> (f32, f32, bool) {
    if beats.len() < 16 {
        return (0.0, 0.0, false);
    }
    let norm = |v: &Vec<f32>| {
        let m = v.iter().fold(0f32, |a, b| a.max(*b)).max(1e-6);
        v.iter().map(|x| x / m).collect::<Vec<f32>>()
    };
    let lr = norm(&f.low_rise);
    let lf = norm(&f.low_flux);
    let low: Vec<f32> = lr.iter().zip(lf.iter()).map(|(a, b)| a.max(*b)).collect();
    let fps = f.fps();
    let (med, mad) = dsp::rolling_median_mad(&low, fps as usize);
    let thr: Vec<f32> = med
        .iter()
        .zip(mad.iter())
        .map(|(m, d)| (m + 2.0 * d).max(0.25))
        .collect();
    let peaks = dsp::pick_peaks(&low, &thr, ((0.1 * fps) as usize).max(1));
    // Keep kick-like peaks only: low-band dominant and near the track's loud low-band level.
    let ref_low = dsp::percentile(
        &f.low_db
            .iter()
            .copied()
            .filter(|v| *v > -90.0)
            .collect::<Vec<_>>(),
        0.95,
    );
    let times: Vec<i64> = peaks
        .iter()
        .filter(|&&p| {
            let hi = (p + 3).min(f.n_frames);
            let share = f.low_share[p..hi].iter().fold(0f32, |m, v| m.max(*v));
            let level = f.low_db[p..hi].iter().fold(-120f32, |m, v| m.max(*v));
            share > 0.6 && level > ref_low - 8.0
        })
        .map(|&p| f.frame_to_sample(p))
        .collect();
    let bs: Vec<i64> = beats.iter().map(|b| b.sample).collect();
    let (mut cx, mut cy, mut n) = (0f64, 0f64, 0usize);
    for &t in &times {
        let i = bs.partition_point(|&b| b <= t);
        if i == 0 || i >= bs.len() {
            continue;
        }
        let ph = (t - bs[i - 1]) as f64 / (bs[i] - bs[i - 1]).max(1) as f64;
        let a = std::f64::consts::TAU * ph;
        cx += a.cos();
        cy += a.sin();
        n += 1;
    }
    if n < 16 {
        return (0.0, 0.0, false);
    }
    let r = ((cx * cx + cy * cy).sqrt() / n as f64) as f32;
    let mean_phase = ((cy.atan2(cx) / std::f64::consts::TAU).rem_euclid(1.0)) as f32;
    let period = (bs[bs.len() - 1] - bs[0]) as f64 / (bs.len() - 1) as f64;
    let iois: Vec<f64> = times.windows(2).map(|w| (w[1] - w[0]) as f64).collect();
    let regular = iois
        .iter()
        .filter(|d| (**d / period - 1.0).abs() < 0.1)
        .count() as f32
        / iois.len().max(1) as f32;
    let shift = r > 0.6 && (mean_phase - 0.5).abs() < 0.15 && regular > 0.5;
    if shift {
        let mids: Vec<i64> = bs.windows(2).map(|w| (w[0] + w[1]) / 2).collect();
        let last_period = bs[bs.len() - 1] - bs[bs.len() - 2];
        let last = bs[bs.len() - 1] + last_period / 2;
        for (b, s) in beats
            .iter_mut()
            .zip(mids.into_iter().chain(std::iter::once(last)))
        {
            b.sample = s;
        }
    }
    (r, mean_phase, shift)
}

/// Double-time correction: when detected kicks form a regular pulse every *two* beats
/// (≥ 60 % of kick intervals within ±10 % of 2 beat periods, and almost none at one
/// period), the grid is running at twice the musical tempo. Keep every other beat,
/// choosing the phase that carries the kicks. Returns true when halved.
pub fn halve_if_double_time(
    beats: &mut Vec<BeatEvent>,
    kicks: &[PercussionEvent],
    sr: u32,
) -> bool {
    if beats.len() < 16 || kicks.len() < 12 {
        return false;
    }
    let period =
        (beats[beats.len() - 1].sample - beats[0].sample) as f64 / (beats.len() - 1) as f64;
    let iois: Vec<f64> = kicks
        .windows(2)
        .map(|w| (w[1].sample - w[0].sample) as f64 / period)
        .collect();
    let near = |x: f64, k: f64| (x / k - 1.0).abs() < 0.1;
    let two = iois.iter().filter(|x| near(**x, 2.0)).count() as f64 / iois.len() as f64;
    let one = iois.iter().filter(|x| near(**x, 1.0)).count() as f64 / iois.len() as f64;
    if two < 0.6 || one > 0.15 {
        return false;
    }
    let tol = (0.05 * sr as f64) as i64;
    let hits = |phase: usize| {
        beats
            .iter()
            .skip(phase)
            .step_by(2)
            .filter(|b| kicks.iter().any(|k| (k.sample - b.sample).abs() <= tol))
            .count()
    };
    let phase = if hits(1) > hits(0) { 1 } else { 0 };
    let kept: Vec<BeatEvent> = beats.iter().skip(phase).step_by(2).cloned().collect();
    *beats = kept;
    true
}

pub fn bpm_from_beats(beats: &[BeatEvent], sr: u32) -> Option<f64> {
    if beats.len() < 4 {
        return None;
    }
    let ibis: Vec<f32> = beats
        .windows(2)
        .map(|w| (w[1].sample - w[0].sample) as f32)
        .collect();
    let m = dsp::median(&ibis) as f64;
    // Refine using the mean of intervals within 10% of the median.
    let close: Vec<f64> = ibis
        .iter()
        .map(|v| *v as f64)
        .filter(|v| (v - m).abs() < 0.1 * m)
        .collect();
    let mean = close.iter().sum::<f64>() / close.len().max(1) as f64;
    (mean > 0.0).then(|| 60.0 * sr as f64 / mean)
}
