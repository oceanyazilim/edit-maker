//! Encoder argument construction.

/// `quality` is a CRF-like value (lower = better, ~18 visually lossless for H.264).
pub fn video_args(
    encoder: &str,
    quality: u8,
    speed_preset: &str,
    bitrate_kbps: Option<u32>,
) -> Vec<String> {
    let q = quality.min(51).to_string();
    let mut a: Vec<String> = vec!["-c:v".into(), encoder.into()];
    let sw_preset = match speed_preset {
        "fast" => "veryfast",
        "draft" => "ultrafast",
        _ => "medium",
    };
    match encoder {
        "libx264" | "libx265" => {
            a.extend(["-preset".into(), sw_preset.into()]);
            if let Some(b) = bitrate_kbps {
                a.extend([
                    "-b:v".into(),
                    format!("{b}k"),
                    "-maxrate".into(),
                    format!("{}k", b * 3 / 2),
                    "-bufsize".into(),
                    format!("{}k", b * 2),
                ]);
            } else {
                a.extend(["-crf".into(), q]);
            }
        }
        e if e.ends_with("_nvenc") => {
            let p = match speed_preset {
                "draft" => "p1",
                "fast" => "p3",
                _ => "p5",
            };
            a.extend(["-preset".into(), p.into()]);
            if let Some(b) = bitrate_kbps {
                a.extend(["-rc".into(), "vbr".into(), "-b:v".into(), format!("{b}k")]);
            } else {
                a.extend([
                    "-rc".into(),
                    "vbr".into(),
                    "-cq".into(),
                    q,
                    "-b:v".into(),
                    "0".into(),
                ]);
            }
        }
        e if e.ends_with("_amf") => {
            a.extend([
                "-quality".into(),
                if speed_preset == "draft" {
                    "speed".into()
                } else {
                    "balanced".into()
                },
            ]);
            if let Some(b) = bitrate_kbps {
                a.extend([
                    "-rc".into(),
                    "vbr_peak".into(),
                    "-b:v".into(),
                    format!("{b}k"),
                ]);
            } else {
                a.extend([
                    "-rc".into(),
                    "cqp".into(),
                    "-qp_i".into(),
                    q.clone(),
                    "-qp_p".into(),
                    q,
                ]);
            }
        }
        e if e.ends_with("_qsv") => {
            if let Some(b) = bitrate_kbps {
                a.extend(["-b:v".into(), format!("{b}k")]);
            } else {
                a.extend(["-global_quality".into(), q]);
            }
        }
        _ => {
            a.extend(["-b:v".into(), format!("{}k", bitrate_kbps.unwrap_or(8000))]);
        }
    }
    if encoder.starts_with("hevc") || encoder == "libx265" {
        a.extend(["-tag:v".into(), "hvc1".into()]);
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn args() {
        let a = video_args("libx264", 18, "quality", None);
        assert!(a.windows(2).any(|w| w[0] == "-crf" && w[1] == "18"));
        let n = video_args("h264_nvenc", 20, "fast", None);
        assert!(n.windows(2).any(|w| w[0] == "-cq" && w[1] == "20"));
        let h = video_args("libx265", 22, "quality", Some(5000));
        assert!(h.contains(&"hvc1".to_string()) && h.contains(&"5000k".to_string()));
    }
}
