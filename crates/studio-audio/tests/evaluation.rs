//! Audio intelligence evaluation on synthetic tracks with exact annotations.
//! Beat tracking, kick detection and drop detection are evaluated separately.

use studio_audio::eval::f_measure;
use studio_audio::synth::{self, BassMode, KickPattern, SynthSection};
use studio_audio::{analyze, AnalysisConfig, DecodedAudio, DropKind, SectionKind};
use studio_core::cancel::noop_progress;
use studio_core::CancelToken;

const BEAT_TOL: f64 = 0.070; // ±70 ms beat F-measure tolerance
const KICK_TOL: f64 = 0.030; // kicks are refined to sample accuracy

fn run(tr: &synth::SynthTrack, cfg: &AnalysisConfig) -> studio_audio::AudioAnalysis {
    let audio = DecodedAudio::from_mono(tr.samples.clone(), tr.sample_rate);
    analyze(&audio, cfg, &CancelToken::new(), &noop_progress()).expect("analysis")
}

fn secs(a: &studio_audio::AudioAnalysis, v: impl Iterator<Item = i64>) -> Vec<f64> {
    v.map(|s| a.seconds(s)).collect()
}

#[test]
fn edm_arrangement_beats_kicks_drops() {
    let tr = synth::render(&synth::edm_arrangement(), 128.0, 44100, 0.5, 1);
    let a = run(&tr, &AnalysisConfig::default());

    // Tempo
    assert!((a.tempo.bpm - 128.0).abs() < 1.0, "bpm {}", a.tempo.bpm);

    // Beats
    let est = secs(&a, a.beats.iter().map(|b| b.sample));
    let beat = f_measure(&tr.beats, &est, BEAT_TOL);
    println!(
        "beat F={:.3} P={:.3} R={:.3}",
        beat.f_measure, beat.precision, beat.recall
    );
    assert!(beat.f_measure >= 0.90, "beat F-measure {beat:?}");

    // Kicks
    let est_k = secs(&a, a.kicks.iter().map(|k| k.sample));
    let kick = f_measure(&tr.kicks, &est_k, KICK_TOL);
    println!(
        "kick F={:.3} P={:.3} R={:.3} (n_est={})",
        kick.f_measure,
        kick.precision,
        kick.recall,
        est_k.len()
    );
    assert!(kick.f_measure >= 0.90, "kick F-measure {kick:?}");

    // Kick timing accuracy (mean absolute error of matched kicks).
    let mut errs = vec![];
    for r in &tr.kicks {
        if let Some(e) = est_k
            .iter()
            .map(|e| (e - r).abs())
            .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.min(d))))
        {
            if e < KICK_TOL {
                errs.push(e);
            }
        }
    }
    let mae = errs.iter().sum::<f64>() / errs.len().max(1) as f64;
    println!("kick MAE = {:.2} ms", mae * 1000.0);
    assert!(mae < 0.010, "kick MAE {mae}");

    // Drops: main drop must be near the first drop start (±1 beat).
    let beat_s = 60.0 / 128.0;
    let main = a.main_drop().expect("main drop");
    let main_t = a.seconds(main.sample);
    println!(
        "drops: {:?}",
        a.drops
            .iter()
            .map(|d| (a.seconds(d.sample), d.kind, d.score))
            .collect::<Vec<_>>()
    );
    assert!(
        tr.drops.iter().any(|d| (d - main_t).abs() <= beat_s + 0.05),
        "main drop at {main_t}, truth {:?}",
        tr.drops
    );
    // The second drop should be found too (secondary).
    let found_all = tr.drops.iter().all(|d| {
        a.drops.iter().any(|c| {
            c.kind != DropKind::FalseDropCandidate
                && (a.seconds(c.sample) - d).abs() <= beat_s + 0.05
        })
    });
    assert!(found_all, "not all drops found");

    // Sections: a Drop section must cover the middle of the first drop.
    let mid = tr.drops[0] + 8.0 * 4.0 * beat_s;
    let s = a.section_at((mid * 44100.0) as i64).unwrap();
    println!(
        "sections: {:?}",
        a.sections
            .iter()
            .map(|s| (a.seconds(s.start_sample), s.kind))
            .collect::<Vec<_>>()
    );
    assert_eq!(s.kind, SectionKind::Drop);
    // Downbeats: compare with truth.
    let est_d = secs(
        &a,
        a.beats.iter().filter(|b| b.is_downbeat).map(|b| b.sample),
    );
    let db = f_measure(&tr.downbeats, &est_d, BEAT_TOL);
    println!("downbeat F={:.3}", db.f_measure);
    assert!(db.f_measure >= 0.9, "downbeat {db:?}");

    // Snares (candidates): hi-hats must not be reported as snares.
    let est_s = secs(&a, a.snares.iter().map(|s| s.sample));
    let sn = f_measure(&tr.snares, &est_s, 0.05);
    println!(
        "snare F={:.3} P={:.3} R={:.3} (n_est={}, n_ref={})",
        sn.f_measure,
        sn.precision,
        sn.recall,
        est_s.len(),
        tr.snares.len()
    );
    assert!(sn.f_measure >= 0.8, "snare {sn:?}");
}

#[test]
fn sustained_and_wobble_bass_are_not_kicks() {
    let mut a1 = SynthSection::new(SectionKind::Groove, 8);
    a1.bass = BassMode::Sustained;
    a1.hats = true;
    let mut a2 = SynthSection::new(SectionKind::Groove, 8);
    a2.bass = BassMode::Wobble;
    a2.hats = true;
    let tr = synth::render(&[a1, a2], 140.0, 44100, 0.3, 2);
    let a = run(&tr, &AnalysisConfig::default());
    println!("false kicks: {}", a.kicks.len());
    // 16 bars at 8th-note wobble = 128 modulation cycles; allow only a handful.
    assert!(a.kicks.len() <= 4, "{} false kicks", a.kicks.len());
}

#[test]
fn distorted_hardtekk_kicks_at_170bpm() {
    let mut s = SynthSection::new(SectionKind::Drop, 16);
    s.kick = KickPattern::FourOnFloor;
    s.kick_distortion = 0.8;
    s.hats = true;
    s.bass = BassMode::Sustained;
    s.sidechain = true;
    let tr = synth::render(&[s], 170.0, 48000, 0.2, 3);
    let a = run(&tr, &AnalysisConfig::default());
    let ok_tempo = (a.tempo.bpm - 170.0).abs() < 2.0 || (a.tempo.bpm - 85.0).abs() < 1.0;
    assert!(ok_tempo, "tempo {}", a.tempo.bpm);
    let est_k = secs(&a, a.kicks.iter().map(|k| k.sample));
    let kick = f_measure(&tr.kicks, &est_k, KICK_TOL);
    println!("hardtekk kick F={:.3}", kick.f_measure);
    assert!(kick.f_measure >= 0.9, "{kick:?}");
}

/// Regression: long distorted kicks over an un-sidechained, equally loud sustained bass.
/// The band *level* barely rises at many onsets (tail sustain / antiphase cancellation);
/// detection relies on low-band spectral flux (pitch reset).
#[test]
fn distorted_kicks_without_sidechain() {
    let mut s = SynthSection::new(SectionKind::Drop, 16);
    s.kick = KickPattern::FourOnFloor;
    s.kick_distortion = 0.8;
    s.hats = true;
    s.bass = BassMode::Sustained;
    let tr = synth::render(&[s], 170.0, 48000, 0.2, 3);
    let a = run(&tr, &AnalysisConfig::default());
    let est_k = secs(&a, a.kicks.iter().map(|k| k.sample));
    let kick = f_measure(&tr.kicks, &est_k, KICK_TOL);
    assert!(kick.f_measure >= 0.9, "{kick:?}");
}

#[test]
fn syncopated_trap_kicks() {
    let mut s = SynthSection::new(SectionKind::Groove, 16);
    s.kick = KickPattern::Syncopated;
    s.snare_backbeat = true;
    s.hats = true;
    let tr = synth::render(&[s], 140.0, 44100, 0.2, 4);
    let a = run(&tr, &AnalysisConfig::default());
    let est_k = secs(&a, a.kicks.iter().map(|k| k.sample));
    let kick = f_measure(&tr.kicks, &est_k, KICK_TOL);
    println!(
        "trap kick F={:.3} P={:.3} R={:.3}",
        kick.f_measure, kick.precision, kick.recall
    );
    assert!(kick.f_measure >= 0.85, "{kick:?}");
}

#[test]
fn other_sample_rates() {
    for sr in [22050u32, 48000] {
        let tr = synth::render(&synth::edm_arrangement()[2..3], 125.0, sr, 0.25, 5);
        let a = run(&tr, &AnalysisConfig::default());
        assert!(
            (a.tempo.bpm - 125.0).abs() < 1.5,
            "sr {sr} bpm {}",
            a.tempo.bpm
        );
        let est = secs(&a, a.beats.iter().map(|b| b.sample));
        assert!(
            f_measure(&tr.beats, &est, BEAT_TOL).f_measure > 0.9,
            "sr {sr}"
        );
    }
}

#[test]
fn bpm_override_is_respected() {
    let tr = synth::render(&synth::edm_arrangement()[2..3], 128.0, 44100, 0.25, 6);
    let cfg = AnalysisConfig {
        bpm_override: Some(128.0),
        ..Default::default()
    };
    let a = run(&tr, &cfg);
    assert!(a.tempo.overridden);
    let est = secs(&a, a.beats.iter().map(|b| b.sample));
    assert!(f_measure(&tr.beats, &est, BEAT_TOL).f_measure > 0.9);
}

#[test]
fn silence_does_not_panic() {
    let audio = DecodedAudio::from_mono(vec![0.0; 44100 * 5], 44100);
    let a = analyze(
        &audio,
        &AnalysisConfig::default(),
        &CancelToken::new(),
        &noop_progress(),
    )
    .unwrap();
    assert!(a.kicks.is_empty());
    assert!(!a.silences.is_empty());
    assert!(!a.warnings.is_empty());
}

#[test]
fn cancellation_is_honoured() {
    let tr = synth::render(&synth::edm_arrangement(), 128.0, 44100, 0.5, 1);
    let audio = DecodedAudio::from_mono(tr.samples, 44100);
    let c = CancelToken::new();
    c.cancel();
    let r = analyze(&audio, &AnalysisConfig::default(), &c, &noop_progress());
    assert!(r.unwrap_err().is_cancelled());
}
