//! The edit planning algorithm.
//!
//! 1. Choose the audio interval (land the main drop at the preset's position).
//! 2. Build a weighted hierarchy of musical events (sections > drops > downbeats > kicks > beats).
//! 3. Place cut points: target shot length per section × cut density, cost =
//!    duration deviation + beat-sync × (1 − event weight); forced cuts at drops and
//!    section boundaries; anticipation hold before drops.
//! 4. Assign scenes with a weighted score (energy match, quality, action alignment,
//!    niche tags, duration fit, continuity) minus repetition/diversity penalties.
//! 5. Automate effects (drop impacts, kick-triggered punches with cooldown and
//!    pattern variation, grade, slow push-ins), transitions (with handle checks), fades.
//! 6. Emit the canonical timeline and validate it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use studio_audio::{AudioAnalysis, DropKind, SectionKind};
use studio_core::{Rational, StudioError, StudioResult, StudioRng, Tick};
use studio_timeline::{
    validate, AssetInfo, Clip, ClipOrigin, Ease, EffectInstance, EffectKind, Framing, Marker,
    MarkerKind, Timeline, TimelineSettings, Track, TrackKind, TransformKeyframe, Transition,
    TransitionKind, ValidationIssue, ValidationSeverity,
};
use studio_vision::{Scene, VideoAnalysis};

use crate::controls::{CreativeControls, Effective, Variant};
use crate::niche::NicheProfile;
use crate::presets::EditPreset;

pub struct FootageSource<'a> {
    pub asset_id: String,
    pub analysis: &'a VideoAnalysis,
    pub duration: Tick,
    /// Display size (after rotation).
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OutputTarget {
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub duration: Tick,
}

/// A user-pinned moment: "at `at_secs` in the output, cut to `asset_id` showing source
/// time `source_secs`". With `snap_to_beat` the time moves to the nearest musical event
/// (beat / downbeat / kick / section / drop) within half a beat, so the pinned moment also
/// lands on the rhythm.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Cue {
    pub at_secs: f64,
    pub asset_id: Option<String>,
    pub source_secs: Option<f64>,
    pub snap_to_beat: bool,
    pub label: String,
}

impl Default for Cue {
    fn default() -> Self {
        Self {
            at_secs: 0.0,
            asset_id: None,
            source_secs: None,
            snap_to_beat: true,
            label: String::new(),
        }
    }
}

/// Where a cue actually landed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ResolvedCue {
    pub requested: Tick,
    pub resolved: Tick,
    pub clip_index: Option<usize>,
    pub honored_source: bool,
}

pub struct PlanInput<'a> {
    pub audio: &'a AudioAnalysis,
    pub music_asset_id: String,
    pub footage: Vec<FootageSource<'a>>,
    pub preset: &'a EditPreset,
    pub controls: CreativeControls,
    pub target: OutputTarget,
    pub seed: u64,
    pub variant: Variant,
    pub niche: Option<&'a NicheProfile>,
    /// User-pinned moments (forced cuts with optional clip/source assignment).
    pub cues: Vec<Cue>,
    /// Clips to preserve exactly (regeneration with locks). Their boundaries become forced
    /// cuts and they are re-emitted unchanged (unless `regenerate_locked_effects`).
    pub locked: Vec<Clip>,
    /// Effects-only mode: keep source and timing of locked clips, recompute their effects.
    pub regenerate_locked_effects: bool,
    /// Explicit start of the audio window in song time (smart segment selector).
    pub audio_start: Option<Tick>,
    /// Where the main drop should land in the output (0..1), overriding the preset.
    pub drop_position: Option<f32>,
    /// Scenes to avoid (asset id, scene index), e.g. when replacing a clip.
    pub avoid_scenes: Vec<(String, u32)>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlannedSegment {
    pub start: Tick,
    pub end: Tick,
    pub section: SectionKind,
    pub target_energy: f32,
    pub asset_id: String,
    pub scene_index: u32,
    pub source_in: Tick,
    pub speed: f64,
    pub score: f32,
    pub reason: String,
    pub is_drop: bool,
    /// The segment reproduces a locked clip.
    #[serde(default)]
    pub locked: bool,
    /// Speed ramp: (fraction of the shot for part A, speed A, speed B). `speed` is the average.
    #[serde(default)]
    pub velocity: Option<(f64, f64, f64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EditPlan {
    pub timeline: Timeline,
    pub audio_start_sample: i64,
    pub audio_end_sample: i64,
    pub segments: Vec<PlannedSegment>,
    pub warnings: Vec<String>,
    pub issues: Vec<ValidationIssue>,
    pub seed: u64,
    pub variant: Variant,
    pub preset_id: String,
    #[serde(default)]
    pub cues: Vec<ResolvedCue>,
}

#[derive(Clone, Copy, Debug)]
struct Event {
    t: Tick,
    w: f32,
    forced: bool,
}

struct Cand<'a> {
    fi: usize,
    scene: &'a Scene,
    span_start: Tick,
    span_end: Tick,
    energy_rank: f32,
}

fn section_name(k: SectionKind) -> &'static str {
    match k {
        SectionKind::Intro => "intro",
        SectionKind::Groove => "groove",
        SectionKind::BuildUp => "build_up",
        SectionKind::Drop => "drop",
        SectionKind::Breakdown => "breakdown",
        SectionKind::Outro => "outro",
    }
}

fn transition_kind(name: &str) -> Option<TransitionKind> {
    match name {
        "cross_dissolve" => Some(TransitionKind::CrossDissolve),
        "flash_cut" => Some(TransitionKind::FlashCut),
        "zoom_blend" => Some(TransitionKind::ZoomBlend),
        "dip_to_black" => Some(TransitionKind::DipToBlack),
        _ => None,
    }
}

/// Choose [start, end) in samples.
fn choose_window(a: &AudioAnalysis, want: i64, drop_pos: f32) -> (i64, i64) {
    let total = a.duration_samples;
    if want >= total - (a.sample_rate as i64 / 10) {
        return (0, total);
    }
    let downbeats: Vec<i64> = a
        .beats
        .iter()
        .filter(|b| b.is_downbeat)
        .map(|b| b.sample)
        .collect();
    let snap = |s: i64| -> i64 {
        downbeats
            .iter()
            .copied()
            .filter(|d| *d >= 0 && *d <= total - want)
            .min_by_key(|d| (d - s).abs())
            .unwrap_or(s)
    };
    let drop = a
        .main_drop()
        .or_else(|| a.drops.iter().find(|d| d.kind == DropKind::SecondaryDrop));
    let start = if let Some(d) = drop {
        let raw = d.sample - (drop_pos as f64 * want as f64) as i64;
        snap(raw.clamp(0, total - want))
    } else {
        // Highest-energy window starting on a downbeat.
        let starts: Vec<i64> = if downbeats.is_empty() {
            (0..20).map(|i| i * (total - want) / 20).collect()
        } else {
            downbeats.clone()
        };
        let energy_of = |s: i64| -> f64 {
            let e = s + want;
            a.sections
                .iter()
                .map(|sec| {
                    let ov = (sec.end_sample.min(e) - sec.start_sample.max(s)).max(0) as f64;
                    ov * sec.energy as f64
                })
                .sum::<f64>()
        };
        starts
            .into_iter()
            .filter(|s| *s >= 0 && *s <= total - want)
            .max_by(|x, y| energy_of(*x).partial_cmp(&energy_of(*y)).unwrap())
            .unwrap_or(0)
    };
    let start = start.clamp(0, total - want);
    (start, start + want)
}

pub fn plan_edit(inp: &PlanInput) -> StudioResult<EditPlan> {
    let a = inp.audio;
    let p = inp.preset;
    let fps = inp.target.fps;
    let sr = a.sample_rate;
    let frame = fps.frame_duration();
    let eff = Effective::resolve(p, &inp.controls, inp.variant, inp.niche);
    let mut warnings = Vec::new();

    if inp.footage.is_empty() {
        return Err(StudioError::invalid_input(
            "Import at least one video before generating an edit.",
        ));
    }
    if inp.target.duration < Tick::from_seconds_f64(1.0) {
        return Err(StudioError::invalid_input(
            "The target duration must be at least one second.",
        ));
    }
    let stream = |name: &str| {
        StudioRng::derive(inp.seed, &format!("{name}/{}/{}", inp.variant.code(), p.id))
    };
    let mut rng_cut = stream("cuts");
    let mut rng_scene = stream("scenes");
    let mut rng_fx = stream("effects");

    // 1. Audio window.
    let want = inp.target.duration.to_samples(sr).min(a.duration_samples);
    let (ws, we) = match inp.audio_start {
        Some(st) => {
            let s0 = st
                .to_samples(sr)
                .clamp(0, (a.duration_samples - want).max(0));
            (s0, s0 + want)
        }
        None => choose_window(
            a,
            want,
            inp.drop_position.unwrap_or(p.drop.position).clamp(0.0, 1.0),
        ),
    };
    let t_end = Tick::from_samples(we - ws, sr).snap_to_frame(fps);
    if t_end < inp.target.duration - frame {
        warnings.push(format!(
            "The music is shorter than the requested duration; the edit is {:.1}s long.",
            t_end.as_seconds_f64()
        ));
    }
    let to_t = |s: i64| Tick::from_samples(s - ws, sr).snap_to_frame(fps);
    let in_win = |s: i64| s >= ws && s < we;
    let beat_secs = 60.0 / a.tempo.bpm.max(1.0);
    let beat = Tick::from_seconds_f64(beat_secs);
    let section_at = |t: Tick| -> (SectionKind, f32, f32) {
        let s = ws + t.to_samples(sr);
        match a.section_at(s) {
            Some(sec) => {
                let prog = (s - sec.start_sample) as f32
                    / (sec.end_sample - sec.start_sample).max(1) as f32;
                (sec.kind, sec.energy, prog)
            }
            None => (SectionKind::Groove, 0.5, 0.0),
        }
    };

    // 2. Musical events.
    let mut events: Vec<Event> = Vec::new();
    for b in a.beats.iter().filter(|b| in_win(b.sample)) {
        let w = if b.is_downbeat {
            if b.bar % 4 == 0 {
                0.85
            } else {
                0.75
            }
        } else {
            0.4 * (0.5 + 0.5 * b.strength)
        };
        events.push(Event {
            t: to_t(b.sample),
            w,
            forced: false,
        });
    }
    for k in a.kicks.iter().filter(|k| in_win(k.sample)) {
        events.push(Event {
            t: to_t(k.sample),
            w: 0.35 + 0.35 * k.intensity * k.confidence,
            forced: false,
        });
    }
    let mut drop_times: Vec<(Tick, bool)> = Vec::new(); // (t, is_main)
    for d in a
        .drops
        .iter()
        .filter(|d| d.kind != DropKind::FalseDropCandidate && in_win(d.sample))
    {
        let t = to_t(d.sample);
        drop_times.push((t, d.kind == DropKind::MainDrop));
        events.push(Event {
            t,
            w: 1.0,
            forced: true,
        });
    }
    for s in a
        .sections
        .iter()
        .filter(|s| s.start_sample > ws && in_win(s.start_sample))
    {
        events.push(Event {
            t: to_t(s.start_sample),
            w: 1.0,
            forced: true,
        });
    }
    events.retain(|e| e.t > Tick::ZERO && e.t < t_end);
    events.sort_by_key(|e| e.t);
    let mut merged: Vec<Event> = Vec::new();
    for e in events {
        if let Some(l) = merged.last_mut() {
            if (e.t - l.t).abs() <= frame {
                l.w = l.w.max(e.w);
                l.forced |= e.forced;
                continue;
            }
        }
        merged.push(e);
    }
    let mut events = merged;

    // 2b. User cues become forced cuts (optionally snapped onto the nearest musical event).
    let half_beat = Tick::from_seconds_f64(beat_secs * 0.5);
    let mut cue_times: Vec<(Tick, usize)> = Vec::new(); // (resolved time, cue index)
    for (ci, cue) in inp.cues.iter().enumerate() {
        let req = Tick::from_seconds_f64(cue.at_secs).snap_to_frame(fps);
        if req <= Tick::ZERO || req >= t_end {
            warnings.push(format!(
                "Cue at {:.2}s is outside the edit (0–{:.2}s) and was ignored.",
                cue.at_secs,
                t_end.as_seconds_f64()
            ));
            continue;
        }
        if let Some(a) = &cue.asset_id {
            if !inp.footage.iter().any(|f| &f.asset_id == a) {
                warnings.push(format!(
                    "Cue at {:.2}s references media that is not in this project's footage.",
                    cue.at_secs
                ));
            }
        }
        let t = if cue.snap_to_beat {
            events
                .iter()
                .map(|e| e.t)
                .filter(|t| (*t - req).abs() <= half_beat)
                .min_by_key(|t| (*t - req).abs())
                .unwrap_or(req)
        } else {
            req
        };
        cue_times.push((t, ci));
        events.push(Event {
            t,
            w: 1.0,
            forced: true,
        });
    }
    events.sort_by_key(|e| e.t);
    events.dedup_by(|b, a| {
        if (b.t - a.t).abs() <= frame {
            a.forced |= b.forced;
            a.w = a.w.max(b.w);
            true
        } else {
            false
        }
    });

    // 2c. Locked clips: keep only valid ones (inside the edit, media still present).
    let locked: Vec<&Clip> = inp
        .locked
        .iter()
        .filter(|c| {
            let ok = c.start >= Tick::ZERO
                && c.end() <= t_end + frame
                && c.duration > Tick::ZERO
                && inp.footage.iter().any(|f| f.asset_id == c.asset_id);
            if !ok {
                warnings.push(format!(
                    "Locked clip {} could not be kept (outside the edit or its media is missing).",
                    c.id
                ));
            }
            ok
        })
        .collect();
    let mut lock_spans: Vec<(Tick, Tick)> = locked
        .iter()
        .map(|c| (c.start, c.end().min(t_end)))
        .collect();
    lock_spans.sort();
    for (a0, a1) in &lock_spans {
        for t in [*a0, *a1] {
            if t > Tick::ZERO && t < t_end {
                events.push(Event {
                    t,
                    w: 1.0,
                    forced: true,
                });
            }
        }
    }
    events.sort_by_key(|e| e.t);

    // 3. Cut points.
    let min_shot = Tick::from_seconds_f64(eff.min_shot_secs as f64);
    let tiny = Tick::from_seconds_f64(0.15);
    let hold = Tick::from_seconds_f64(beat_secs * p.drop.pre_drop_hold_beats as f64 * 0.9);
    let mut cuts: Vec<Tick> = vec![Tick::ZERO];
    let mut t = Tick::ZERO;
    // Phrasing: shot lengths follow a seeded pattern (mean 1.0) instead of a constant length,
    // restarted at each section, with a longer hold on phrase starts (every 8 bars).
    const PATTERNS: &[&[f64]] = &[
        &[1.0, 1.0, 2.0],
        &[2.0, 1.0, 1.0, 2.0, 1.0, 1.0, 4.0],
        &[1.0, 1.0, 1.0, 1.0, 2.0, 2.0],
        &[2.0, 2.0, 1.0, 1.0, 1.0, 1.0],
        &[1.0, 2.0, 1.0, 1.0, 3.0],
    ];
    let mut pat: &[f64] = PATTERNS[rng_cut.below(PATTERNS.len())];
    let mut pat_mean = pat.iter().sum::<f64>() / pat.len() as f64;
    let mut pat_i = 0usize;
    let mut cur_section: Option<SectionKind> = None;
    let phrase = beat * 32;
    loop {
        if let Some((_, le)) = lock_spans.iter().find(|(ls, le)| *ls <= t && t < *le) {
            // Inside a locked clip: jump to its end.
            let le = *le;
            if le >= t_end {
                break;
            }
            cuts.push(le);
            t = le;
            continue;
        }
        let next_lock = lock_spans.iter().map(|(ls, _)| *ls).find(|ls| *ls > t);
        let (kind, _e, prog) = section_at(t);
        if cur_section != Some(kind) {
            cur_section = Some(kind);
            pat = PATTERNS[rng_cut.below(PATTERNS.len())];
            pat_mean = pat.iter().sum::<f64>() / pat.len() as f64;
            pat_i = 0;
        }
        let mut shape = pat[pat_i % pat.len()] / pat_mean;
        pat_i += 1;
        // Hold a little longer when a shot starts on a phrase boundary.
        if phrase.0 > 0 && (t.0 % phrase.0) < frame.0 && t > Tick::ZERO {
            shape = shape.max(1.5);
        }
        let mut l = beat_secs
            * p.pacing.base_shot_beats as f64
            * p.section_multiplier(kind) as f64
            * eff.shot_scale as f64
            * shape;
        if kind == SectionKind::BuildUp {
            l *= 1.0 - 0.5 * p.pacing.build_up_acceleration as f64 * prog as f64;
        }
        let l = l.clamp(eff.min_shot_secs as f64, eff.max_shot_secs as f64);
        let lt = Tick::from_seconds_f64(l);
        let lo = t + min_shot;
        // Monotony guard: after 3 equal-length shots, prefer a different (musical) length.
        let n = cuts.len();
        let last_len = if n >= 2 {
            cuts[n - 1] - cuts[n - 2]
        } else {
            Tick::ZERO
        };
        let mut run_len = 0usize;
        if n >= 2 {
            let mut k = n - 1;
            while k >= 1 && ((cuts[k] - cuts[k - 1]) - last_len).abs() <= frame {
                run_len += 1;
                k -= 1;
            }
        }
        let mut hi = t + Tick::from_seconds_f64((l * 1.75).max(eff.min_shot_secs as f64 * 1.5));
        if run_len >= 3 {
            // Make sure a longer musical alternative (≥ 2× the repeated length) is reachable.
            hi = hi.max(t + Tick((last_len.0 as f64 * 2.1) as i64));
        }
        if lo >= t_end - Tick(min_shot.0 / 2) {
            break;
        }
        let next_drop = drop_times.iter().map(|d| d.0).find(|d| *d > t);
        let in_hold = |c: Tick| next_drop.map(|d| c > d - hold && c < d).unwrap_or(false);
        let forced = events
            .iter()
            .find(|e| e.forced && e.t > t + tiny && e.t <= hi)
            .map(|e| e.t);
        let chosen = if let Some(f) = forced {
            f
        } else {
            let mut best = (f32::MAX, t + lt);
            let mut consider = |c: Tick, w: f32, best: &mut (f32, Tick)| {
                if c < lo || c > hi || c >= t_end || in_hold(c) {
                    return;
                }
                let d = (c - t).as_seconds_f64() as f32;
                let repeat = if run_len >= 3 && ((c - t) - last_len).abs() <= frame {
                    3.0
                } else {
                    0.0
                };
                let cost = 0.6 * (d - l as f32).abs() / l as f32
                    + eff.beat_sync * (1.0 - w)
                    + repeat
                    + eff.temperature * rng_cut.next_f64() as f32;
                if cost < best.0 {
                    *best = (cost, c);
                }
            };
            // The free (off-grid) target is only used when no musical event is available
            // in the window, or when beat sync is deliberately weak.
            let has_event = events
                .iter()
                .any(|e| e.t >= lo && e.t <= hi && e.t < t_end && !in_hold(e.t));
            if !has_event || eff.beat_sync < 0.5 {
                consider(t + lt, 0.0, &mut best);
            }
            for e in events.iter().filter(|e| e.t >= lo && e.t <= hi) {
                consider(e.t, e.w, &mut best);
            }
            best.1
        };
        let mut chosen = chosen.snap_to_frame(fps);
        if let Some(nl) = next_lock {
            if chosen > nl || lo > nl {
                chosen = nl;
            }
        }
        if std::env::var_os("STUDIO_DEBUG_CUTS").is_some() {
            eprintln!(
                "cut t={:.3} -> {:.3} L={:.3} run={} forced={} hi={:.3} kind={:?}",
                t.as_seconds_f64(),
                chosen.as_seconds_f64(),
                l,
                run_len,
                forced.is_some(),
                (hi - t).as_seconds_f64(),
                kind
            );
        }
        if chosen <= t || chosen >= t_end {
            break;
        }
        cuts.push(chosen);
        t = chosen;
    }
    // Avoid a tiny last segment.
    if cuts.len() > 1
        && t_end - *cuts.last().unwrap() < Tick(min_shot.0 * 7 / 10)
        && !lock_spans
            .iter()
            .any(|(a0, a1)| *a0 == *cuts.last().unwrap() || *a1 == *cuts.last().unwrap())
    {
        cuts.pop();
    }
    cuts.push(t_end);

    // 4. Scene candidates.
    let margin = Tick::from_seconds_f64(0.12);
    let mut cands: Vec<Cand> = Vec::new();
    for (fi, f) in inp.footage.iter().enumerate() {
        for s in f.analysis.scenes.iter().filter(|s| s.usable) {
            let ss = s.start
                + if s.start > Tick::ZERO {
                    margin
                } else {
                    Tick::ZERO
                };
            let se = (s.end - margin).min(f.duration);
            if se - ss >= Tick::from_seconds_f64(0.2) {
                cands.push(Cand {
                    fi,
                    scene: s,
                    span_start: ss,
                    span_end: se,
                    energy_rank: 0.0,
                });
            }
        }
    }
    if cands.is_empty() {
        return Err(StudioError::invalid_input(
            "None of the imported videos contain usable scenes (too short or black).",
        ));
    }
    // Energy rank (percentile) makes footage energy comparable with music energy.
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by(|x, y| {
        cands[*x]
            .scene
            .energy
            .partial_cmp(&cands[*y].scene.energy)
            .unwrap()
    });
    let n_c = cands.len().max(2) as f32 - 1.0;
    for (rank, &ci) in order.iter().enumerate() {
        cands[ci].energy_rank = if cands.len() == 1 {
            0.5
        } else {
            rank as f32 / n_c
        };
    }

    let w = &p.scoring;
    let mut usage: HashMap<(usize, u32), u32> = HashMap::new();
    let mut used_ranges: HashMap<usize, Vec<(Tick, Tick)>> = HashMap::new();
    let mut recent: Vec<(usize, u32)> = Vec::new();
    let mut segments: Vec<PlannedSegment> = Vec::new();
    let mut transitions: Vec<Option<Transition>> = Vec::new();
    let mut prev: Option<(usize, u32, Tick)> = None; // (footage, scene, source_out)

    let n_seg = cuts.len() - 1;
    for gi in 0..n_seg {
        let (gs, ge) = (cuts[gi], cuts[gi + 1]);
        let d = ge - gs;
        let (kind, sec_energy, _) = section_at(gs);
        if let Some(lc) = locked.iter().find(|c| (c.start - gs).abs() <= frame) {
            let fi = inp
                .footage
                .iter()
                .position(|f| f.asset_id == lc.asset_id)
                .unwrap_or(0);
            let scene_index = lc
                .origin
                .scene_id
                .as_ref()
                .and_then(|s| s.rsplit('#').next())
                .and_then(|x| x.parse::<u32>().ok())
                .unwrap_or(0);
            *usage.entry((fi, scene_index)).or_insert(0) += 1;
            used_ranges
                .entry(fi)
                .or_default()
                .push((lc.source_in, lc.source_out()));
            recent.push((fi, scene_index));
            prev = Some((fi, scene_index, lc.source_out()));
            transitions.push(lc.transition_in.clone());
            segments.push(PlannedSegment {
                start: gs,
                end: ge,
                section: kind,
                target_energy: sec_energy,
                asset_id: lc.asset_id.clone(),
                scene_index,
                source_in: lc.source_in,
                speed: lc.speed,
                score: lc.origin.score.unwrap_or(0.0),
                reason: format!("locked: {}", lc.origin.reason.clone().unwrap_or_default()),
                is_drop: drop_times.iter().any(|(dt, _)| (*dt - gs).abs() <= frame),
                locked: true,
                velocity: None,
            });
            continue;
        }
        let is_drop = drop_times.iter().any(|(dt, _)| (*dt - gs).abs() <= frame);
        let is_main_drop = drop_times
            .iter()
            .any(|(dt, m)| *m && (*dt - gs).abs() <= frame);
        let ends_at_drop = drop_times.iter().any(|(dt, _)| (*dt - ge).abs() <= frame);
        let mut target_e = (sec_energy + 0.5 * eff.energy_bias).clamp(0.0, 1.0);
        if is_drop {
            target_e = target_e.max(0.9);
        }
        // Transition into this segment.
        let sec_ok = p
            .transitions
            .sections
            .iter()
            .any(|s| s == section_name(kind));
        let mut tr: Option<Transition> = None;
        if gi > 0
            && !is_drop
            && sec_ok
            && !p.transitions.kinds.is_empty()
            && rng_fx.next_f64() < eff.transition_prob as f64
        {
            let kname = &p.transitions.kinds[rng_fx.below(p.transitions.kinds.len())];
            if let Some(k) = transition_kind(kname) {
                let prev_d = cuts[gi] - cuts[gi - 1];
                let dur = Tick::from_seconds_f64(beat_secs * p.transitions.duration_beats as f64)
                    .min(Tick((prev_d.0.min(d.0) as f64 * 0.4) as i64))
                    .snap_to_frame(fps);
                if dur >= frame * 2 {
                    tr = Some(Transition {
                        kind: k,
                        duration: dur,
                    });
                }
            }
        }
        // Velocity ramps: into the drop (fast -> slow motion) and, occasionally, right after
        // a hit inside drop sections (burst -> ease). Never on cue-pinned cuts.
        let cue_here = cue_times.iter().any(|(t, _)| (*t - gs).abs() <= frame);
        let vel_amt = (p.drop.velocity * (0.5 + eff.effect_intensity)).clamp(0.0, 1.5) as f64;
        let mut velocity: Option<(f64, f64, f64)> = None;
        if vel_amt > 0.01 && !cue_here && d >= Tick::from_seconds_f64(0.6) {
            if ends_at_drop {
                velocity = Some((0.6, 1.0 + vel_amt, 0.4));
            } else if kind == SectionKind::Drop
                && !is_drop
                && rng_fx.next_f64() < 0.35 * vel_amt.min(1.0)
            {
                velocity = Some((0.3, 1.0 + 0.8 * vel_amt, 0.75));
            }
        }
        if velocity.is_some() {
            tr = None;
        }
        let speed = if let Some((f, s1, s2)) = velocity {
            f * s1 + (1.0 - f) * s2
        } else if ends_at_drop && p.drop.pre_drop_slowmo < 1.0 {
            p.drop.pre_drop_slowmo as f64
        } else {
            1.0
        };
        let handle = tr
            .as_ref()
            .filter(|t| t.needs_handles())
            .map(|t| t.half())
            .unwrap_or(Tick::ZERO);
        // handle after the clip is needed if the *next* segment gets a dissolve; we reserve a
        // small tail handle opportunistically and re-check in validation.
        let need = |spd: f64| d.scale_f64(spd) + handle.scale_f64(spd);

        // A cue pinned to this cut restricts the choice to its media (and source moment).
        let cue = cue_times
            .iter()
            .find(|(t, _)| (*t - gs).abs() <= frame)
            .map(|(_, i)| &inp.cues[*i]);
        let cue_asset = cue
            .and_then(|c| c.asset_id.as_ref())
            .filter(|a| inp.footage.iter().any(|f| &f.asset_id == *a));
        let cue_src = cue.and_then(|c| c.source_secs).map(Tick::from_seconds_f64);

        let mut best: Option<(f32, usize, f64, String)> = None;
        for pass in 0..2 {
            for (ci, c) in cands.iter().enumerate() {
                if let Some(a) = cue_asset {
                    if &inp.footage[c.fi].asset_id != a {
                        continue;
                    }
                    // First pass: only the scene that contains the requested source moment.
                    if let (0, Some(src)) = (pass, cue_src) {
                        if src < c.span_start || src >= c.span_end {
                            continue;
                        }
                    }
                }
                let span = c.span_end - c.span_start;
                let mut spd = speed;
                if span < need(spd) {
                    if pass == 0 {
                        continue;
                    }
                    // Second pass: allow mild slow motion to stretch short scenes.
                    spd = (span.0 as f64 / need(1.0).0 as f64).max(0.5);
                    if span < need(spd) || spd > 1.0 {
                        continue;
                    }
                }
                let s = c.scene;
                let key = (c.fi, s.index);
                let energy_term = 1.0 - (c.energy_rank - target_e).abs();
                let action = if is_drop || kind == SectionKind::Drop {
                    s.motion_peak
                } else {
                    0.0
                };
                let niche = inp.niche.map(|n| n.tag_score(&s.tags)).unwrap_or(0.0);
                let fit = (span.as_seconds_f64() / need(spd).as_seconds_f64().max(0.01)).min(3.0)
                    as f32
                    / 3.0;
                let rep = usage
                    .get(&key)
                    .map(|u| (*u as f32).powf(1.3))
                    .unwrap_or(0.0);
                let recent_pen = recent
                    .iter()
                    .rev()
                    .take(4)
                    .enumerate()
                    .map(|(i, r)| if *r == key { 1.0 - i as f32 * 0.2 } else { 0.0 })
                    .sum::<f32>();
                let cont = match prev {
                    Some((pf, ps, _)) if pf == c.fi && s.index == ps + 1 => 1.0,
                    Some((pf, _, _)) if pf == c.fi => 0.3,
                    Some(_) => -0.3,
                    None => 0.0,
                };
                let avoid = if inp
                    .avoid_scenes
                    .iter()
                    .any(|(a, i)| a == &inp.footage[c.fi].asset_id && *i == s.index)
                {
                    5.0
                } else {
                    0.0
                };
                let slow_pen = if spd < 1.0 && speed >= 1.0 && cue_asset.is_none() {
                    0.3
                } else {
                    0.0
                };
                let score = w.energy_match * energy_term
                    + w.quality * s.quality
                    + w.action_alignment * action
                    + w.niche_tags * niche
                    + w.duration_fit * fit
                    + eff.continuity_weight * cont
                    - w.repetition_penalty * rep
                    - w.diversity * recent_pen
                    - slow_pen
                    - avoid
                    + eff.temperature * 2.0 * rng_scene.next_f64() as f32;
                if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
                    let reason = format!(
                        "{}: energy {:.2}→{:.2}{}{}",
                        section_name(kind),
                        c.energy_rank,
                        target_e,
                        if action > 0.5 { ", action peak" } else { "" },
                        if spd < 1.0 { ", slow motion" } else { "" }
                    );
                    best = Some((score, ci, spd, reason));
                }
            }
            if best.is_some() {
                break;
            }
        }
        // Last resort: the longest scene, looped by speed clamp (rare: extremely short footage).
        let (score, ci, spd, reason) = match best {
            Some(b) => b,
            None => {
                let (ci, _) = cands
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, c)| (c.span_end - c.span_start).0)
                    .unwrap();
                warnings.push("Footage is shorter than some planned shots; some shots use the longest available scene.".into());
                tr = None;
                (0.0, ci, 0.5, "fallback: longest scene".into())
            }
        };
        let c = &cands[ci];
        let need_src = need(spd);
        let span = c.span_end - c.span_start;
        let max_in = c.span_end - need_src;
        let lo_in = c.span_start + handle.scale_f64(spd);
        let max_in = max_in.max(lo_in);
        let mut source_in = lo_in;
        let continuing = matches!(prev, Some((pf, ps, pout)) if pf == c.fi && ps == c.scene.index && pout >= lo_in && pout <= max_in);
        if let (Some(_), Some(src)) = (cue_asset, cue_src) {
            // The pinned source moment appears exactly at the cut.
            let clamped = src.clamp(
                c.span_start,
                (c.span_end - d.scale_f64(spd)).max(c.span_start),
            );
            if clamped != src {
                warnings.push(format!(
                    "Cue at {:.2}s: source moment {:.2}s is too close to a scene boundary; used {:.2}s.",
                    gs.as_seconds_f64(),
                    src.as_seconds_f64(),
                    clamped.as_seconds_f64()
                ));
            }
            source_in = clamped;
            tr = None; // a dissolve would show the moment early
        } else if continuing {
            source_in = prev.unwrap().2;
        } else if let (true, Some(peak)) =
            (is_drop || kind == SectionKind::Drop, c.scene.motion_peak_t)
        {
            // Land the action peak just after the cut.
            let lead = Tick::from_seconds_f64(0.08).scale_f64(spd);
            source_in = (peak - lead).clamp(lo_in, max_in);
        } else if matches!(kind, SectionKind::Intro) {
            source_in = lo_in;
        } else if max_in > lo_in {
            // Least-reused offset among a few candidates.
            let ranges = used_ranges.get(&c.fi).cloned().unwrap_or_default();
            let mut best_off = (i64::MAX, lo_in);
            for k in 0..6 {
                let u = if k == 0 {
                    rng_scene.next_f64()
                } else {
                    k as f64 / 5.0
                };
                let cand_in = lo_in + Tick(((max_in - lo_in).0 as f64 * u) as i64);
                let cand_out = cand_in + need_src;
                let overlap: i64 = ranges
                    .iter()
                    .map(|(a0, a1)| (cand_out.min(*a1) - cand_in.max(*a0)).0.max(0))
                    .sum();
                if overlap < best_off.0 {
                    best_off = (overlap, cand_in);
                }
            }
            source_in = best_off.1;
        }
        let _ = span;
        let key = (c.fi, c.scene.index);
        *usage.entry(key).or_insert(0) += 1;
        used_ranges
            .entry(c.fi)
            .or_default()
            .push((source_in, source_in + need_src));
        recent.push(key);
        prev = Some((c.fi, c.scene.index, source_in + d.scale_f64(spd)));
        transitions.push(tr);
        segments.push(PlannedSegment {
            start: gs,
            end: ge,
            section: kind,
            target_energy: target_e,
            asset_id: inp.footage[c.fi].asset_id.clone(),
            scene_index: c.scene.index,
            source_in,
            speed: spd,
            score,
            velocity,
            locked: false,
            reason: if is_main_drop {
                format!("MAIN DROP — {reason}")
            } else {
                reason
            },
            is_drop,
        });
    }

    // 5. Build clips.
    let out_ar = inp.target.width as f32 / inp.target.height as f32;
    let mut fx_n = 0u32;
    let mut next_fx_id = || {
        fx_n += 1;
        format!(
            "fx-{}-{:04x}-{:04}",
            inp.variant.code(),
            inp.seed & 0xffff,
            fx_n
        )
    };
    let grade = &p.effects.color;
    let has_grade = grade.brightness.abs() > 1e-3
        || (grade.contrast - 1.0).abs() > 1e-3
        || (grade.saturation - 1.0).abs() > 1e-3;
    let mut clips: Vec<Clip> = Vec::new();
    for (gi, seg) in segments.iter().enumerate() {
        let locked_src = if seg.locked {
            locked
                .iter()
                .find(|c| (c.start - seg.start).abs() <= frame)
                .copied()
        } else {
            None
        };
        if let (Some(lc), false) = (locked_src, inp.regenerate_locked_effects) {
            let mut kept = lc.clone();
            kept.locked = true;
            clips.push(kept);
            continue;
        }
        let f = inp
            .footage
            .iter()
            .find(|f| f.asset_id == seg.asset_id)
            .unwrap();
        let scene = &f.analysis.scenes[seg.scene_index as usize];
        let d = seg.end - seg.start;
        let mut clip = Clip::new(
            {
                // Deterministic, and never colliding with a preserved (locked) clip id.
                let mut id = format!(
                    "clip-{}-{:04x}-{:03}",
                    inp.variant.code(),
                    inp.seed & 0xffff,
                    gi
                );
                while locked.iter().any(|c| c.id == id) {
                    id.push('x');
                }
                id
            },
            seg.asset_id.clone(),
            seg.start,
            d,
            seg.source_in,
        );
        clip.speed = seg.speed;
        if let Some(lc) = locked_src {
            clip.id = lc.id.clone();
            clip.locked = true;
        }
        let src_ar = f.width as f32 / f.height.max(1) as f32;
        clip.framing = if p.smart_crop && (src_ar - out_ar).abs() > 0.05 {
            Framing {
                center_x: 0.5 + (scene.focus_x - 0.5) * 0.7,
                center_y: 0.5 + (scene.focus_y - 0.5) * 0.7,
                ..Framing::default()
            }
        } else {
            Framing::default()
        };
        clip.transition_in = transitions[gi].clone();
        if let Some(lc) = locked_src {
            clip.framing = lc.framing.clone();
        }
        clip.origin = ClipOrigin {
            section: Some(section_name(seg.section).into()),
            reason: Some(seg.reason.clone()),
            scene_id: Some(format!("{}#{}", seg.asset_id, seg.scene_index)),
            score: Some(seg.score),
        };
        if has_grade {
            clip.effects.push(EffectInstance {
                id: next_fx_id(),
                kind: EffectKind::Color {
                    brightness: grade.brightness,
                    contrast: grade.contrast,
                    saturation: grade.saturation,
                },
                offset: Tick::ZERO,
                duration: d,
                version: 1,
                trigger: Some("grade".into()),
            });
        }
        if p.effects.vignette > 0.0 {
            clip.effects.push(EffectInstance {
                id: next_fx_id(),
                kind: EffectKind::Vignette {
                    strength: p.effects.vignette.min(1.0),
                },
                offset: Tick::ZERO,
                duration: d,
                version: 1,
                trigger: Some("grade".into()),
            });
        }
        // Slow push-in / pull-out on calm, long-enough shots.
        let calm = matches!(
            seg.section,
            SectionKind::Intro | SectionKind::Breakdown | SectionKind::Outro | SectionKind::Groove
        );
        if calm && p.effects.slow_zoom > 0.0 && d >= Tick::from_seconds_f64(1.5) {
            let delta = p.effects.slow_zoom * (0.5 + eff.effect_intensity);
            let (z0, z1) = if rng_fx.next_f64() < 0.7 {
                (1.0, 1.0 + delta)
            } else {
                (1.0 + delta, 1.0)
            };
            clip.keyframes = vec![
                TransformKeyframe {
                    t: Tick::ZERO,
                    zoom: z0,
                    offset_x: 0.0,
                    offset_y: 0.0,
                    ease: Ease::Linear,
                },
                TransformKeyframe {
                    t: d,
                    zoom: z1,
                    offset_x: 0.0,
                    offset_y: 0.0,
                    ease: Ease::EaseInOut,
                },
            ];
        }
        // Drop impact.
        if seg.is_drop && eff.drop_impact > 0.05 {
            let main = drop_times
                .iter()
                .any(|(dt, m)| *m && (*dt - seg.start).abs() <= frame);
            let scale = if main { 1.0 } else { 0.75 } * eff.drop_impact;
            let cap = |s: f64| Tick::from_seconds_f64(s).min(d);
            if p.drop.flash {
                clip.effects.push(fx(
                    next_fx_id(),
                    EffectKind::Flash {
                        strength: (0.85 * scale).min(1.0),
                    },
                    cap(0.3),
                    "drop",
                ));
            }
            if p.drop.zoom_punch > 0.0 {
                clip.effects.push(fx(
                    next_fx_id(),
                    EffectKind::ZoomPunch {
                        amount: (p.drop.zoom_punch * scale).min(1.0),
                    },
                    cap(0.6),
                    "drop",
                ));
            }
            if p.drop.rgb_split {
                clip.effects.push(fx(
                    next_fx_id(),
                    EffectKind::RgbSplit {
                        offset: (0.015 * scale).min(0.1),
                    },
                    cap(0.35),
                    "drop",
                ));
            }
            if p.drop.glitch > 0.0 {
                clip.effects.push(fx(
                    next_fx_id(),
                    EffectKind::Glitch {
                        strength: (p.drop.glitch * scale).min(1.0),
                        seed: rng_fx.next_u64() as u32,
                    },
                    cap(0.4),
                    "drop",
                ));
            }
            if p.drop.zoom_blur > 0.0 {
                clip.effects.push(fx(
                    next_fx_id(),
                    EffectKind::ZoomBlur {
                        amount: (p.drop.zoom_blur * scale).min(0.3),
                    },
                    cap(0.35),
                    "drop",
                ));
            }
            if p.drop.shake {
                clip.effects.push(fx(
                    next_fx_id(),
                    EffectKind::Shake {
                        amplitude: (0.02 * scale).min(0.2),
                        frequency_hz: 12.0,
                        seed: rng_fx.next_u64() as u32,
                    },
                    cap(0.45),
                    "drop",
                ));
            }
        }
        if let Some((frac, s1, s2)) = seg.velocity {
            // Rescale ramp speeds if scene selection had to change the average speed.
            let avg0 = frac * s1 + (1.0 - frac) * s2;
            let k = if avg0 > 0.0 { seg.speed / avg0 } else { 1.0 };
            let s2 = s2 * k;
            // The speed change itself lands on a musical event (35–80 % into the shot).
            let target = seg.start + Tick((d.0 as f64 * frac) as i64);
            let lo = seg.start + Tick((d.0 as f64 * 0.35) as i64);
            let hi = seg.start + Tick((d.0 as f64 * 0.8) as i64);
            let split = events
                .iter()
                .map(|e| e.t)
                .filter(|t| *t >= lo && *t <= hi)
                .min_by_key(|t| (*t - target).abs())
                .unwrap_or(target);
            let d1 = (split - seg.start).snap_to_frame(fps);
            // Keep the source budget (d * average speed) by solving for the fast speed.
            let budget = d.as_seconds_f64() * seg.speed;
            let s1 = if d1 > Tick::ZERO {
                ((budget - (d - d1).as_seconds_f64() * s2) / d1.as_seconds_f64()).clamp(0.25, 4.0)
            } else {
                s1 * k
            };
            if d1 >= frame * 2 && d - d1 >= frame * 2 {
                let mut b = clip.clone();
                clip.duration = d1;
                clip.speed = s1;
                clip.keyframes.clear();
                b.id = format!("{}b", clip.id);
                while locked.iter().any(|c| c.id == b.id) {
                    b.id.push('x');
                }
                b.start = seg.start + d1;
                b.duration = d - d1;
                b.speed = s2;
                b.source_in = seg.source_in + d1.scale_f64(s1);
                b.transition_in = None;
                b.keyframes.clear();
                // Effects: grade/vignette on both parts; timed effects stay in the part they start in.
                let (mut ea, mut eb) = (vec![], vec![]);
                for e in clip.effects.drain(..) {
                    if e.trigger.as_deref() == Some("grade") {
                        let mut a2 = e.clone();
                        a2.duration = d1;
                        ea.push(a2);
                        let mut b2 = e;
                        b2.id = format!("{}b", b2.id);
                        b2.duration = d - d1;
                        eb.push(b2);
                    } else if e.offset < d1 {
                        ea.push(e);
                    } else {
                        let mut b2 = e;
                        b2.offset -= d1;
                        eb.push(b2);
                    }
                }
                // Speed feel: directional motion blur on the fast part.
                let fast_a = s1 > s2;
                let blur_amt =
                    (0.02 * (s1.max(s2) - 1.0).max(0.0) as f32 * (0.5 + eff.effect_intensity))
                        .min(0.08);
                if blur_amt > 0.002 {
                    let angle = if scene.camera_dy.abs() > scene.camera_dx.abs() {
                        90.0
                    } else {
                        0.0
                    };
                    let (host, len) = if fast_a {
                        (&mut ea, d1)
                    } else {
                        (&mut eb, d - d1)
                    };
                    host.push(EffectInstance {
                        id: next_fx_id(),
                        kind: EffectKind::MotionBlur {
                            amount: blur_amt,
                            angle_deg: angle,
                        },
                        offset: Tick::ZERO,
                        duration: len,
                        version: 1,
                        trigger: Some("velocity".into()),
                    });
                }
                clip.effects = ea;
                b.effects = eb;
                clip.origin.reason =
                    Some(format!("{} · velocity {:.2}x→{:.2}x", seg.reason, s1, s2));
                b.origin.reason = clip.origin.reason.clone();
                clips.push(clip);
                clips.push(b);
                continue;
            }
        }
        clips.push(clip);
    }

    // Kick-triggered effects with cooldown, per-bar limits and pattern variation.
    if eff.effect_intensity > 0.02
        && (p.effects.kick_zoom > 0.0
            || p.effects.kick_shake > 0.0
            || p.effects.kick_glitch > 0.0
            || p.effects.kick_blur > 0.0
            || p.effects.kick_chroma > 0.0)
        && p.effects.max_per_bar > 0
    {
        let cooldown =
            Tick::from_seconds_f64(beat_secs * p.effects.cooldown_beats as f64).max(frame * 3);
        let bar = beat * 4;
        let mut last_t: Option<Tick> = None;
        let mut bar_counts: HashMap<i64, u32> = HashMap::new();
        // (pool index, weight): 0 zoom, 1 shake, 2 glitch, 3 motion blur, 4 chroma.
        let pool: Vec<(usize, f64)> = [
            (0, p.effects.kick_zoom, 1.0),
            (1, p.effects.kick_shake, 0.6),
            (2, p.effects.kick_glitch, 0.35),
            (3, p.effects.kick_blur, 0.5),
            (4, p.effects.kick_chroma, 0.4),
        ]
        .iter()
        .filter(|(_, v, _)| *v > 0.0)
        .map(|(i, _, w)| (*i, *w))
        .collect();
        let mut last_kinds: Vec<usize> = Vec::new();
        for k in a.kicks.iter().filter(|k| in_win(k.sample)) {
            let strength = k.intensity * k.confidence;
            if strength < eff.kick_threshold {
                continue;
            }
            let t = to_t(k.sample);
            let (kind, _, _) = section_at(t);
            if !p
                .effects
                .kick_sections
                .iter()
                .any(|s| s == section_name(kind))
            {
                continue;
            }
            if last_t.map(|l| t - l < cooldown).unwrap_or(false) {
                continue;
            }
            if drop_times
                .iter()
                .any(|(dt, _)| (t - *dt).abs() < Tick::from_seconds_f64(0.25))
            {
                continue;
            }
            let bar_i = t.0 / bar.0.max(1);
            let cnt = bar_counts.entry(bar_i).or_insert(0);
            if *cnt >= p.effects.max_per_bar {
                continue;
            }
            let Some(ci) = clips.iter().position(|c| {
                t >= c.start && t < c.end() && (!c.locked || inp.regenerate_locked_effects)
            }) else {
                continue;
            };
            let clip = &mut clips[ci];
            let off = t - clip.start;
            let remain = clip.duration - off;
            let dur = Tick::from_seconds_f64((beat_secs * 0.75).min(0.35)).min(remain);
            if dur < frame * 2 {
                continue;
            }
            // Weighted pick; never the same effect three times in a row.
            let banned = if last_kinds.len() >= 2
                && last_kinds[last_kinds.len() - 1] == last_kinds[last_kinds.len() - 2]
                && pool.len() > 1
            {
                Some(last_kinds[last_kinds.len() - 1])
            } else {
                None
            };
            let opts: Vec<(usize, f64)> = pool
                .iter()
                .copied()
                .filter(|(i, _)| Some(*i) != banned)
                .collect();
            let total: f64 = opts.iter().map(|(_, w)| w).sum();
            let mut r = rng_fx.next_f64() * total;
            let mut pick = opts[0].0;
            for (i, w) in &opts {
                if r < *w {
                    pick = *i;
                    break;
                }
                r -= w;
            }
            let amt = 0.6 + 0.4 * k.intensity;
            let s = eff.effect_intensity * 1.4 * amt;
            let kind = match pick {
                0 => EffectKind::ZoomPunch {
                    amount: (p.effects.kick_zoom * s).min(1.0),
                },
                1 => EffectKind::Shake {
                    amplitude: (p.effects.kick_shake * s).min(0.2),
                    frequency_hz: 14.0,
                    seed: rng_fx.next_u64() as u32,
                },
                2 => EffectKind::Glitch {
                    strength: (p.effects.kick_glitch * s).min(1.0),
                    seed: rng_fx.next_u64() as u32,
                },
                3 => EffectKind::MotionBlur {
                    amount: (p.effects.kick_blur * s).min(0.2),
                    angle_deg: if rng_fx.next_f64() < 0.5 { 0.0 } else { 90.0 },
                },
                _ => EffectKind::ChromaticAberration {
                    amount: (p.effects.kick_chroma * s).min(0.1),
                },
            };
            let zoom = pick;
            clip.effects.push(EffectInstance {
                id: next_fx_id(),
                kind,
                offset: off,
                duration: dur,
                version: 1,
                trigger: Some("kick".into()),
            });
            last_kinds.push(zoom);
            last_t = Some(t);
            *cnt += 1;
        }
    }

    // Fades.
    if p.fade_in_secs > 0.0 {
        if let Some(c) = clips
            .first_mut()
            .filter(|c| !c.locked || inp.regenerate_locked_effects)
        {
            let d = Tick::from_seconds_f64(p.fade_in_secs as f64).min(c.duration);
            c.effects.push(fx(
                next_fx_id(),
                EffectKind::Fade { fade_in: true },
                d,
                "fade",
            ));
        }
    }
    let fade_out = Tick::from_seconds_f64(p.fade_out_secs as f64);
    if p.fade_out_secs > 0.0 {
        if let Some(c) = clips
            .last_mut()
            .filter(|c| !c.locked || inp.regenerate_locked_effects)
        {
            let d = fade_out.min(c.duration);
            c.effects.push(EffectInstance {
                id: next_fx_id(),
                kind: EffectKind::Fade { fade_in: false },
                offset: c.duration - d,
                duration: d,
                version: 1,
                trigger: Some("fade".into()),
            });
        }
    }

    // 6. Timeline.
    let settings = TimelineSettings {
        width: inp.target.width,
        height: inp.target.height,
        fps,
        sample_rate: 48000,
        background: [0, 0, 0, 255],
    };
    let mut tl = Timeline::new(settings);
    let mut v1 = Track::new("V1", TrackKind::Video, "Video 1");
    v1.clips = clips;
    let mut a1 = Track::new("A1", TrackKind::Audio, "Music");
    let mut music = Clip::new(
        "music",
        inp.music_asset_id.clone(),
        Tick::ZERO,
        t_end,
        Tick::from_samples(ws, sr),
    );
    music.fade_in = if ws > 0 {
        Tick::from_seconds_f64(0.03)
    } else {
        Tick::ZERO
    };
    music.fade_out = if p.fade_out_secs > 0.0 {
        fade_out.max(Tick::from_seconds_f64(0.05))
    } else {
        Tick::from_seconds_f64(0.05)
    };
    a1.clips.push(music);
    tl.tracks.push(v1);
    tl.tracks.push(a1);
    tl.duration = Some(t_end);
    for (i, (dt, main)) in drop_times.iter().enumerate() {
        tl.markers.push(Marker {
            id: format!("m-drop-{i}"),
            t: *dt,
            label: if *main {
                "Main drop".into()
            } else {
                "Drop".into()
            },
            kind: MarkerKind::Drop,
        });
    }
    for (i, s) in a
        .sections
        .iter()
        .filter(|s| in_win(s.start_sample) || s.start_sample <= ws && s.end_sample > ws)
        .enumerate()
    {
        let t = if s.start_sample <= ws {
            Tick::ZERO
        } else {
            to_t(s.start_sample)
        };
        tl.markers.push(Marker {
            id: format!("m-sec-{i}"),
            t,
            label: section_name(s.kind).replace('_', " "),
            kind: MarkerKind::Section,
        });
    }

    // 7. Validate.
    let mut assets: HashMap<String, AssetInfo> = inp
        .footage
        .iter()
        .map(|f| {
            (
                f.asset_id.clone(),
                AssetInfo {
                    duration: f.duration,
                    has_video: true,
                    has_audio: false,
                    is_image: false,
                },
            )
        })
        .collect();
    assets.insert(
        inp.music_asset_id.clone(),
        AssetInfo {
            duration: Tick::from_samples(a.duration_samples, sr),
            has_video: false,
            has_audio: true,
            is_image: false,
        },
    );
    let mut issues = validate(&tl, &assets);
    // Self-repair: drop transitions that lack handles.
    if issues
        .iter()
        .any(|i| i.severity == ValidationSeverity::Error && i.message.contains("handles"))
    {
        let bad: Vec<String> = issues
            .iter()
            .filter(|i| i.message.contains("handles"))
            .filter_map(|i| i.clip_id.clone())
            .collect();
        for c in tl.tracks[0].clips.iter_mut() {
            if bad.contains(&c.id) {
                c.transition_in = None;
            }
        }
        issues = validate(&tl, &assets);
    }
    if issues
        .iter()
        .any(|i| i.severity == ValidationSeverity::Error)
    {
        return Err(
            StudioError::validation("The generated edit failed validation.")
                .with_details(format!("{issues:?}")),
        );
    }
    let cues_out: Vec<ResolvedCue> = cue_times
        .iter()
        .map(|(t, i)| {
            let cue = &inp.cues[*i];
            let seg_index = segments
                .iter()
                .position(|sg| (sg.start - *t).abs() <= frame);
            let clip_index = tl.tracks[0]
                .clips
                .iter()
                .position(|c| (c.start - *t).abs() <= frame);
            let honored_source = match (seg_index, &cue.asset_id, cue.source_secs) {
                (Some(ci), Some(a), Some(src)) => {
                    &segments[ci].asset_id == a
                        && (segments[ci].source_in - Tick::from_seconds_f64(src)).abs() <= frame
                }
                (Some(ci), Some(a), None) => &segments[ci].asset_id == a,
                (Some(_), None, _) => true,
                _ => false,
            };
            ResolvedCue {
                requested: Tick::from_seconds_f64(cue.at_secs),
                resolved: *t,
                clip_index,
                honored_source,
            }
        })
        .collect();
    Ok(EditPlan {
        cues: cues_out,
        timeline: tl,
        audio_start_sample: ws,
        audio_end_sample: we,
        segments,
        warnings,
        issues,
        seed: inp.seed,
        variant: inp.variant,
        preset_id: p.id.clone(),
    })
}

fn fx(id: String, kind: EffectKind, duration: Tick, trigger: &str) -> EffectInstance {
    EffectInstance {
        id,
        kind,
        offset: Tick::ZERO,
        duration,
        version: 1,
        trigger: Some(trigger.into()),
    }
}
