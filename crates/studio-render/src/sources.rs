//! Per-clip source decoding for the compositor.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use studio_core::{Rational, StudioError, StudioResult, Tick};
use studio_media::{FfmpegTools, FrameReader, FrameRequest, PixelFormat};
use studio_timeline::{Clip, FitMode};

use crate::compositor::SourceFrame;
use crate::state::max_zoom;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderAssetKind {
    Video,
    Audio,
    Image,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RenderAsset {
    pub path: PathBuf,
    pub kind: RenderAssetKind,
    /// Display size (after rotation). 0 for audio.
    pub width: u32,
    pub height: u32,
    pub duration: Tick,
    pub has_audio: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecodeTarget {
    pub width: u32,
    pub height: u32,
    pub headroom: f32,
    pub fit: FitMode,
}

fn even(v: f32) -> u32 {
    (((v / 2.0).round() as u32) * 2).max(2)
}

/// Decode size: the fit/cover size for the canvas times zoom headroom (≤ 1.5×).
pub fn decode_target(asset: &RenderAsset, clip: &Clip, cw: u32, ch: u32) -> DecodeTarget {
    let (aw, ah) = (asset.width.max(1) as f32, asset.height.max(1) as f32);
    let fit = clip.framing.mode;
    let base = match fit {
        FitMode::Cover => (cw as f32 / aw).max(ch as f32 / ah),
        FitMode::Fit => (cw as f32 / aw).min(ch as f32 / ah),
    };
    let headroom = max_zoom(clip).clamp(1.0, 1.5);
    DecodeTarget {
        width: even(aw * base * headroom),
        height: even(ah * base * headroom),
        headroom,
        fit,
    }
}

struct ClipDecoder {
    reader: Option<FrameReader>,
    next_t: Option<Tick>,
    frame: Vec<u8>,
    have_frame: bool,
    target: DecodeTarget,
    still: bool,
    eof: bool,
}

pub struct SourceManager<'a> {
    tools: &'a FfmpegTools,
    assets: &'a HashMap<String, RenderAsset>,
    fps: Rational,
    cw: u32,
    ch: u32,
    decoders: HashMap<String, ClipDecoder>,
}

impl<'a> SourceManager<'a> {
    pub fn new(
        tools: &'a FfmpegTools,
        assets: &'a HashMap<String, RenderAsset>,
        fps: Rational,
        cw: u32,
        ch: u32,
    ) -> Self {
        Self {
            tools,
            assets,
            fps,
            cw,
            ch,
            decoders: HashMap::new(),
        }
    }

    fn open(
        &self,
        clip: &Clip,
        asset: &RenderAsset,
        target: DecodeTarget,
        t: Tick,
    ) -> StudioResult<FrameReader> {
        let still = asset.kind == RenderAssetKind::Image;
        let speed = clip.speed.clamp(0.05, 16.0);
        let fps = if still || (speed - 1.0).abs() < 1e-9 {
            self.fps
        } else {
            Rational::new(
                (self.fps.num as f64 * 1000.0 / speed).round() as i64,
                self.fps.den * 1000,
            )
            .unwrap_or(self.fps)
        };
        let req = FrameRequest {
            path: asset.path.clone(),
            start: if still {
                Tick::ZERO
            } else {
                clip.source_time(t).max(Tick::ZERO)
            },
            duration: None,
            fps,
            width: target.width,
            height: target.height,
            pixel_format: PixelFormat::Rgba,
            still_image: still,
        };
        FrameReader::open(self.tools, &req)
    }

    /// Source frame of `clip` for timeline time `t`. Sequential calls (t advancing by
    /// one frame) stream from one decoder; any jump re-opens the decoder (seek).
    pub fn frame(&mut self, clip: &Clip, t: Tick) -> StudioResult<SourceFrame<'_>> {
        let asset = self
            .assets
            .get(&clip.asset_id)
            .ok_or_else(|| {
                StudioError::not_found("Source media could not be found.")
                    .with_details(clip.asset_id.clone())
            })?
            .clone();
        if !self.decoders.contains_key(&clip.id) {
            let target = decode_target(&asset, clip, self.cw, self.ch);
            let n = (target.width * target.height * 4) as usize;
            self.decoders.insert(
                clip.id.clone(),
                ClipDecoder {
                    reader: None,
                    next_t: None,
                    frame: vec![0u8; n],
                    have_frame: false,
                    target,
                    still: asset.kind == RenderAssetKind::Image,
                    eof: false,
                },
            );
        }
        let frame_dur = self.fps.frame_duration();
        let needs_open = {
            let d = &self.decoders[&clip.id];
            if d.still {
                !d.have_frame
            } else {
                d.reader.is_none() && !d.eof || d.next_t != Some(t)
            }
        };
        if needs_open {
            let target = self.decoders[&clip.id].target;
            let reader = self.open(clip, &asset, target, t)?;
            let d = self.decoders.get_mut(&clip.id).unwrap();
            d.reader = Some(reader);
            d.eof = false;
        }
        let d = self.decoders.get_mut(&clip.id).unwrap();
        let read_now = !d.still || !d.have_frame;
        if read_now && !d.eof {
            if let Some(r) = d.reader.as_mut() {
                if r.read_frame(&mut d.frame)? {
                    d.have_frame = true;
                } else {
                    // End of source (rounding at the very end): hold the last frame.
                    d.eof = true;
                    d.reader = None;
                }
            }
            if d.still {
                d.reader = None;
            }
        }
        d.next_t = Some(t + frame_dur);
        if !d.have_frame {
            // Nothing decodable: transparent frame (renders background).
            d.frame.iter_mut().for_each(|b| *b = 0);
        }
        Ok(SourceFrame {
            data: &d.frame,
            width: d.target.width,
            height: d.target.height,
            headroom: d.target.headroom,
            fit: d.target.fit,
        })
    }

    /// Free decoders of clips that ended before `t - keep`.
    pub fn release_before(&mut self, clips_end: &HashMap<String, Tick>, t: Tick, keep: Tick) {
        self.decoders
            .retain(|id, _| clips_end.get(id).map(|e| *e + keep >= t).unwrap_or(false));
    }
}
