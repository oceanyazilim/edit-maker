//! FFprobe metadata extraction. Parsing is separated from execution so it can be
//! unit-tested with recorded ffprobe JSON.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use studio_core::{ErrorCode, Rational, StudioError, StudioResult, Tick};

use crate::process::command;
use crate::tools::FfmpegTools;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Video,
    Audio,
    Image,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VideoStreamInfo {
    pub index: u32,
    pub codec: String,
    pub profile: Option<String>,
    /// Coded size.
    pub width: u32,
    pub height: u32,
    /// Size after applying rotation metadata (what the viewer sees).
    pub display_width: u32,
    pub display_height: u32,
    pub rotation: i32,
    pub fps: Rational,
    pub r_frame_rate: Option<Rational>,
    /// Heuristic: avg_frame_rate differs from r_frame_rate.
    pub variable_frame_rate: bool,
    pub pix_fmt: Option<String>,
    pub sample_aspect_ratio: Rational,
    pub color_space: Option<String>,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    /// PQ / HLG transfer.
    pub hdr: bool,
    pub bit_rate: Option<u64>,
    pub frame_count: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AudioStreamInfo {
    pub index: u32,
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_layout: Option<String>,
    pub bit_rate: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MediaMetadata {
    pub kind: MediaKind,
    pub container: String,
    pub duration: Tick,
    pub size_bytes: u64,
    pub bit_rate: Option<u64>,
    pub video: Option<VideoStreamInfo>,
    pub audio: Vec<AudioStreamInfo>,
    pub warnings: Vec<String>,
}

impl MediaMetadata {
    pub fn has_audio(&self) -> bool {
        !self.audio.is_empty()
    }
    pub fn has_video(&self) -> bool {
        self.video.is_some()
    }
}

pub fn probe(tools: &FfmpegTools, path: &Path) -> StudioResult<MediaMetadata> {
    let out = command(&tools.ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(crate::ffmpeg_file_url(path))
        .output()
        .map_err(|e| {
            StudioError::new(ErrorCode::ToolMissing, "FFprobe could not be started.")
                .with_details(e.to_string())
        })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(crate::process::classify_ffmpeg_error(&err).with_details(err));
    }
    let json: Value = serde_json::from_slice(&out.stdout)?;
    parse_probe_json(&json)
}

fn num<T: std::str::FromStr>(v: &Value) -> Option<T> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.to_string().parse().ok(),
        _ => None,
    }
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(|x| x.as_str())
        .map(String::from)
        .filter(|x| !x.is_empty() && x != "unknown")
}

fn rotation_of(stream: &Value) -> i32 {
    if let Some(r) = stream
        .get("tags")
        .and_then(|t| t.get("rotate"))
        .and_then(num::<i32>)
    {
        return r;
    }
    if let Some(list) = stream.get("side_data_list").and_then(|l| l.as_array()) {
        for sd in list {
            if let Some(r) = sd.get("rotation").and_then(num::<f64>) {
                // Display matrix rotation is counter-clockwise; normalize to clockwise degrees.
                return (-(r.round() as i32)).rem_euclid(360);
            }
        }
    }
    0
}

const IMAGE_CODECS: &[&str] = &["png", "mjpeg", "jpeg", "webp", "bmp", "tiff", "gif"];

pub fn parse_probe_json(json: &Value) -> StudioResult<MediaMetadata> {
    let format = json.get("format").ok_or_else(|| {
        StudioError::invalid_media("The media file is corrupted or not supported.")
    })?;
    let streams = json
        .get("streams")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let mut warnings = Vec::new();
    let container = s(format, "format_name").unwrap_or_else(|| "unknown".into());
    let fmt_duration = format.get("duration").and_then(num::<f64>);
    let size_bytes = format.get("size").and_then(num::<u64>).unwrap_or(0);
    let bit_rate = format.get("bit_rate").and_then(num::<u64>);

    let mut video: Option<VideoStreamInfo> = None;
    let mut audio = Vec::new();
    let mut is_image = false;
    for st in &streams {
        let ctype = st.get("codec_type").and_then(|x| x.as_str()).unwrap_or("");
        let index = st.get("index").and_then(num::<u32>).unwrap_or(0);
        match ctype {
            "video" if video.is_none() => {
                let disposition_pic = st
                    .get("disposition")
                    .and_then(|d| d.get("attached_pic"))
                    .and_then(num::<i32>)
                    .unwrap_or(0)
                    == 1;
                if disposition_pic {
                    // Cover art inside an audio file is not a video track.
                    continue;
                }
                let codec = s(st, "codec_name").unwrap_or_else(|| "unknown".into());
                let width = st.get("width").and_then(num::<u32>).unwrap_or(0);
                let height = st.get("height").and_then(num::<u32>).unwrap_or(0);
                if width == 0 || height == 0 {
                    warnings.push("Video stream has no valid dimensions.".into());
                    continue;
                }
                let avg = s(st, "avg_frame_rate").and_then(|x| Rational::parse(&x));
                let rfr = s(st, "r_frame_rate").and_then(|x| Rational::parse(&x));
                let fps = avg.or(rfr).unwrap_or(Rational::FPS_30);
                let vfr = match (avg, rfr) {
                    (Some(a), Some(r)) => {
                        (a.as_f64() - r.as_f64()).abs() > 0.01 * r.as_f64().max(1.0)
                    }
                    _ => false,
                };
                let rotation = rotation_of(st);
                let (dw, dh) = if rotation % 180 == 90 {
                    (height, width)
                } else {
                    (width, height)
                };
                let sar = s(st, "sample_aspect_ratio")
                    .and_then(|x| Rational::parse(&x.replace(':', "/")))
                    .unwrap_or(Rational { num: 1, den: 1 });
                let transfer = s(st, "color_transfer");
                let hdr = matches!(
                    transfer.as_deref(),
                    Some("smpte2084") | Some("arib-std-b67")
                );
                if IMAGE_CODECS.contains(&codec.as_str())
                    && fmt_duration.map(|d| d < 0.05).unwrap_or(true)
                {
                    is_image = true;
                }
                if vfr {
                    warnings.push("Variable frame rate detected; frames are conformed to the project frame rate.".into());
                }
                if hdr {
                    warnings.push("HDR source detected. HDR-to-SDR tone mapping is not implemented yet; colours may look washed out.".into());
                }
                video = Some(VideoStreamInfo {
                    index,
                    codec,
                    profile: s(st, "profile"),
                    width,
                    height,
                    display_width: dw,
                    display_height: dh,
                    rotation,
                    fps,
                    r_frame_rate: rfr,
                    variable_frame_rate: vfr,
                    pix_fmt: s(st, "pix_fmt"),
                    sample_aspect_ratio: if sar.num == 0 {
                        Rational { num: 1, den: 1 }
                    } else {
                        sar
                    },
                    color_space: s(st, "color_space"),
                    color_transfer: transfer,
                    color_primaries: s(st, "color_primaries"),
                    hdr,
                    bit_rate: st.get("bit_rate").and_then(num::<u64>),
                    frame_count: st.get("nb_frames").and_then(num::<u64>),
                });
            }
            "audio" => {
                audio.push(AudioStreamInfo {
                    index,
                    codec: s(st, "codec_name").unwrap_or_else(|| "unknown".into()),
                    sample_rate: st.get("sample_rate").and_then(num::<u32>).unwrap_or(0),
                    channels: st.get("channels").and_then(num::<u32>).unwrap_or(0),
                    channel_layout: s(st, "channel_layout"),
                    bit_rate: st.get("bit_rate").and_then(num::<u64>),
                });
            }
            _ => {}
        }
    }
    let kind = if is_image {
        MediaKind::Image
    } else if video.is_some() {
        MediaKind::Video
    } else if !audio.is_empty() {
        MediaKind::Audio
    } else {
        return Err(StudioError::new(
            ErrorCode::UnsupportedFormat,
            "The file contains no usable audio or video streams.",
        ));
    };
    let duration = match (kind, fmt_duration) {
        (MediaKind::Image, _) => Tick::ZERO,
        (_, Some(d)) if d > 0.0 => Tick::from_seconds_f64(d),
        _ => {
            warnings.push("Duration is unknown; the file may be truncated.".into());
            Tick::ZERO
        }
    };
    if kind != MediaKind::Image && duration == Tick::ZERO {
        return Err(StudioError::invalid_media(
            "The media file has no measurable duration (corrupted or truncated).",
        ));
    }
    Ok(MediaMetadata {
        kind,
        container,
        duration,
        size_bytes,
        bit_rate,
        video,
        audio,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rotated_phone_video() {
        let j: Value = serde_json::from_str(r#"{
            "streams": [
              {"index":0,"codec_name":"h264","codec_type":"video","width":1920,"height":1080,
               "avg_frame_rate":"30000/1001","r_frame_rate":"30000/1001","pix_fmt":"yuv420p",
               "sample_aspect_ratio":"1:1","color_transfer":"bt709",
               "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}],"nb_frames":"300"},
              {"index":1,"codec_name":"aac","codec_type":"audio","sample_rate":"48000","channels":2,"channel_layout":"stereo"}
            ],
            "format": {"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"10.010000","size":"1234567","bit_rate":"986000"}
        }"#).unwrap();
        let m = parse_probe_json(&j).unwrap();
        assert_eq!(m.kind, MediaKind::Video);
        let v = m.video.unwrap();
        assert_eq!(v.rotation, 90);
        assert_eq!((v.display_width, v.display_height), (1080, 1920));
        assert_eq!(v.fps, Rational::new(30000, 1001).unwrap());
        assert!(!v.variable_frame_rate);
        assert_eq!(m.audio[0].sample_rate, 48000);
        assert_eq!(m.duration, Tick::from_seconds_f64(10.01));
    }

    #[test]
    fn detects_vfr_hdr_and_cover_art() {
        let j: Value = serde_json::from_str(
            r#"{
            "streams": [
              {"index":0,"codec_name":"hevc","codec_type":"video","width":3840,"height":2160,
               "avg_frame_rate":"2997/100","r_frame_rate":"60/1","color_transfer":"smpte2084"}
            ],
            "format": {"format_name":"mov","duration":"5.0"}
        }"#,
        )
        .unwrap();
        let v = parse_probe_json(&j).unwrap().video.unwrap();
        assert!(v.variable_frame_rate && v.hdr);

        let a: Value = serde_json::from_str(r#"{
            "streams": [
              {"index":0,"codec_name":"mp3","codec_type":"audio","sample_rate":"44100","channels":2},
              {"index":1,"codec_name":"mjpeg","codec_type":"video","width":500,"height":500,"disposition":{"attached_pic":1}}
            ],
            "format": {"format_name":"mp3","duration":"180.5"}
        }"#).unwrap();
        let m = parse_probe_json(&a).unwrap();
        assert_eq!(m.kind, MediaKind::Audio);
        assert!(m.video.is_none());
    }

    #[test]
    fn image_and_invalid() {
        let j: Value = serde_json::from_str(r#"{"streams":[{"index":0,"codec_name":"png","codec_type":"video","width":800,"height":600}],"format":{"format_name":"png_pipe"}}"#).unwrap();
        assert_eq!(parse_probe_json(&j).unwrap().kind, MediaKind::Image);
        let bad: Value =
            serde_json::from_str(r#"{"streams":[],"format":{"format_name":"x"}}"#).unwrap();
        assert!(parse_probe_json(&bad).is_err());
        let trunc: Value = serde_json::from_str(r#"{"streams":[{"index":0,"codec_name":"h264","codec_type":"video","width":640,"height":360}],"format":{"format_name":"mp4"}}"#).unwrap();
        assert_eq!(
            parse_probe_json(&trunc).unwrap_err().code,
            ErrorCode::InvalidMedia
        );
    }
}
