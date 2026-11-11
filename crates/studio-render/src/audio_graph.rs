//! FFmpeg audio filtergraph from the timeline's audio tracks. Clip placement is
//! sample-accurate (`adelay` in samples at 48 kHz).

use std::collections::HashMap;

use studio_core::Tick;
use studio_media::{ffmpeg_file_url, ffmpeg_secs};
use studio_timeline::{Timeline, TrackKind};

use crate::sources::RenderAsset;

pub const MIX_RATE: u32 = 48_000;

#[derive(Debug, Clone, PartialEq)]
pub struct AudioGraph {
    pub input_args: Vec<String>,
    pub filter: String,
    pub out_label: String,
}

fn atempo_chain(speed: f64) -> String {
    let mut s = speed;
    let mut parts = Vec::new();
    while s < 0.5 {
        parts.push("atempo=0.5".to_string());
        s /= 0.5;
    }
    while s > 2.0 {
        parts.push("atempo=2.0".to_string());
        s /= 2.0;
    }
    if (s - 1.0).abs() > 1e-6 {
        parts.push(format!("atempo={s:.6}"));
    }
    parts.join(",")
}

/// Build the audio graph. `first_input` is the FFmpeg input index of the first audio input.
pub fn build(
    tl: &Timeline,
    assets: &HashMap<String, RenderAsset>,
    total: Tick,
    first_input: usize,
) -> Option<AudioGraph> {
    let mut input_args = Vec::new();
    let mut chains = Vec::new();
    let mut labels = Vec::new();
    let mut idx = first_input;
    for tr in tl
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio && !t.muted)
    {
        for c in &tr.clips {
            let Some(a) = assets.get(&c.asset_id) else {
                continue;
            };
            if !a.has_audio || c.start >= total {
                continue;
            }
            let dur = c.duration.min(total - c.start);
            let src_dur = dur.scale_f64(c.speed);
            input_args.extend([
                "-ss".to_string(),
                ffmpeg_secs(c.source_in),
                "-t".to_string(),
                ffmpeg_secs(src_dur + Tick::from_millis(50)),
                "-i".to_string(),
                ffmpeg_file_url(&a.path),
            ]);
            let mut f = format!(
                "[{idx}:a]aresample={MIX_RATE},aformat=sample_fmts=fltp:channel_layouts=stereo,atrim=end={},asetpts=PTS-STARTPTS",
                ffmpeg_secs(src_dur)
            );
            let tempo = atempo_chain(c.speed);
            if !tempo.is_empty() {
                f.push(',');
                f.push_str(&tempo);
            }
            if (c.volume - 1.0).abs() > 1e-4 {
                f.push_str(&format!(",volume={:.4}", c.volume.max(0.0)));
            }
            if c.fade_in > Tick::ZERO {
                f.push_str(&format!(
                    ",afade=t=in:st=0:d={}",
                    ffmpeg_secs(c.fade_in.min(dur))
                ));
            }
            if c.fade_out > Tick::ZERO {
                let fo = c.fade_out.min(dur);
                f.push_str(&format!(
                    ",afade=t=out:st={}:d={}",
                    ffmpeg_secs(dur - fo),
                    ffmpeg_secs(fo)
                ));
            }
            let delay = c.start.to_samples(MIX_RATE);
            if delay > 0 {
                f.push_str(&format!(",adelay=delays={delay}S:all=1"));
            }
            let label = format!("a{}", labels.len());
            f.push_str(&format!("[{label}]"));
            chains.push(f);
            labels.push(label);
            idx += 1;
        }
    }
    if labels.is_empty() {
        return None;
    }
    let ins: String = labels.iter().map(|l| format!("[{l}]")).collect();
    let mix = if labels.len() == 1 {
        format!("{ins}anull")
    } else {
        format!(
            "{ins}amix=inputs={}:normalize=0:dropout_transition=0",
            labels.len()
        )
    };
    // Exact output length: pad silence, then trim.
    let filter = format!(
        "{};{mix},apad,atrim=end={}[aout]",
        chains.join(";"),
        ffmpeg_secs(total)
    );
    Some(AudioGraph {
        input_args,
        filter,
        out_label: "aout".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_timeline::{Clip, TimelineSettings, Track};

    #[test]
    fn graph_for_music_clip() {
        let mut tl = Timeline::new(TimelineSettings::default());
        let mut a = Track::new("A1", TrackKind::Audio, "Music");
        let mut c = Clip::new(
            "m",
            "music",
            Tick::from_seconds_f64(1.0),
            Tick::from_seconds_f64(10.0),
            Tick::from_seconds_f64(30.0),
        );
        c.fade_out = Tick::from_seconds_f64(0.5);
        a.clips.push(c);
        tl.tracks.push(a);
        let mut assets = HashMap::new();
        assets.insert(
            "music".to_string(),
            RenderAsset {
                path: "C:/m.wav".into(),
                kind: crate::sources::RenderAssetKind::Audio,
                width: 0,
                height: 0,
                duration: Tick::from_seconds_f64(200.0),
                has_audio: true,
            },
        );
        let g = build(&tl, &assets, Tick::from_seconds_f64(11.0), 1).unwrap();
        assert!(g.input_args.contains(&"30.000000".to_string()));
        assert!(g.filter.contains("[1:a]"));
        assert!(g.filter.contains("adelay=delays=48000S"));
        assert!(g.filter.contains("afade=t=out:st=9.500000:d=0.500000"));
        assert!(g.filter.ends_with("atrim=end=11.000000[aout]"));
    }

    #[test]
    fn tempo_chain() {
        assert_eq!(atempo_chain(1.0), "");
        assert_eq!(atempo_chain(0.25), "atempo=0.5,atempo=0.500000");
        assert!(atempo_chain(3.0).starts_with("atempo=2.0,atempo=1.5"));
    }
}
