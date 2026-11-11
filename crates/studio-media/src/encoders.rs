//! Runtime encoder detection. An encoder listed by `ffmpeg -encoders` is only
//! reported available after a real one-second test encode succeeds, because a
//! build may list NVENC/AMF/QSV without a compatible GPU or driver.

use serde::{Deserialize, Serialize};
use studio_core::StudioResult;

use crate::process::command;
use crate::tools::FfmpegTools;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    H264,
    Hevc,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EncoderInfo {
    pub name: String,
    pub codec: VideoCodec,
    pub hardware: bool,
    pub vendor: String,
    pub listed: bool,
    pub available: bool,
}

const CANDIDATES: &[(&str, VideoCodec, bool, &str)] = &[
    ("libx264", VideoCodec::H264, false, "software"),
    ("h264_nvenc", VideoCodec::H264, true, "nvidia"),
    ("h264_amf", VideoCodec::H264, true, "amd"),
    ("h264_qsv", VideoCodec::H264, true, "intel"),
    (
        "h264_mf",
        VideoCodec::H264,
        true,
        "windows-media-foundation",
    ),
    ("libx265", VideoCodec::Hevc, false, "software"),
    ("hevc_nvenc", VideoCodec::Hevc, true, "nvidia"),
    ("hevc_amf", VideoCodec::Hevc, true, "amd"),
    ("hevc_qsv", VideoCodec::Hevc, true, "intel"),
];

pub fn detect_encoders(tools: &FfmpegTools) -> StudioResult<Vec<EncoderInfo>> {
    let out = command(&tools.ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()?;
    let listing = String::from_utf8_lossy(&out.stdout).to_string();
    let listed_names: Vec<String> = listing
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let flags = it.next()?;
            let name = it.next()?;
            (flags.len() == 6 && flags.starts_with('V')).then(|| name.to_string())
        })
        .collect();
    let mut res = Vec::new();
    for &(name, codec, hw, vendor) in CANDIDATES {
        let listed = listed_names.iter().any(|n| n == name);
        let available = listed && test_encode(tools, name);
        res.push(EncoderInfo {
            name: name.into(),
            codec,
            hardware: hw,
            vendor: vendor.into(),
            listed,
            available,
        });
    }
    Ok(res)
}

fn test_encode(tools: &FfmpegTools, encoder: &str) -> bool {
    let out = command(&tools.ffmpeg)
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:size=320x240:rate=30:duration=1",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            encoder,
            "-f",
            "null",
            "-",
        ])
        .output();
    matches!(out, Ok(o) if o.status.success())
}

/// Pick the best available encoder for a codec, honouring a preference.
pub fn choose_encoder<'a>(
    list: &'a [EncoderInfo],
    codec: VideoCodec,
    prefer_hardware: bool,
) -> Option<&'a EncoderInfo> {
    let mut c: Vec<&EncoderInfo> = list
        .iter()
        .filter(|e| e.codec == codec && e.available)
        .collect();
    c.sort_by_key(|e| if e.hardware == prefer_hardware { 0 } else { 1 });
    c.into_iter().next()
}
