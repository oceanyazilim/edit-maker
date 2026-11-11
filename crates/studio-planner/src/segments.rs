//! Smart Music Segment Selector: ranks audio ranges of a given length for short-form edits.
//!
//! Candidates are built only from detected structure (drops, sections, downbeats). Nothing is
//! invented: a track without a detected drop gets no "drop" suggestions. Starts are snapped to
//! phrase-aligned downbeats (bar index multiple of 4 when available), and ends that land on a
//! phrase boundary score higher, so suggestions avoid stopping in the middle of a phrase.

use serde::{Deserialize, Serialize};
use studio_audio::{AudioAnalysis, DropKind, SectionKind};

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    MainDropEdit,
    SecondDropEdit,
    BuildUpToDrop,
    HighEnergy,
    CinematicIntro,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SegmentSuggestion {
    pub kind: SegmentKind,
    pub label: String,
    pub start_secs: f64,
    pub end_secs: f64,
    /// Where the drop lands inside the segment (seconds from its start), if it contains one.
    pub drop_offset_secs: Option<f64>,
    /// Heuristic 0..1 ranking score (not a probability).
    pub score: f32,
    /// When the range ends mid-phrase: the nearest duration (within ±20 %) that ends on a
    /// phrase boundary instead, as an alternative the user can pick.
    pub phrase_fit_duration_secs: Option<f64>,
    /// Human-readable reasons derived from the measured features.
    pub reasons: Vec<String>,
}

fn overlap(a: (f64, f64), b: (f64, f64)) -> f64 {
    let inter = (a.1.min(b.1) - a.0.max(b.0)).max(0.0);
    inter / (a.1 - a.0).min(b.1 - b.0).max(1e-6)
}

/// Rank segment candidates of `duration_secs` length.
pub fn recommend_segments(a: &AudioAnalysis, duration_secs: f64) -> Vec<SegmentSuggestion> {
    let sr = a.sample_rate as f64;
    let total = a.duration_seconds();
    let dur = duration_secs.min(total);
    if dur <= 0.5 {
        return vec![];
    }
    let downs: Vec<(f64, i32)> = a
        .beats
        .iter()
        .filter(|b| b.is_downbeat)
        .map(|b| (b.sample as f64 / sr, b.bar))
        .collect();
    let phrase_downs: Vec<f64> = {
        let p: Vec<f64> = downs
            .iter()
            .filter(|(_, bar)| bar.rem_euclid(4) == 0)
            .map(|(t, _)| *t)
            .collect();
        if p.len() >= 2 {
            p
        } else {
            downs.iter().map(|(t, _)| *t).collect()
        }
    };
    let max_start = (total - dur).max(0.0);
    let bar_secs = 4.0 * 60.0 / a.tempo.bpm.max(1.0);
    let nearest = |pts: &[f64], want: f64| -> Option<f64> {
        pts.iter()
            .copied()
            .filter(|t| *t <= max_start + 1e-6)
            .min_by(|x, y| (x - want).abs().partial_cmp(&(y - want).abs()).unwrap())
    };
    let bar_downs: Vec<f64> = downs.iter().map(|(t, _)| *t).collect();
    // Snap a desired start: a phrase downbeat within one bar of the target, otherwise the
    // nearest bar downbeat (keeps a sensible lead-in before a drop), otherwise unsnapped.
    let snap = |want: f64| -> (f64, &'static str) {
        if let Some(t) =
            nearest(&phrase_downs, want).filter(|t| (t - want).abs() <= bar_secs * 1.01)
        {
            return (t, "starts on a phrase downbeat");
        }
        if let Some(t) = nearest(&bar_downs, want).filter(|t| (t - want).abs() < dur * 0.5) {
            return (t, "starts on a downbeat");
        }
        (
            want.clamp(0.0, max_start),
            "start not aligned to a downbeat",
        )
    };
    let phrase_fit = |start: f64| -> Option<f64> {
        phrase_downs
            .iter()
            .map(|t| t - start)
            .filter(|d| *d >= dur * 0.8 && *d <= dur * 1.2 && start + d <= total + 1e-6)
            .min_by(|x, y| (x - dur).abs().partial_cmp(&(y - dur).abs()).unwrap())
    };
    let end_on_phrase = |end: f64| -> bool {
        let beat = 60.0 / a.tempo.bpm.max(1.0);
        phrase_downs.iter().any(|t| (t - end).abs() < beat * 0.5) || (total - end).abs() < 0.05
    };
    let energy_in = |s: f64, e: f64| -> f32 {
        let (mut acc, mut w) = (0.0f64, 0.0f64);
        for sec in &a.sections {
            let (ss, se) = (sec.start_sample as f64 / sr, sec.end_sample as f64 / sr);
            let ov = (se.min(e) - ss.max(s)).max(0.0);
            acc += ov * sec.energy as f64;
            w += ov;
        }
        if w > 0.0 {
            (acc / w) as f32
        } else {
            0.0
        }
    };

    let mut out: Vec<SegmentSuggestion> = Vec::new();
    let mut push = |kind: SegmentKind,
                    label: &str,
                    want_start: f64,
                    drop_t: Option<f64>,
                    base: f32,
                    mut reasons: Vec<String>| {
        let (start, start_note) = snap(want_start);
        let end = (start + dur).min(total);
        let drop_offset = drop_t
            .filter(|d| *d >= start && *d < end)
            .map(|d| d - start);
        let energy = energy_in(start, end);
        let ends_well = end_on_phrase(end);
        let mut score = base + 0.25 * energy;
        match start_note {
            "starts on a phrase downbeat" => score += 0.1,
            "starts on a downbeat" => score += 0.05,
            _ => {}
        }
        reasons.push(start_note.into());
        let fit = if ends_well { None } else { phrase_fit(start) };
        if ends_well {
            score += 0.1;
            reasons.push("ends on a phrase boundary".into());
        } else if let Some(f) = fit {
            reasons.push(format!(
                "ends mid-phrase; {f:.2}s would end on a phrase boundary"
            ));
        } else {
            reasons.push("ends mid-phrase".into());
        }
        if drop_t.is_some() && drop_offset.is_none() {
            return; // the drop does not fit in this range
        }
        reasons.push(format!("mean section energy {energy:.2}"));
        out.push(SegmentSuggestion {
            kind,
            label: label.into(),
            start_secs: start,
            end_secs: end,
            drop_offset_secs: drop_offset,
            score: score.clamp(0.0, 1.0),
            phrase_fit_duration_secs: fit,
            reasons,
        });
    };

    let mut drops: Vec<_> = a
        .drops
        .iter()
        .filter(|d| d.kind != DropKind::FalseDropCandidate)
        .collect();
    drops.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap());
    for (i, d) in drops.iter().enumerate() {
        let t = d.sample as f64 / sr;
        let (kind, label) = if d.kind == DropKind::MainDrop || i == 0 {
            (SegmentKind::MainDropEdit, "Main Drop Edit")
        } else {
            (SegmentKind::SecondDropEdit, "Second Drop Edit")
        };
        let base = if kind == SegmentKind::MainDropEdit {
            0.5
        } else {
            0.35
        } * (0.5 + d.score);
        push(
            kind,
            label,
            t - 0.25 * dur,
            Some(t),
            base,
            vec![format!(
                "contains the {} at {t:.2}s (score {:.2})",
                if kind == SegmentKind::MainDropEdit {
                    "main drop"
                } else {
                    "drop"
                },
                d.score
            )],
        );
        // Build-up to drop: most of the segment before the drop.
        let has_build = a
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::BuildUp && (s.end_sample as f64 / sr - t).abs() < 1.0);
        let base_b = if has_build { 0.45 } else { 0.3 } * (0.5 + d.score);
        let mut r = vec![format!("builds into the drop at {t:.2}s")];
        if has_build {
            r.push("a detected build-up section precedes the drop".into());
        }
        push(
            SegmentKind::BuildUpToDrop,
            "Build-up to Drop",
            t - 0.65 * dur,
            Some(t),
            base_b,
            r,
        );
    }
    // Highest-energy window that does not merely duplicate a drop suggestion.
    if !phrase_downs.is_empty() {
        let best = phrase_downs
            .iter()
            .copied()
            .filter(|s| *s <= max_start + 1e-6)
            .max_by(|x, y| {
                energy_in(*x, *x + dur)
                    .partial_cmp(&energy_in(*y, *y + dur))
                    .unwrap()
            });
        if let Some(s) = best {
            push(
                SegmentKind::HighEnergy,
                "High Energy Section",
                s,
                None,
                0.3,
                vec!["highest mean section energy".into()],
            );
        }
    }
    if a.sections
        .first()
        .map(|s| s.kind == SectionKind::Intro && (s.end_sample - s.start_sample) as f64 / sr >= 2.0)
        .unwrap_or(false)
    {
        push(
            SegmentKind::CinematicIntro,
            "Cinematic Introduction",
            0.0,
            None,
            0.2,
            vec!["uses the detected intro section".into()],
        );
    }

    out.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap());
    let mut kept: Vec<SegmentSuggestion> = Vec::new();
    for s in out {
        if kept
            .iter()
            .all(|k| overlap((k.start_secs, k.end_secs), (s.start_secs, s.end_secs)) < 0.85)
        {
            kept.push(s);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_audio::{analyze, synth, AnalysisConfig, DecodedAudio};
    use studio_core::cancel::noop_progress;
    use studio_core::CancelToken;

    #[test]
    fn suggestions_come_from_detected_structure() {
        let tr = synth::render(&synth::edm_arrangement(), 128.0, 44100, 0.5, 1);
        let a = analyze(
            &DecodedAudio::from_mono(tr.samples.clone(), 44100),
            &AnalysisConfig::default(),
            &CancelToken::new(),
            &noop_progress(),
        )
        .unwrap();
        for dur in [10.0, 15.0, 20.0, 30.0] {
            let s = recommend_segments(&a, dur);
            assert!(!s.is_empty());
            let top = &s[0];
            assert!((top.end_secs - top.start_secs - dur).abs() < 1e-6, "length");
            assert!(top.start_secs >= 0.0 && top.end_secs <= a.duration_seconds() + 1e-6);
            // A drop suggestion must contain a true drop.
            for x in s.iter().filter(|x| {
                matches!(
                    x.kind,
                    SegmentKind::MainDropEdit
                        | SegmentKind::SecondDropEdit
                        | SegmentKind::BuildUpToDrop
                )
            }) {
                let d = x.start_secs + x.drop_offset_secs.expect("drop offset");
                assert!(tr.drops.iter().any(|t| (t - d).abs() < 0.6), "drop at {d}");
            }
            // Starts are phrase-aligned downbeats.
            let beat = 60.0 / 128.0;
            let bar = 4.0 * beat;
            let rel = (top.start_secs - 0.5) / bar;
            assert!(
                (rel - rel.round()).abs() < 0.05,
                "start {} not on a bar",
                top.start_secs
            );
            println!(
                "{dur}s: {:?}",
                s.iter()
                    .map(|x| (x.label.clone(), x.start_secs, x.score))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn no_drop_means_no_drop_suggestions() {
        let mut a_sec = synth::SynthSection::new(SectionKind::Groove, 16);
        a_sec.hats = true;
        a_sec.kick = synth::KickPattern::FourOnFloor;
        let tr = synth::render(&[a_sec], 120.0, 44100, 0.2, 2);
        let mut a = analyze(
            &DecodedAudio::from_mono(tr.samples, 44100),
            &AnalysisConfig::default(),
            &CancelToken::new(),
            &noop_progress(),
        )
        .unwrap();
        a.drops.clear();
        let s = recommend_segments(&a, 10.0);
        assert!(s.iter().all(|x| x.drop_offset_secs.is_none()));
        assert!(s.iter().any(|x| x.kind == SegmentKind::HighEnergy));
    }
}
