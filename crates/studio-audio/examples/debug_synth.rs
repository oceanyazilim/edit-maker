//! Developer tool: analyze a synthetic scenario and print details vs. ground truth.
//! Usage: cargo run -p studio-audio --example debug_synth -- <edm|wobble|hardtekk>
use studio_audio::eval::f_measure;
use studio_audio::synth::{self, BassMode, KickPattern, SynthSection};
use studio_audio::{analyze, AnalysisConfig, DecodedAudio, SectionKind};
use studio_core::cancel::noop_progress;
use studio_core::CancelToken;

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "edm".into());
    let tr = match which.as_str() {
        "wobble" => {
            let mut a2 = SynthSection::new(SectionKind::Groove, 8);
            a2.bass = BassMode::Wobble;
            a2.hats = true;
            let mut a1 = a2.clone();
            a1.bass = BassMode::Sustained;
            synth::render(&[a1, a2], 140.0, 44100, 0.3, 2)
        }
        "hardtekk" => {
            let mut s = SynthSection::new(SectionKind::Drop, 16);
            s.kick = KickPattern::FourOnFloor;
            s.kick_distortion = 0.8;
            s.hats = true;
            s.bass = BassMode::Sustained;
            s.sidechain = std::env::var_os("NO_SC").is_none();
            synth::render(&[s], 170.0, 48000, 0.2, 3)
        }
        "trap" => {
            let mut s = SynthSection::new(SectionKind::Groove, 16);
            s.kick = KickPattern::Syncopated;
            s.snare_backbeat = true;
            s.hats = true;
            synth::render(&[s], 140.0, 44100, 0.2, 4)
        }
        _ => synth::render(&synth::edm_arrangement(), 128.0, 44100, 0.5, 1),
    };
    let audio = DecodedAudio::from_mono(tr.samples.clone(), tr.sample_rate);
    let a = analyze(
        &audio,
        &AnalysisConfig::default(),
        &CancelToken::new(),
        &noop_progress(),
    )
    .unwrap();
    println!(
        "bpm {:.2} conf {:.2} alts {:?}",
        a.tempo.bpm, a.tempo.confidence, a.tempo.alternatives
    );
    let est: Vec<f64> = a.beats.iter().map(|b| a.seconds(b.sample)).collect();
    println!(
        "beats est {} ref {} first est {:?} first ref {:?}",
        est.len(),
        tr.beats.len(),
        est.first(),
        tr.beats.first()
    );
    println!("last est {:?} last ref {:?}", est.last(), tr.beats.last());
    let f = f_measure(&tr.beats, &est, 0.07);
    println!("beat {:?}", f);
    // mean signed offset of matched beats
    let mut offs = vec![];
    for r in &tr.beats {
        if let Some(e) = est
            .iter()
            .min_by(|x, y| (*x - r).abs().partial_cmp(&(*y - r).abs()).unwrap())
        {
            if (e - r).abs() < 0.07 {
                offs.push(e - r);
            }
        }
    }
    println!(
        "beat mean offset {:.1} ms",
        1000.0 * offs.iter().sum::<f64>() / offs.len().max(1) as f64
    );
    let ek: Vec<f64> = a.kicks.iter().map(|k| a.seconds(k.sample)).collect();
    println!("kick {:?}", f_measure(&tr.kicks, &ek, 0.03));
    for k in a.kicks.iter().take(12) {
        let t = a.seconds(k.sample);
        let near = tr
            .kicks
            .iter()
            .map(|r| (r - t).abs())
            .fold(f64::MAX, f64::min);
        println!(
            "  kick {:.3}s conf {:.2} int {:.2} nearest_ref {:.1}ms",
            t,
            k.confidence,
            k.intensity,
            near * 1000.0
        );
    }
    println!(
        "drops {:?}",
        a.drops
            .iter()
            .map(|d| (
                format!("{:.2}", a.seconds(d.sample)),
                d.kind,
                format!("{:.2}", d.score)
            ))
            .collect::<Vec<_>>()
    );
    println!("truth drops {:?}", tr.drops);
    println!(
        "sections {:?}",
        a.sections
            .iter()
            .map(|s| (
                format!("{:.2}", a.seconds(s.start_sample)),
                s.kind,
                format!("{:.2}", s.energy)
            ))
            .collect::<Vec<_>>()
    );
    println!("truth sections {:?}", tr.sections);
}
