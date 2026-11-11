//! Planner tests on a real analysis of a synthetic track plus scene descriptors.

use std::sync::OnceLock;

use studio_audio::{analyze, synth, AnalysisConfig, AudioAnalysis, DecodedAudio};
use studio_core::cancel::noop_progress;
use studio_core::{CancelToken, Rational, Tick};
use studio_planner::*;
use studio_vision::{Scene, VideoAnalysis};

fn audio() -> &'static AudioAnalysis {
    static A: OnceLock<AudioAnalysis> = OnceLock::new();
    A.get_or_init(|| {
        let tr = synth::render(&synth::edm_arrangement(), 128.0, 44100, 0.5, 1);
        analyze(
            &DecodedAudio::from_mono(tr.samples, 44100),
            &AnalysisConfig::default(),
            &CancelToken::new(),
            &noop_progress(),
        )
        .unwrap()
    })
}

fn scene(i: u32, start: f64, end: f64, energy: f32) -> Scene {
    Scene {
        index: i,
        start: Tick::from_seconds_f64(start),
        end: Tick::from_seconds_f64(end),
        motion: energy,
        motion_peak: energy,
        motion_peak_t: Some(Tick::from_seconds_f64((start + end) / 2.0)),
        impacts: vec![],
        brightness: 0.5,
        contrast: 0.2,
        saturation: 0.4,
        sharpness: 0.7,
        black_ratio: 0.0,
        stability: 0.9,
        camera_dx: 0.0,
        camera_dy: 0.0,
        energy,
        quality: 0.6 + 0.3 * ((i % 3) as f32 / 2.0),
        focus_x: 0.4 + 0.05 * (i % 4) as f32,
        focus_y: 0.5,
        tags: if energy > 0.6 {
            vec!["high_motion".into(), "action_peak".into()]
        } else {
            vec!["low_motion".into()]
        },
        usable: true,
    }
}

fn footage() -> Vec<VideoAnalysis> {
    (0..3)
        .map(|f| {
            let scenes: Vec<Scene> = (0..12)
                .map(|i| {
                    scene(
                        i,
                        i as f64 * 5.0,
                        i as f64 * 5.0 + 5.0,
                        ((i * 7 + f * 3) % 10) as f32 / 10.0,
                    )
                })
                .collect();
            VideoAnalysis {
                version: 1,
                analysis_fps: Rational::new(15, 1).unwrap(),
                frame_count: 900,
                width: 160,
                height: 90,
                cuts: vec![],
                scenes,
                curves: Default::default(),
            }
        })
        .collect()
}

fn preset(id: &str) -> EditPreset {
    builtin_presets().into_iter().find(|p| p.id == id).unwrap()
}

fn plan(
    fa: &[VideoAnalysis],
    p: &EditPreset,
    controls: CreativeControls,
    seed: u64,
    variant: Variant,
    secs: f64,
) -> EditPlan {
    let footage = fa
        .iter()
        .enumerate()
        .map(|(i, a)| FootageSource {
            asset_id: format!("vid{i}"),
            analysis: a,
            duration: Tick::from_seconds_f64(60.0),
            width: 1920,
            height: 1080,
        })
        .collect();
    plan_edit(&PlanInput {
        audio: audio(),
        music_asset_id: "music".into(),
        footage,
        preset: p,
        controls,
        target: OutputTarget {
            width: 1080,
            height: 1920,
            fps: Rational::FPS_30,
            duration: Tick::from_seconds_f64(secs),
        },
        seed,
        variant,
        niche: None,
        cues: vec![],

        locked: vec![],

        regenerate_locked_effects: false,

        audio_start: None,

        drop_position: None,

        avoid_scenes: vec![],
    })
    .expect("plan")
}

fn video(p: &EditPlan) -> &Vec<studio_timeline::Clip> {
    &p.timeline.tracks[0].clips
}

#[test]
fn deterministic_for_same_seed() {
    let fa = footage();
    let p = preset("high-energy-edm");
    let a = plan(
        &fa,
        &p,
        CreativeControls::default(),
        42,
        Variant::Balanced,
        20.0,
    );
    let b = plan(
        &fa,
        &p,
        CreativeControls::default(),
        42,
        Variant::Balanced,
        20.0,
    );
    assert_eq!(
        serde_json::to_string(&a.timeline).unwrap(),
        serde_json::to_string(&b.timeline).unwrap()
    );
    let c = plan(
        &fa,
        &p,
        CreativeControls::default(),
        43,
        Variant::Balanced,
        20.0,
    );
    assert_ne!(
        serde_json::to_string(&a.timeline).unwrap(),
        serde_json::to_string(&c.timeline).unwrap()
    );
}

#[test]
fn structure_duration_drop_and_beat_alignment() {
    let fa = footage();
    let p = preset("high-energy-edm");
    let pl = plan(
        &fa,
        &p,
        CreativeControls::default(),
        7,
        Variant::Balanced,
        20.0,
    );
    let a = audio();
    let fps = Rational::FPS_30;
    assert_eq!(
        pl.timeline.duration(),
        Tick::from_seconds_f64(20.0).snap_to_frame(fps)
    );
    // Clips tile the timeline with no gaps.
    let v = video(&pl);
    assert_eq!(v[0].start, Tick::ZERO);
    for w in v.windows(2) {
        assert_eq!(w[0].end(), w[1].start);
    }
    assert_eq!(v.last().unwrap().end(), pl.timeline.duration());
    // A cut lands on the main drop, and the drop sits near the preset position.
    let drop = pl
        .timeline
        .markers
        .iter()
        .find(|m| m.label == "Main drop")
        .expect("drop in window");
    assert!(v.iter().any(|c| c.start == drop.t), "no cut at the drop");
    let pos = drop.t.as_seconds_f64() / 20.0;
    assert!(
        (pos - p.drop.position as f64).abs() < 0.12,
        "drop position {pos}"
    );
    let drop_clip = v.iter().find(|c| c.start == drop.t).unwrap();
    assert!(drop_clip
        .effects
        .iter()
        .any(|e| e.trigger.as_deref() == Some("drop")));
    // Cuts follow musical events.
    let ws = pl.audio_start_sample;
    let ev: Vec<Tick> = a
        .beats
        .iter()
        .map(|b| b.sample)
        .chain(a.kicks.iter().map(|k| k.sample))
        .chain(a.sections.iter().map(|s| s.start_sample))
        .map(|s| Tick::from_samples(s - ws, a.sample_rate))
        .collect();
    let aligned = v
        .iter()
        .skip(1)
        .filter(|c| {
            ev.iter()
                .any(|e| (*e - c.start).abs() <= fps.frame_duration())
        })
        .count();
    let ratio = aligned as f64 / (v.len() - 1).max(1) as f64;
    println!("{} clips, beat-aligned ratio {ratio:.2}", v.len());
    assert!(ratio >= 0.9, "aligned ratio {ratio}");
    // Shots respect the minimum length.
    let min = Tick::from_seconds_f64(0.15);
    assert!(v.iter().all(|c| c.duration >= min));
    // Kick effects exist, only in drop sections, and respect the cooldown.
    let kicks: Vec<Tick> = v
        .iter()
        .flat_map(|c| {
            c.effects
                .iter()
                .filter(|e| e.trigger.as_deref() == Some("kick"))
                .map(move |e| c.start + e.offset)
        })
        .collect();
    assert!(!kicks.is_empty());
    for w in kicks.windows(2) {
        assert!(w[1] - w[0] >= fps.frame_duration() * 3);
    }
}

#[test]
fn presets_and_controls_change_behaviour() {
    let fa = footage();
    let edm = plan(
        &fa,
        &preset("high-energy-edm"),
        CreativeControls::default(),
        1,
        Variant::Balanced,
        20.0,
    );
    let ambient = plan(
        &fa,
        &preset("atmospheric-ambient"),
        CreativeControls::default(),
        1,
        Variant::Balanced,
        20.0,
    );
    let minimal = plan(
        &fa,
        &preset("minimal-clean"),
        CreativeControls::default(),
        1,
        Variant::Balanced,
        20.0,
    );
    println!(
        "edm {} clips, ambient {}, minimal {}",
        video(&edm).len(),
        video(&ambient).len(),
        video(&minimal).len()
    );
    assert!(video(&edm).len() > 2 * video(&ambient).len());
    assert!(video(&minimal).iter().all(|c| c
        .effects
        .iter()
        .all(|e| e.trigger.as_deref() != Some("kick"))));
    let dense = plan(
        &fa,
        &preset("high-energy-edm"),
        CreativeControls {
            cut_density: 1.0,
            ..Default::default()
        },
        1,
        Variant::Balanced,
        20.0,
    );
    let sparse = plan(
        &fa,
        &preset("high-energy-edm"),
        CreativeControls {
            cut_density: 0.0,
            ..Default::default()
        },
        1,
        Variant::Balanced,
        20.0,
    );
    println!(
        "dense {} sparse {}",
        video(&dense).len(),
        video(&sparse).len()
    );
    assert!(video(&dense).len() > video(&sparse).len());
    let no_fx = plan(
        &fa,
        &preset("high-energy-edm"),
        CreativeControls {
            effect_intensity: 0.0,
            ..Default::default()
        },
        1,
        Variant::Balanced,
        20.0,
    );
    let kick_fx = |p: &EditPlan| {
        video(p)
            .iter()
            .map(|c| {
                c.effects
                    .iter()
                    .filter(|e| e.trigger.as_deref() == Some("kick"))
                    .count()
            })
            .sum::<usize>()
    };
    assert_eq!(kick_fx(&no_fx), 0);
}

#[test]
fn variants_are_meaningfully_different() {
    let fa = footage();
    let p = preset("high-energy-edm");
    let plans: Vec<EditPlan> = Variant::ALL
        .iter()
        .map(|v| plan(&fa, &p, CreativeControls::default(), 5, *v, 20.0))
        .collect();
    for i in 0..plans.len() {
        for j in i + 1..plans.len() {
            let s = similarity(&plans[i].timeline, &plans[j].timeline);
            println!("{:?} vs {:?}: {s:.2}", plans[i].variant, plans[j].variant);
            assert!(
                s < 0.85,
                "{:?} vs {:?} too similar: {s}",
                plans[i].variant,
                plans[j].variant
            );
        }
    }
    assert!((similarity(&plans[0].timeline, &plans[0].timeline) - 1.0).abs() < 1e-6);
}

#[test]
fn repetition_is_limited() {
    let fa = footage();
    let pl = plan(
        &fa,
        &preset("fast-paced-montage"),
        CreativeControls::default(),
        9,
        Variant::Balanced,
        30.0,
    );
    let v = video(&pl);
    // No identical scene twice in a row.
    for w in pl.segments.windows(2) {
        assert!(
            !(w[0].asset_id == w[1].asset_id
                && w[0].scene_index == w[1].scene_index
                && w[0].source_in == w[1].source_in)
        );
    }
    let mut keys: Vec<(String, u32)> = pl
        .segments
        .iter()
        .map(|s| (s.asset_id.clone(), s.scene_index))
        .collect();
    let n = keys.len();
    keys.sort();
    keys.dedup();
    println!("{} segments use {} distinct scenes", n, keys.len());
    assert!(keys.len() as f64 >= (n as f64 * 0.6).min(36.0));
    assert!(
        v.len() >= n,
        "velocity ramps may split a segment into two clips"
    );
}

#[test]
fn rejects_missing_footage() {
    let p = preset("high-energy-edm");
    let r = plan_edit(&PlanInput {
        audio: audio(),
        music_asset_id: "m".into(),
        footage: vec![],
        preset: &p,
        controls: CreativeControls::default(),
        target: OutputTarget {
            width: 1080,
            height: 1920,
            fps: Rational::FPS_30,
            duration: Tick::from_seconds_f64(10.0),
        },
        seed: 1,
        variant: Variant::Balanced,
        niche: None,
        cues: vec![],

        locked: vec![],

        regenerate_locked_effects: false,

        audio_start: None,

        drop_position: None,

        avoid_scenes: vec![],
    });
    assert!(r.is_err());
}

fn plan_with_cues(fa: &[VideoAnalysis], cues: Vec<Cue>) -> EditPlan {
    let footage = fa
        .iter()
        .enumerate()
        .map(|(i, a)| FootageSource {
            asset_id: format!("vid{i}"),
            analysis: a,
            duration: Tick::from_seconds_f64(60.0),
            width: 1920,
            height: 1080,
        })
        .collect();
    let p = preset("high-energy-edm");
    plan_edit(&PlanInput {
        audio: audio(),
        music_asset_id: "music".into(),
        footage,
        preset: &p,
        controls: CreativeControls::default(),
        target: OutputTarget {
            width: 1080,
            height: 1920,
            fps: Rational::FPS_30,
            duration: Tick::from_seconds_f64(20.0),
        },
        seed: 3,
        variant: Variant::Balanced,
        niche: None,
        cues,
        locked: vec![],
        regenerate_locked_effects: false,
        audio_start: None,
        drop_position: None,
        avoid_scenes: vec![],
    })
    .expect("plan")
}

#[test]
fn cues_pin_clip_moments_to_seconds_on_the_beat() {
    let fa = footage();
    let fps = Rational::FPS_30;
    let base = plan_with_cues(&fa, vec![]);
    let ws = base.audio_start_sample;
    let a = audio();
    let beats: Vec<Tick> = a
        .beats
        .iter()
        .map(|b| b.sample)
        .chain(a.kicks.iter().map(|k| k.sample))
        .map(|s| Tick::from_samples(s - ws, a.sample_rate))
        .collect();
    let beat = Tick::from_seconds_f64(60.0 / a.tempo.bpm);

    // Cue 1: "vid2 at source 21.0 s must appear at ~5.1 s, on the rhythm".
    // Cue 2: exact time, no snapping: vid1 at 13.37 s.
    let pl = plan_with_cues(
        &fa,
        vec![
            Cue {
                at_secs: 5.1,
                asset_id: Some("vid2".into()),
                source_secs: Some(21.0),
                snap_to_beat: true,
                label: "hook".into(),
            },
            Cue {
                at_secs: 13.37,
                asset_id: Some("vid1".into()),
                source_secs: None,
                snap_to_beat: false,
                label: String::new(),
            },
        ],
    );
    assert_eq!(pl.cues.len(), 2);
    for c in &pl.cues {
        assert!(c.honored_source, "{c:?}");
    }
    let v = &pl.timeline.tracks[0].clips;
    let c1 = &v[pl.cues[0].clip_index.unwrap()];
    assert_eq!(c1.asset_id, "vid2");
    assert_eq!(c1.source_in, Tick::from_seconds_f64(21.0));
    // Snapped onto a musical event, within half a beat of the request.
    assert!(
        beats
            .iter()
            .any(|b| (*b - c1.start).abs() <= fps.frame_duration()),
        "cue not on a beat"
    );
    assert!(
        (c1.start - Tick::from_seconds_f64(5.1)).abs() <= Tick(beat.0 / 2) + fps.frame_duration()
    );
    let c2 = &v[pl.cues[1].clip_index.unwrap()];
    assert_eq!(c2.asset_id, "vid1");
    assert_eq!(c2.start, Tick::from_seconds_f64(13.37).snap_to_frame(fps));
    // The rest of the edit is still valid and gap-free.
    for w in v.windows(2) {
        assert_eq!(w[0].end(), w[1].start);
    }
    // A moment too close to its scene's end is moved and reported, never silently changed.
    let late = plan_with_cues(
        &fa,
        vec![Cue {
            at_secs: 5.1,
            asset_id: Some("vid2".into()),
            source_secs: Some(24.7),
            ..Default::default()
        }],
    );
    assert!(!late.cues[0].honored_source);
    assert!(
        late.warnings.iter().any(|w| w.contains("scene boundary")),
        "{:?}",
        late.warnings
    );
    // Out-of-range cue is reported, not silently dropped.
    let bad = plan_with_cues(
        &fa,
        vec![Cue {
            at_secs: 99.0,
            ..Default::default()
        }],
    );
    assert!(bad.warnings.iter().any(|w| w.contains("outside")));
}

#[test]
fn velocity_ramp_into_drop_keeps_source_continuous() {
    let fa = footage();
    let pl = plan(
        &fa,
        &preset("aggressive-phonk"),
        CreativeControls::default(),
        4,
        Variant::Balanced,
        20.0,
    );
    let v = &pl.timeline.tracks[0].clips;
    let drop = pl
        .timeline
        .markers
        .iter()
        .find(|m| m.label == "Main drop")
        .expect("drop")
        .t;
    // The clip ending at the drop is the slow part of a ramp, preceded by its fast part.
    let i = v
        .iter()
        .position(|c| c.end() == drop)
        .expect("clip ending at drop");
    let (a, b) = (&v[i - 1], &v[i]);
    assert!(
        a.speed > 1.0 && b.speed < 1.0,
        "speeds {} -> {}",
        a.speed,
        b.speed
    );
    assert_eq!(a.asset_id, b.asset_id);
    assert_eq!(a.end(), b.start);
    // Source time continues exactly across the speed change (within 1 tick per rounding).
    assert!(
        (a.source_out() - b.source_in).abs() <= Tick(2),
        "{} vs {}",
        a.source_out(),
        b.source_in
    );
    assert!(a
        .effects
        .iter()
        .any(|e| matches!(e.kind, studio_timeline::EffectKind::MotionBlur { .. })));
    // The new kick pool is used and stays varied.
    let kinds: Vec<&str> = v
        .iter()
        .flat_map(|c| {
            c.effects
                .iter()
                .filter(|e| e.trigger.as_deref() == Some("kick"))
                .map(|e| e.kind.type_name())
        })
        .collect();
    for w in kinds.windows(3) {
        assert!(
            !(w[0] == w[1] && w[1] == w[2]),
            "same kick effect 3x in a row: {kinds:?}"
        );
    }
    let distinct: std::collections::BTreeSet<&str> = kinds.iter().copied().collect();
    println!("kick effects: {kinds:?}");
    assert!(distinct.len() >= 3, "{distinct:?}");
    // Drop gets the extended impact set.
    let dc = v.iter().find(|c| c.start == drop).unwrap();
    assert!(dc.effects.iter().any(|e| e.kind.type_name() == "glitch"));
    assert!(dc.effects.iter().any(|e| e.kind.type_name() == "zoom_blur"));
}

#[test]
fn pacing_is_phrased_not_one_cut_per_beat() {
    // A long, uniform drop section must not become a constant one-shot-per-beat grid.
    let fa = footage();
    for (preset_id, seed) in [
        ("brazilian-funk-velocity", 3u64),
        ("aggressive-phonk", 8),
        ("high-energy-edm", 1),
    ] {
        let pl = plan(
            &fa,
            &preset(preset_id),
            CreativeControls::default(),
            seed,
            Variant::Balanced,
            30.0,
        );
        let durs: Vec<i64> = pl
            .segments
            .iter()
            .map(|s| (s.end - s.start).to_frame_round(Rational::FPS_30))
            .collect();
        let mut run = 1;
        let mut max_run = 1;
        for w in durs.windows(2) {
            if (w[0] - w[1]).abs() <= 1 {
                run += 1;
                max_run = max_run.max(run);
            } else {
                run = 1;
            }
        }
        let distinct: std::collections::BTreeSet<i64> = durs.iter().map(|d| (d + 2) / 5).collect();
        println!(
            "{preset_id}: {} shots, max equal-length run {max_run}, durations(frames) {durs:?}",
            durs.len()
        );
        assert!(
            distinct.len() >= 3,
            "{preset_id}: shot lengths not varied: {durs:?}"
        );
        assert!(
            max_run <= 4,
            "{preset_id}: {max_run} equal-length shots in a row: {durs:?}"
        );
    }
}

fn plan_locked(
    fa: &[VideoAnalysis],
    seed: u64,
    locked: Vec<studio_timeline::Clip>,
    fx_only: bool,
    avoid: Vec<(String, u32)>,
    audio_start: Option<Tick>,
) -> EditPlan {
    let footage = fa
        .iter()
        .enumerate()
        .map(|(i, a)| FootageSource {
            asset_id: format!("vid{i}"),
            analysis: a,
            duration: Tick::from_seconds_f64(60.0),
            width: 1920,
            height: 1080,
        })
        .collect();
    let p = preset("aggressive-phonk");
    plan_edit(&PlanInput {
        audio: audio(),
        music_asset_id: "music".into(),
        footage,
        preset: &p,
        controls: CreativeControls::default(),
        target: OutputTarget {
            width: 1080,
            height: 1920,
            fps: Rational::FPS_30,
            duration: Tick::from_seconds_f64(20.0),
        },
        seed,
        variant: Variant::Balanced,
        niche: None,
        cues: vec![],
        locked,
        regenerate_locked_effects: fx_only,
        audio_start,
        drop_position: None,
        avoid_scenes: avoid,
    })
    .expect("plan")
}

fn video_clips(p: &EditPlan) -> Vec<studio_timeline::Clip> {
    p.timeline.tracks[0].clips.clone()
}

#[test]
fn regenerate_keeps_locked_clips_identical() {
    let fa = footage();
    let a = plan_locked(&fa, 1, vec![], false, vec![], None);
    let ws = Some(Tick::from_samples(
        a.audio_start_sample,
        audio().sample_rate,
    ));
    let five = Tick::from_seconds_f64(5.0);
    let locked: Vec<_> = video_clips(&a)
        .into_iter()
        .filter(|c| c.end() <= five)
        .map(|mut c| {
            c.locked = true;
            c
        })
        .collect();
    assert!(!locked.is_empty());
    let b = plan_locked(&fa, 99, locked.clone(), false, vec![], ws);
    let vb = video_clips(&b);
    for lc in &locked {
        let got = vb.iter().find(|c| c.id == lc.id).expect("locked clip kept");
        assert_eq!(got, lc, "locked clip changed");
    }
    // Rest of the edit is regenerated (different), valid and gap-free.
    for w in vb.windows(2) {
        assert_eq!(w[0].end(), w[1].start);
    }
    assert_eq!(vb.last().unwrap().end(), b.timeline.duration());
    let after_a: Vec<_> = video_clips(&a)
        .into_iter()
        .filter(|c| c.start >= five)
        .map(|c| (c.start, c.asset_id, c.source_in))
        .collect();
    let after_b: Vec<_> = vb
        .iter()
        .filter(|c| c.start >= five)
        .map(|c| (c.start, c.asset_id.clone(), c.source_in))
        .collect();
    assert_ne!(
        after_a, after_b,
        "unlocked part should change with a new seed"
    );
    // No kick effect was injected into a locked clip.
    assert!(locked
        .iter()
        .all(|lc| vb.iter().find(|c| c.id == lc.id).unwrap().effects == lc.effects));
}

#[test]
fn regenerate_selected_range_only() {
    let fa = footage();
    let a = plan_locked(&fa, 2, vec![], false, vec![], None);
    let ws = Some(Tick::from_samples(
        a.audio_start_sample,
        audio().sample_rate,
    ));
    let (r0, r1) = (Tick::from_seconds_f64(5.0), Tick::from_seconds_f64(12.0));
    // Lock everything that is not fully inside the selected range.
    let outside: Vec<_> = video_clips(&a)
        .into_iter()
        .filter(|c| !(c.start >= r0 && c.end() <= r1))
        .map(|mut c| {
            c.locked = true;
            c
        })
        .collect();
    let b = plan_locked(&fa, 77, outside.clone(), false, vec![], ws);
    let vb = video_clips(&b);
    for lc in &outside {
        assert_eq!(
            vb.iter().find(|c| c.id == lc.id),
            Some(lc),
            "clip outside the range changed"
        );
    }
    assert!(
        vb.iter().any(|c| !c.locked),
        "range must contain regenerated clips"
    );
}

#[test]
fn effects_only_and_replace_scene() {
    let fa = footage();
    let a = plan_locked(&fa, 3, vec![], false, vec![], None);
    let ws = Some(Tick::from_samples(
        a.audio_start_sample,
        audio().sample_rate,
    ));
    let all: Vec<_> = video_clips(&a)
        .into_iter()
        .map(|mut c| {
            c.locked = true;
            c
        })
        .collect();
    let b = plan_locked(&fa, 50, all.clone(), true, vec![], ws);
    let vb = video_clips(&b);
    assert_eq!(vb.len(), all.len());
    for (x, y) in all.iter().zip(vb.iter()) {
        assert_eq!(
            (
                x.id.clone(),
                x.start,
                x.duration,
                x.source_in,
                x.asset_id.clone()
            ),
            (
                y.id.clone(),
                y.start,
                y.duration,
                y.source_in,
                y.asset_id.clone()
            )
        );
    }
    let kicks = |v: &[studio_timeline::Clip]| {
        v.iter()
            .flat_map(|c| {
                c.effects
                    .iter()
                    .filter(|e| e.trigger.as_deref() == Some("kick"))
                    .map(|e| (c.start + e.offset, e.kind.type_name().to_string()))
            })
            .collect::<Vec<_>>()
    };
    assert_ne!(kicks(&all), kicks(&vb), "effects should be recomputed");

    // Replace one clip: lock all others, avoid its scene.
    let target = &video_clips(&a)[3];
    let scene: u32 = target
        .origin
        .scene_id
        .as_ref()
        .unwrap()
        .rsplit('#')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let others: Vec<_> = video_clips(&a)
        .into_iter()
        .filter(|c| c.id != target.id)
        .map(|mut c| {
            c.locked = true;
            c
        })
        .collect();
    let r = plan_locked(
        &fa,
        4,
        others,
        false,
        vec![(target.asset_id.clone(), scene)],
        ws,
    );
    let new = video_clips(&r)
        .into_iter()
        .find(|c| c.start == target.start)
        .expect("slot refilled");
    assert_eq!(new.duration, target.duration);
    assert!(
        new.asset_id != target.asset_id || new.origin.scene_id != target.origin.scene_id,
        "scene was not replaced"
    );
}

#[test]
fn clip_ids_are_unique_after_partial_regeneration() {
    let fa = footage();
    let a = plan_locked(&fa, 5, vec![], false, vec![], None);
    let ws = Some(Tick::from_samples(
        a.audio_start_sample,
        audio().sample_rate,
    ));
    let half: Vec<_> = video_clips(&a)
        .into_iter()
        .step_by(2)
        .map(|mut c| {
            c.locked = true;
            c
        })
        .collect();
    let b = plan_locked(&fa, 6, half, false, vec![], ws);
    let mut ids: Vec<String> = video_clips(&b).iter().map(|c| c.id.clone()).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n, "duplicate clip ids");
    let mut fx: Vec<String> = video_clips(&b)
        .iter()
        .flat_map(|c| c.effects.iter().map(|e| e.id.clone()))
        .collect();
    let m = fx.len();
    fx.sort();
    fx.dedup();
    assert_eq!(fx.len(), m, "duplicate effect ids");
}
