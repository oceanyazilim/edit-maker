//! Audio Intelligence engine.
//!
//! All musical event positions are expressed in **samples at the source sample
//! rate** (the accuracy reference). Conversion to the project timebase happens
//! via [`studio_core::Tick::from_samples`] only where needed.
//!
//! Pipeline: decode → mono analysis signal → STFT features → onset envelope →
//! tempo → beats (dynamic programming) → downbeats → kicks/snares → bar-level
//! structure → drop candidates.
//!
//! Confidence values are heuristic scores in 0..1. They are **not** calibrated
//! probabilities.

pub mod beats;
pub mod decode;
pub mod dsp;
pub mod eval;
pub mod features;
pub mod model;
pub mod percussion;
pub mod structure;
pub mod synth;
pub mod waveform;

pub use decode::{decode_file, DecodedAudio};
pub use model::*;

use studio_core::cancel::{report, ProgressSink};
use studio_core::{CancelToken, StudioResult};

/// Analyze an already decoded signal.
pub fn analyze(
    audio: &DecodedAudio,
    config: &AnalysisConfig,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> StudioResult<AudioAnalysis> {
    let sr = audio.sample_rate;
    let mut warnings = Vec::new();
    if audio.samples.is_empty() {
        return Err(studio_core::StudioError::invalid_media(
            "The audio stream contains no samples.",
        ));
    }
    report(progress, 0.02, "Computing spectral features");
    let feats = features::compute(&audio.samples, sr, cancel)?;
    cancel.check()?;

    report(progress, 0.35, "Estimating tempo");
    let tempo = beats::estimate_tempo(&feats, config);
    cancel.check()?;

    report(progress, 0.45, "Tracking beats");
    let mut beat_list = beats::track_beats(&feats, tempo.bpm, config);
    let (ph_on, ph_off, shifted) = beats::correct_half_beat_phase(&mut beat_list, &feats);
    if std::env::var_os("STUDIO_DEBUG_PHASE").is_some() {
        eprintln!("phase: R {ph_on:.3} mean phase {ph_off:.3} shifted={shifted}");
    }
    if shifted {
        tracing::info!(
            "beat phase corrected by half a beat (R {ph_on:.3}, mean phase {ph_off:.3})"
        );
    }
    cancel.check()?;

    report(progress, 0.55, "Detecting kicks");
    let lowpassed = dsp::lowpass_twice(&audio.samples, sr as f32, 200.0);
    let beat_samples: Vec<i64> = beat_list.iter().map(|b| b.sample).collect();
    let kicks = percussion::detect_kicks(&feats, &lowpassed, &beat_samples, config);
    // A manual BPM override is authoritative; otherwise use the kick pulse to fix 2x tempo.
    if config.bpm_override.is_none() && beats::halve_if_double_time(&mut beat_list, &kicks, sr) {
        tracing::info!("double-time beat grid halved using the kick pulse");
        warnings
            .push("Tempo was detected at double time and corrected using the kick pulse.".into());
    }
    cancel.check()?;

    report(progress, 0.65, "Estimating downbeats");
    let downbeat_conf =
        beats::assign_downbeats(&mut beat_list, &feats, &kicks, config.beats_per_bar.max(1));

    report(progress, 0.70, "Detecting snares");
    let snares = percussion::detect_snares(&feats, &beat_list, config);
    cancel.check()?;

    report(progress, 0.78, "Analyzing structure");
    let st = structure::analyze_structure(&feats, &beat_list, &kicks, audio.samples.len() as i64);
    cancel.check()?;

    let bpm_final = beats::bpm_from_beats(&beat_list, sr).unwrap_or(tempo.bpm);
    if tempo.confidence < 0.3 {
        warnings.push("Tempo confidence is low; consider setting the BPM manually.".into());
    }
    if beat_list.len() < 8 {
        warnings
            .push("Very few beats were detected; the track may be arrhythmic or too short.".into());
    }
    if st.drops.iter().all(|d| d.kind != DropKind::MainDrop) {
        warnings.push("Audio analysis completed, but no confident main drop was found.".into());
    }

    let silences = features::detect_silence(&feats, sr, -60.0, 0.5);
    let summary = features::summary(&feats, sr, audio.samples.len());

    report(progress, 1.0, "Audio analysis complete");
    Ok(AudioAnalysis {
        version: ANALYSIS_VERSION,
        sample_rate: sr,
        duration_samples: audio.samples.len() as i64,
        channels: audio.channels,
        tempo: TempoEstimate {
            bpm: bpm_final,
            ..tempo
        },
        downbeat_confidence: downbeat_conf,
        beats: beat_list,
        kicks,
        snares,
        sections: st.sections,
        drops: st.drops,
        silences,
        curves: feats.curves(),
        summary,
        warnings,
        config: config.clone(),
    })
}

/// Decode a file from disk and analyze it.
pub fn analyze_file(
    path: &std::path::Path,
    config: &AnalysisConfig,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> StudioResult<AudioAnalysis> {
    report(progress, 0.0, "Decoding audio");
    let audio = decode_file(path, cancel)?;
    let sub = studio_core::cancel::sub_progress(progress, 0.15, 1.0);
    analyze(&audio, config, cancel, &sub)
}
