//! Timeline validation used before preview/render and after automatic generation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use studio_core::Tick;

use crate::model::{Timeline, TrackKind};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AssetInfo {
    pub duration: Tick,
    pub has_video: bool,
    pub has_audio: bool,
    /// Still images can be held for any duration.
    pub is_image: bool,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub clip_id: Option<String>,
    pub message: String,
}

pub fn validate(t: &Timeline, assets: &HashMap<String, AssetInfo>) -> Vec<ValidationIssue> {
    let mut out = Vec::new();
    let err = |out: &mut Vec<ValidationIssue>, clip: Option<&str>, m: String| {
        out.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            clip_id: clip.map(String::from),
            message: m,
        })
    };
    let warn = |out: &mut Vec<ValidationIssue>, clip: Option<&str>, m: String| {
        out.push(ValidationIssue {
            severity: ValidationSeverity::Warning,
            clip_id: clip.map(String::from),
            message: m,
        })
    };
    let s = &t.settings;
    if s.width < 16 || s.height < 16 || s.width % 2 != 0 || s.height % 2 != 0 {
        err(
            &mut out,
            None,
            format!(
                "Output resolution {}x{} must be even and at least 16 px.",
                s.width, s.height
            ),
        );
    }
    if s.fps.as_f64() < 1.0 || s.fps.as_f64() > 240.0 {
        err(
            &mut out,
            None,
            format!("Frame rate {} is out of range.", s.fps),
        );
    }
    let frame = s.fps.frame_duration();
    for track in &t.tracks {
        let mut clips: Vec<_> = track.clips.iter().collect();
        clips.sort_by_key(|c| c.start);
        for w in clips.windows(2) {
            if w[1].start < w[0].end() {
                err(
                    &mut out,
                    Some(&w[1].id),
                    format!("Clip overlaps the previous clip on track '{}'.", track.name),
                );
            }
        }
        for (i, c) in clips.iter().enumerate() {
            if c.duration <= Tick::ZERO {
                err(&mut out, Some(&c.id), "Clip has no duration.".into());
                continue;
            }
            if !(c.speed > 0.0 && c.speed.is_finite() && c.speed <= 16.0) {
                err(
                    &mut out,
                    Some(&c.id),
                    format!("Clip speed {} is invalid.", c.speed),
                );
            }
            if c.duration < frame && track.kind == TrackKind::Video {
                warn(
                    &mut out,
                    Some(&c.id),
                    "Clip is shorter than one frame.".into(),
                );
            }
            for e in &c.effects {
                if let Err(er) = e.kind.validate() {
                    err(&mut out, Some(&c.id), er.message);
                }
                if e.offset < Tick::ZERO || e.offset + e.duration > c.duration + frame {
                    warn(
                        &mut out,
                        Some(&c.id),
                        format!("Effect '{}' extends beyond its clip.", e.kind.type_name()),
                    );
                }
            }
            if track.kind == TrackKind::Text {
                continue;
            }
            let Some(a) = assets.get(&c.asset_id) else {
                err(
                    &mut out,
                    Some(&c.id),
                    "Source media for this clip is missing.".into(),
                );
                continue;
            };
            if track.kind == TrackKind::Video && !a.has_video {
                err(
                    &mut out,
                    Some(&c.id),
                    "Clip on a video track references media without video.".into(),
                );
            }
            if track.kind == TrackKind::Audio && !a.has_audio {
                err(
                    &mut out,
                    Some(&c.id),
                    "Clip on an audio track references media without audio.".into(),
                );
            }
            if a.is_image {
                continue;
            }
            if c.source_in < Tick::ZERO {
                err(
                    &mut out,
                    Some(&c.id),
                    "Clip starts before the beginning of its source.".into(),
                );
            }
            // One frame of tolerance for rounding of source durations.
            if c.source_out() > a.duration + frame {
                err(
                    &mut out,
                    Some(&c.id),
                    "Clip extends beyond the end of its source media.".into(),
                );
            }
            if let Some(tr) = &c.transition_in {
                if i == 0 || clips[i - 1].end() != c.start {
                    warn(
                        &mut out,
                        Some(&c.id),
                        "Transition has no adjacent previous clip; it will be ignored.".into(),
                    );
                } else if tr.needs_handles() {
                    let prev = clips[i - 1];
                    let half = tr.half();
                    let in_handle = c.source_in - half.scale_f64(c.speed);
                    let out_handle = prev.source_out() + half.scale_f64(prev.speed);
                    let prev_dur = assets
                        .get(&prev.asset_id)
                        .map(|a| if a.is_image { Tick::MAX } else { a.duration })
                        .unwrap_or(Tick::ZERO);
                    if in_handle < Tick::ZERO || out_handle > prev_dur + frame {
                        err(
                            &mut out,
                            Some(&c.id),
                            "Transition needs more source media (handles) than available.".into(),
                        );
                    }
                }
            }
        }
    }
    if t.duration() <= Tick::ZERO {
        err(&mut out, None, "The timeline is empty.".into());
    }
    out
}

pub fn has_errors(issues: &[ValidationIssue]) -> bool {
    issues
        .iter()
        .any(|i| i.severity == ValidationSeverity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{Transition, TransitionKind};
    use crate::model::*;

    fn s(x: f64) -> Tick {
        Tick::from_seconds_f64(x)
    }

    #[test]
    fn detects_missing_media_and_out_of_range() {
        let mut t = Timeline::new(TimelineSettings::default());
        let mut tr = Track::new("v", TrackKind::Video, "V");
        tr.clips.push(Clip::new("a", "m1", s(0.0), s(2.0), s(9.0)));
        tr.clips
            .push(Clip::new("b", "missing", s(2.0), s(1.0), s(0.0)));
        t.tracks.push(tr);
        let mut assets = HashMap::new();
        assets.insert(
            "m1".into(),
            AssetInfo {
                duration: s(10.0),
                has_video: true,
                has_audio: false,
                is_image: false,
            },
        );
        let issues = validate(&t, &assets);
        assert!(issues
            .iter()
            .any(|i| i.clip_id.as_deref() == Some("a") && i.message.contains("beyond the end")));
        assert!(issues
            .iter()
            .any(|i| i.clip_id.as_deref() == Some("b") && i.message.contains("missing")));
    }

    #[test]
    fn transition_handles() {
        let mut t = Timeline::new(TimelineSettings::default());
        let mut tr = Track::new("v", TrackKind::Video, "V");
        tr.clips.push(Clip::new("a", "m1", s(0.0), s(2.0), s(8.0))); // source out = 10.0 = end of media
        let mut b = Clip::new("b", "m1", s(2.0), s(2.0), s(0.0)); // no handle before
        b.transition_in = Some(Transition {
            kind: TransitionKind::CrossDissolve,
            duration: s(0.5),
        });
        tr.clips.push(b);
        t.tracks.push(tr);
        let mut assets = HashMap::new();
        assets.insert(
            "m1".into(),
            AssetInfo {
                duration: s(10.0),
                has_video: true,
                has_audio: false,
                is_image: false,
            },
        );
        assert!(has_errors(&validate(&t, &assets)));
        // A flash cut does not need handles.
        t.tracks[0].clips[1].transition_in = Some(Transition {
            kind: TransitionKind::FlashCut,
            duration: s(0.2),
        });
        assert!(!has_errors(&validate(&t, &assets)));
    }
}
