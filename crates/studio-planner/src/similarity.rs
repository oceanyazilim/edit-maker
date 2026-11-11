//! Similarity between two edits (0 = unrelated, 1 = identical), used to prevent
//! batch generation of near-duplicates.
//!
//! = 0.5 · clip overlap   (fraction of time showing the same asset at ≈ the same source time)
//! + 0.3 · cut overlap    (F-measure of cut positions within ±2 frames)
//! + 0.2 · effect overlap (F-measure of (effect type, time ±2 frames))

use studio_core::Tick;
use studio_timeline::{Timeline, TrackKind};

fn video_clips(t: &Timeline) -> Vec<&studio_timeline::Clip> {
    t.tracks
        .iter()
        .filter(|tr| tr.kind == TrackKind::Video)
        .flat_map(|tr| tr.clips.iter())
        .collect()
}

fn f_match(a: &[(Tick, &str)], b: &[(Tick, &str)], tol: Tick) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut used = vec![false; b.len()];
    let mut m = 0;
    for (ta, ka) in a {
        if let Some(j) = b
            .iter()
            .enumerate()
            .position(|(j, (tb, kb))| !used[j] && kb == ka && (*tb - *ta).abs() <= tol)
        {
            used[j] = true;
            m += 1;
        }
    }
    2.0 * m as f32 / (a.len() + b.len()) as f32
}

pub fn similarity(a: &Timeline, b: &Timeline) -> f32 {
    let dur = a.duration().min(b.duration());
    if dur <= Tick::ZERO {
        return 0.0;
    }
    let ca = video_clips(a);
    let cb = video_clips(b);
    let step = Tick::from_seconds_f64(0.1);
    let tol_src = Tick::from_seconds_f64(0.5);
    let (mut same, mut total) = (0u32, 0u32);
    let mut t = Tick::ZERO;
    while t < dur {
        total += 1;
        let x = ca.iter().find(|c| t >= c.start && t < c.end());
        let y = cb.iter().find(|c| t >= c.start && t < c.end());
        if let (Some(x), Some(y)) = (x, y) {
            if x.asset_id == y.asset_id && (x.source_time(t) - y.source_time(t)).abs() <= tol_src {
                same += 1;
            }
        }
        t += step;
    }
    let clip_overlap = same as f32 / total.max(1) as f32;
    let tol = a.settings.fps.frame_duration() * 2;
    let cuts = |cs: &[&studio_timeline::Clip]| {
        cs.iter()
            .skip(1)
            .map(|c| (c.start, "cut"))
            .collect::<Vec<_>>()
    };
    let cut_overlap = f_match(&cuts(&ca), &cuts(&cb), tol);
    let effs = |cs: &[&studio_timeline::Clip]| {
        cs.iter()
            .flat_map(|c| {
                c.effects
                    .iter()
                    .filter(|e| e.trigger.as_deref() != Some("grade"))
                    .map(move |e| (c.start + e.offset, e.kind.type_name()))
            })
            .collect::<Vec<_>>()
    };
    let fx_overlap = f_match(&effs(&ca), &effs(&cb), tol);
    0.5 * clip_overlap + 0.3 * cut_overlap + 0.2 * fx_overlap
}
