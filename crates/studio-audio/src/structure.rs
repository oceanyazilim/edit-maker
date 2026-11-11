//! Musical structure (sections) and drop candidate analysis at bar resolution.
//!
//! Bar features (RMS, low-band level, high-band level, onset density, kick rate)
//! are z-normalized; section boundaries are peaks of a two-scale
//! "difference of means" novelty with a phrase-grid (4/8 bar) bonus.
//! A drop is modelled as a *transition*: energy contrast across the boundary,
//! low-frequency re-entry, preceding build-up, short pre-drop dip, post-boundary
//! density and whether the new energy is sustained.

use crate::dsp;
use crate::features::Features;
use crate::model::{
    BeatEvent, DropCandidate, DropFeatures, DropKind, MusicalSection, PercussionEvent, SectionKind,
};

pub struct StructureResult {
    pub sections: Vec<MusicalSection>,
    pub drops: Vec<DropCandidate>,
}

struct Bars {
    starts: Vec<i64>, // sample positions; bar i spans starts[i]..starts[i+1]
    /// Index of the first bar that begins a phrase grid (aligned to downbeat 0).
    beats_per_bar: usize,
}

fn build_bars(beats: &[BeatEvent], n_samples: i64, sr: u32) -> Bars {
    let downs: Vec<i64> = beats
        .iter()
        .filter(|b| b.is_downbeat)
        .map(|b| b.sample)
        .collect();
    if downs.len() >= 4 {
        let mut starts = vec![0i64];
        let bar_len = (downs[downs.len() - 1] - downs[0]) / (downs.len() as i64 - 1).max(1);
        // Extrapolate bars before the first detected downbeat.
        let mut pre = Vec::new();
        let mut t = downs[0] - bar_len;
        while t > bar_len / 2 {
            pre.push(t);
            t -= bar_len;
        }
        pre.reverse();
        starts.extend(pre);
        starts.extend(downs.iter().copied().filter(|&s| s > 0));
        let mut t = *downs.last().unwrap() + bar_len;
        while t < n_samples - bar_len / 2 {
            starts.push(t);
            t += bar_len;
        }
        starts.push(n_samples);
        starts.dedup();
        return Bars {
            starts,
            beats_per_bar: 4,
        };
    }
    // Fallback: 2-second pseudo-bars.
    let step = (2 * sr) as i64;
    let mut starts: Vec<i64> = (0..)
        .map(|i| i * step)
        .take_while(|&s| s < n_samples)
        .collect();
    starts.push(n_samples);
    Bars {
        starts,
        beats_per_bar: 4,
    }
}

fn zscore(v: &[f32]) -> Vec<f32> {
    let m = dsp::mean(v);
    let s = dsp::std_dev(v).max(1e-3);
    v.iter().map(|x| (x - m) / s).collect()
}

fn slope(v: &[f32]) -> f32 {
    let n = v.len();
    if n < 2 {
        return 0.0;
    }
    let xm = (n - 1) as f32 / 2.0;
    let ym = dsp::mean(v);
    let (mut num, mut den) = (0.0, 0.0);
    for (i, y) in v.iter().enumerate() {
        let dx = i as f32 - xm;
        num += dx * (y - ym);
        den += dx * dx;
    }
    if den > 0.0 {
        num / den
    } else {
        0.0
    }
}

pub fn analyze_structure(
    f: &Features,
    beats: &[BeatEvent],
    kicks: &[PercussionEvent],
    n_samples: i64,
) -> StructureResult {
    let bars = build_bars(beats, n_samples, f.sr);
    let nb = bars.starts.len() - 1;
    if nb < 3 {
        return StructureResult {
            sections: vec![MusicalSection {
                start_sample: 0,
                end_sample: n_samples,
                kind: SectionKind::Groove,
                energy: 0.5,
                energy_slope: 0.0,
                confidence: 0.1,
            }],
            drops: vec![],
        };
    }
    let fps = f.fps();
    let onset_thr: Vec<f32> = {
        let (med, mad) = dsp::rolling_median_mad(&f.onset, fps as usize);
        med.iter()
            .zip(mad.iter())
            .map(|(m, d)| m + 2.0 * d + 1e-3)
            .collect()
    };
    let onset_peaks = dsp::pick_peaks(&f.onset, &onset_thr, ((0.05 * fps) as usize).max(1));

    let mut rms = Vec::with_capacity(nb);
    let mut low = Vec::with_capacity(nb);
    let mut high = Vec::with_capacity(nb);
    let mut dens = Vec::with_capacity(nb);
    let mut kick_rate = Vec::with_capacity(nb);
    for i in 0..nb {
        let (s0, s1) = (bars.starts[i], bars.starts[i + 1]);
        let a = f.sample_to_frame(s0);
        let b = f.sample_to_frame(s1).max(a + 1).min(f.n_frames);
        let clampv = |v: &[f32]| v.iter().map(|x| x.max(-80.0)).collect::<Vec<f32>>();
        rms.push(dsp::mean(&clampv(&f.rms_db[a..b])));
        low.push(dsp::mean(&clampv(&f.low_db[a..b])));
        high.push(dsp::mean(&clampv(&f.high_db[a..b])));
        let secs = ((s1 - s0) as f32 / f.sr as f32).max(1e-3);
        dens.push(onset_peaks.iter().filter(|&&p| p >= a && p < b).count() as f32 / secs);
        kick_rate.push(
            kicks
                .iter()
                .filter(|k| k.sample >= s0 && k.sample < s1)
                .count() as f32
                / secs,
        );
    }
    let (zr, zl, zh, zd, zk) = (
        zscore(&rms),
        zscore(&low),
        zscore(&high),
        zscore(&dens),
        zscore(&kick_rate),
    );
    let energy_raw: Vec<f32> = (0..nb)
        .map(|i| 0.35 * zr[i] + 0.30 * zl[i] + 0.15 * zd[i] + 0.20 * zk[i])
        .collect();
    let (emin, emax) = energy_raw
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let energy: Vec<f32> = energy_raw
        .iter()
        .map(|v| (v - emin) / (emax - emin).max(1e-3))
        .collect();
    let vecs: Vec<[f32; 5]> = (0..nb)
        .map(|i| [zr[i], zl[i], zh[i], zd[i], zk[i]])
        .collect();

    // Novelty at bar boundaries.
    let mean_vec = |a: usize, b: usize| -> [f32; 5] {
        let mut m = [0f32; 5];
        let n = (b - a).max(1) as f32;
        for v in &vecs[a..b] {
            for k in 0..5 {
                m[k] += v[k] / n;
            }
        }
        m
    };
    let dist = |x: [f32; 5], y: [f32; 5]| {
        x.iter()
            .zip(y.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt()
    };
    let mut nov = vec![0f32; nb];
    for b in 1..nb {
        let mut s = 0.0;
        for (l, w) in [(4usize, 1.0f32), (2, 0.5)] {
            let a0 = b.saturating_sub(l);
            let b1 = (b + l).min(nb);
            if b - a0 >= 1 && b1 - b >= 1 {
                s += w * dist(mean_vec(a0, b), mean_vec(b, b1));
            }
        }
        // Phrase bonus.
        let mult = if b % 8 == 0 {
            1.3
        } else if b % 4 == 0 {
            1.15
        } else {
            1.0
        };
        nov[b] = s * mult;
    }
    let nm = dsp::mean(&nov[1..]);
    let ns = dsp::std_dev(&nov[1..]);
    let thr = vec![nm + 0.5 * ns; nb];
    let mut bounds = dsp::pick_peaks(&nov, &thr, 4);
    let dur_s = n_samples as f32 / f.sr as f32;
    let max_sections = ((dur_s / 10.0).ceil() as usize).max(2);
    if bounds.len() + 1 > max_sections {
        bounds.sort_by(|a, b| nov[*b].partial_cmp(&nov[*a]).unwrap());
        bounds.truncate(max_sections - 1);
        bounds.sort_unstable();
    }
    let nov_max = nov.iter().fold(0f32, |m, v| m.max(*v)).max(1e-3);

    // Segments in bar indices.
    let mut seg_edges = vec![0usize];
    seg_edges.extend(bounds.iter().copied());
    seg_edges.push(nb);
    let segs: Vec<(usize, usize)> = seg_edges
        .windows(2)
        .map(|w| (w[0], w[1]))
        .filter(|(a, b)| b > a)
        .collect();
    let seg_e: Vec<f32> = segs
        .iter()
        .map(|&(a, b)| dsp::mean(&energy[a..b]))
        .collect();
    let seg_low: Vec<f32> = segs.iter().map(|&(a, b)| dsp::mean(&zl[a..b])).collect();
    let seg_slope: Vec<f32> = segs.iter().map(|&(a, b)| slope(&energy[a..b])).collect();
    let (smin, smax) = seg_e
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let rel: Vec<f32> = seg_e
        .iter()
        .map(|v| (v - smin) / (smax - smin).max(1e-3))
        .collect();

    let n_seg = segs.len();
    let mut kinds = vec![SectionKind::Groove; n_seg];
    for i in 0..n_seg {
        if rel[i] >= 0.65 && seg_low[i] > -0.2 {
            kinds[i] = SectionKind::Drop;
        }
    }
    for i in 0..n_seg {
        if kinds[i] == SectionKind::Drop {
            continue;
        }
        let next_drop = i + 1 < n_seg && kinds[i + 1] == SectionKind::Drop;
        if i == 0 && rel[i] < 0.5 {
            kinds[i] = if next_drop && seg_slope[i] > 0.02 {
                SectionKind::BuildUp
            } else {
                SectionKind::Intro
            };
        } else if i == n_seg - 1 && (rel[i] < 0.5 || seg_slope[i] < -0.02) {
            kinds[i] = SectionKind::Outro;
        } else if next_drop && seg_slope[i] > 0.008 {
            kinds[i] = SectionKind::BuildUp;
        } else if rel[i] < 0.45 {
            kinds[i] = SectionKind::Breakdown;
        }
    }
    let sections: Vec<MusicalSection> = segs
        .iter()
        .enumerate()
        .map(|(i, &(a, b))| {
            let edge_nov = |bar: usize| {
                if bar == 0 || bar >= nb {
                    1.0
                } else {
                    nov[bar] / nov_max
                }
            };
            MusicalSection {
                start_sample: if a == 0 { 0 } else { bars.starts[a] },
                end_sample: if b == nb { n_samples } else { bars.starts[b] },
                kind: kinds[i],
                energy: seg_e[i].clamp(0.0, 1.0),
                energy_slope: seg_slope[i],
                confidence: (0.5 * (edge_nov(a) + edge_nov(b))).clamp(0.0, 1.0),
            }
        })
        .collect();

    // Drop candidates at every bar boundary, keep local maxima.
    let beat_len = beats
        .windows(2)
        .map(|w| (w[1].sample - w[0].sample) as f32)
        .collect::<Vec<_>>();
    let beat_len = if beat_len.is_empty() {
        (0.5 * f.sr as f32) as i64
    } else {
        dsp::median(&beat_len) as i64
    };
    let mut cands: Vec<(usize, f32, DropFeatures)> = Vec::new();
    for b in 2..nb.saturating_sub(1) {
        let before = &energy[b.saturating_sub(4)..b];
        let after = &energy[b..(b + 4).min(nb)];
        let contrast = dsp::mean(after) - dsp::mean(before);
        if contrast <= 0.05 {
            continue;
        }
        let low_re =
            ((dsp::mean(&zl[b..(b + 4).min(nb)]) - dsp::mean(&zl[b.saturating_sub(4)..b])) / 2.0)
                .clamp(0.0, 1.0);
        let build = (slope(&energy[b.saturating_sub(8)..b]) * 8.0).clamp(0.0, 1.0);
        let dip = {
            let s = bars.starts[b];
            let fa = f.sample_to_frame(s - beat_len);
            let fb = f.sample_to_frame(s).max(fa + 1);
            let pa = f.sample_to_frame(s - 4 * beat_len);
            let last = dsp::mean(&f.rms_db[fa..fb]);
            let prev = dsp::mean(&f.rms_db[pa..fa.max(pa + 1)]);
            ((prev - last) / 6.0).clamp(0.0, 1.0)
        };
        let post_density = ((dsp::mean(&zd[b..(b + 4).min(nb)]) + 1.0) / 2.0).clamp(0.0, 1.0);
        let sustained = if b + 4 < nb {
            let later = dsp::mean(&energy[(b + 4)..(b + 8).min(nb)]);
            let first = dsp::mean(after).max(1e-3);
            (later / first).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let cn = (contrast / 0.5).clamp(0.0, 1.0);
        let score = 0.35 * cn
            + 0.20 * low_re
            + 0.15 * build
            + 0.10 * dip
            + 0.10 * post_density
            + 0.10 * sustained;
        cands.push((
            b,
            score,
            DropFeatures {
                energy_contrast: contrast,
                low_reentry: low_re,
                buildup: build,
                pre_drop_dip: dip,
                post_density,
                sustained,
            },
        ));
    }
    // Local maxima within ±4 bars.
    let mut kept: Vec<(usize, f32, DropFeatures)> = Vec::new();
    for c in &cands {
        let dominated = cands.iter().any(|o| {
            o.0 != c.0
                && (o.0 as isize - c.0 as isize).abs() < 4
                && (o.1 > c.1 || (o.1 == c.1 && o.0 < c.0))
        });
        if !dominated && c.1 >= 0.25 {
            kept.push(c.clone());
        }
    }
    kept.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let mut drops = Vec::new();
    let mut main_bar: Option<usize> = None;
    for (b, score, feats) in kept {
        let real = feats.energy_contrast > 0.15 && feats.sustained >= 0.5 && score >= 0.45;
        let kind = if real && main_bar.is_none() {
            main_bar = Some(b);
            DropKind::MainDrop
        } else if real
            && main_bar
                .map(|m| (m as isize - b as isize).abs() >= 8)
                .unwrap_or(true)
        {
            DropKind::SecondaryDrop
        } else {
            DropKind::FalseDropCandidate
        };
        drops.push(DropCandidate {
            sample: bars.starts[b],
            kind,
            score: score.clamp(0.0, 1.0),
            features: feats,
        });
    }
    drops.sort_by_key(|d| d.sample);
    let _ = bars.beats_per_bar;
    StructureResult { sections, drops }
}
