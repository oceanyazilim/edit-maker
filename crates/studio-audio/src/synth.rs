//! Deterministic synthetic music generator with exact ground-truth annotations.
//! Used for regression tests, evaluation and test fixtures (no copyrighted audio).

use crate::model::SectionKind;
use studio_core::StudioRng;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KickPattern {
    None,
    FourOnFloor,
    /// Trap/phonk-style syncopated pattern (16th grid positions 0, 6, 10 of 16).
    Syncopated,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BassMode {
    None,
    /// Continuous distorted 55 Hz bass (must NOT be detected as kicks).
    Sustained,
    /// Amplitude-modulated bass at 8th-note rate (smooth modulation).
    Wobble,
}

#[derive(Clone, Debug)]
pub struct SynthSection {
    pub kind: SectionKind,
    pub bars: u32,
    pub kick: KickPattern,
    pub kick_distortion: f32,
    pub snare_backbeat: bool,
    pub snare_every_beat: bool,
    pub hats: bool,
    pub bass: BassMode,
    pub riser: bool,
    pub pad: bool,
    /// Duck the bass after each kick (sidechain compression, standard in EDM/hardtekk).
    pub sidechain: bool,
}

impl SynthSection {
    pub fn new(kind: SectionKind, bars: u32) -> Self {
        Self {
            kind,
            bars,
            kick: KickPattern::None,
            kick_distortion: 0.0,
            snare_backbeat: false,
            snare_every_beat: false,
            hats: false,
            bass: BassMode::None,
            riser: false,
            pad: false,
            sidechain: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SynthTrack {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub bpm: f64,
    /// Beat times (seconds) in sections that contain percussion.
    pub beats: Vec<f64>,
    pub downbeats: Vec<f64>,
    pub kicks: Vec<f64>,
    pub snares: Vec<f64>,
    /// Start time of every Drop section that follows a non-drop section.
    pub drops: Vec<f64>,
    pub sections: Vec<(f64, f64, SectionKind)>,
}

fn add(buf: &mut [f32], start: usize, src: impl Iterator<Item = f32>) {
    for (i, v) in src.enumerate() {
        if let Some(s) = buf.get_mut(start + i) {
            *s += v;
        } else {
            break;
        }
    }
}

pub fn render(
    sections: &[SynthSection],
    bpm: f64,
    sample_rate: u32,
    lead_in_secs: f64,
    seed: u64,
) -> SynthTrack {
    let sr = sample_rate as f64;
    let beat = 60.0 / bpm;
    let bar = beat * 4.0;
    let total_bars: u32 = sections.iter().map(|s| s.bars).sum();
    let total = lead_in_secs + total_bars as f64 * bar + 1.0;
    let mut buf = vec![0f32; (total * sr) as usize];
    let mut rng = StudioRng::new(seed);
    let mut noise = move || (rng.next_f64() * 2.0 - 1.0) as f32;

    let mut tr = SynthTrack {
        samples: vec![],
        sample_rate,
        bpm,
        beats: vec![],
        downbeats: vec![],
        kicks: vec![],
        snares: vec![],
        drops: vec![],
        sections: vec![],
    };
    let mut t_sec = lead_in_secs;
    let mut prev_kind: Option<SectionKind> = None;
    for sec in sections {
        let start = t_sec;
        let end = start + sec.bars as f64 * bar;
        tr.sections.push((start, end, sec.kind));
        if sec.kind == SectionKind::Drop && prev_kind != Some(SectionKind::Drop) {
            tr.drops.push(start);
        }
        prev_kind = Some(sec.kind);
        let percussive =
            sec.kick != KickPattern::None || sec.hats || sec.snare_backbeat || sec.snare_every_beat;
        let n_beats = sec.bars * 4;
        let mut sec_kicks: Vec<f64> = Vec::new();
        for b in 0..n_beats {
            let tb = start + b as f64 * beat;
            if percussive {
                tr.beats.push(tb);
                if b % 4 == 0 {
                    tr.downbeats.push(tb);
                }
            }
            // Kicks.
            let mut kick_times = vec![];
            match sec.kick {
                KickPattern::FourOnFloor => kick_times.push(tb),
                KickPattern::Syncopated => {
                    if b % 4 == 0 {
                        for pos in [0.0, 6.0, 10.0] {
                            kick_times.push(tb + pos * beat / 4.0);
                        }
                    }
                }
                KickPattern::None => {}
            }
            for kt in kick_times {
                tr.kicks.push(kt);
                sec_kicks.push(kt);
                let dist = sec.kick_distortion;
                let len = (0.35 * sr) as usize;
                let mut phase = 0f64;
                let v: Vec<f32> = (0..len)
                    .map(|i| {
                        let t = i as f64 / sr;
                        let f = 45.0 + 105.0 * (-t / 0.03).exp();
                        phase += 2.0 * std::f64::consts::PI * f / sr;
                        let amp = (-t / 0.12).exp();
                        let mut s = (phase.sin() * amp) as f32;
                        if dist > 0.0 {
                            s = (s * (1.0 + 6.0 * dist)).tanh() / (1.0 + dist).tanh().max(0.5);
                        }
                        0.8 * s
                    })
                    .collect();
                add(&mut buf, (kt * sr) as usize, v.into_iter());
                let click: Vec<f32> = (0..(0.004 * sr) as usize)
                    .map(|i| 0.12 * noise() * (-(i as f32) / (0.0015 * sr as f32)).exp())
                    .collect();
                add(&mut buf, (kt * sr) as usize, click.into_iter());
            }
            // Snares.
            let snare_here =
                sec.snare_every_beat || (sec.snare_backbeat && (b % 4 == 1 || b % 4 == 3));
            if snare_here {
                tr.snares.push(tb);
                let len = (0.2 * sr) as usize;
                let mut prev = 0f32;
                let v: Vec<f32> = (0..len)
                    .map(|i| {
                        let t = i as f32 / sr as f32;
                        let n = noise();
                        let hp = n - prev;
                        prev = n;
                        0.35 * hp * (-t / 0.07).exp()
                            + 0.15
                                * (2.0 * std::f32::consts::PI * 190.0 * t).sin()
                                * (-t / 0.04).exp()
                    })
                    .collect();
                add(&mut buf, (tb * sr) as usize, v.into_iter());
            }
            // Hats on 8ths.
            if sec.hats {
                for half in 0..2 {
                    let th = tb + half as f64 * beat / 2.0;
                    let mut prev = 0f32;
                    let v: Vec<f32> = (0..(0.05 * sr) as usize)
                        .map(|i| {
                            let n = noise();
                            let hp = n - prev;
                            prev = n;
                            0.08 * hp * (-(i as f32) / (0.012 * sr as f32)).exp()
                        })
                        .collect();
                    add(&mut buf, (th * sr) as usize, v.into_iter());
                }
            }
        }
        // Continuous layers.
        let s0 = (start * sr) as usize;
        let s1 = ((end * sr) as usize).min(buf.len());
        let fade = (0.05 * sr) as usize;
        let mut ki = 0usize;
        for i in s0..s1 {
            let t = i as f64 / sr;
            while ki + 1 < sec_kicks.len() && sec_kicks[ki + 1] <= t {
                ki += 1;
            }
            let duck = if sec.sidechain && !sec_kicks.is_empty() && sec_kicks[ki] <= t {
                (1.0 - 0.85 * (-(t - sec_kicks[ki]) / 0.09).exp()) as f32
            } else {
                1.0
            };
            let lt = (i - s0) as f64 / sr;
            let edge = (((i - s0).min(s1 - i)) as f32 / fade as f32).min(1.0);
            let mut v = 0f32;
            match sec.bass {
                BassMode::Sustained => {
                    v += duck
                        * 0.30
                        * (2.5 * (2.0 * std::f64::consts::PI * 55.0 * t).sin() as f32).tanh()
                }
                BassMode::Wobble => {
                    let lfo =
                        0.5 + 0.5 * (2.0 * std::f64::consts::PI * (bpm / 60.0 * 2.0) * t).sin();
                    v += duck * (0.35 * (2.0 * std::f64::consts::PI * 55.0 * t).sin() * lfo) as f32;
                }
                BassMode::None => {}
            }
            if sec.pad {
                for f in [220.0, 277.18, 329.63] {
                    v += 0.04 * (2.0 * std::f64::consts::PI * f * t).sin() as f32;
                }
            }
            if sec.riser {
                let prog = (lt / (end - start)) as f32;
                v += 0.12 * prog * prog * noise();
            }
            buf[i] += v * edge;
        }
        t_sec = end;
    }
    let peak = buf.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-6);
    let g = 0.89 / peak;
    for s in buf.iter_mut() {
        *s *= g;
    }
    tr.samples = buf;
    tr
}

/// A representative EDM-like arrangement: intro → build → drop → breakdown → drop → outro.
pub fn edm_arrangement() -> Vec<SynthSection> {
    use SectionKind::*;
    let mut intro = SynthSection::new(Intro, 8);
    intro.hats = true;
    intro.pad = true;
    let mut build = SynthSection::new(BuildUp, 8);
    build.hats = true;
    build.snare_every_beat = true;
    build.riser = true;
    let mut drop1 = SynthSection::new(Drop, 16);
    drop1.kick = KickPattern::FourOnFloor;
    drop1.snare_backbeat = true;
    drop1.hats = true;
    drop1.bass = BassMode::Sustained;
    let mut brk = SynthSection::new(Breakdown, 8);
    brk.hats = true;
    brk.pad = true;
    let mut drop2 = drop1.clone();
    drop2.bars = 8;
    let mut outro = SynthSection::new(Outro, 4);
    outro.hats = true;
    outro.pad = true;
    vec![intro, build, drop1, brk, drop2, outro]
}
