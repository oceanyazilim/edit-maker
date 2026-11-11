//! Clip Discovery: query the indexed scene database of a profile's library.
//!
//! Filters operate on measured scene properties (motion, energy, quality,
//! duration, camera movement tags…) plus file-name text. There is no semantic
//! recognition of characters or subjects yet; queries for such terms only
//! match file names, and the result says so (`semantic_search_available = false`).

use serde::{Deserialize, Serialize};
use studio_core::Tick;
use studio_vision::VideoAnalysis;

use crate::library::ProfileAsset;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ClipQuery {
    /// Words matched against the file name and scene tags.
    pub text: String,
    /// All of these tags must be present (e.g. "high_motion", "pan_left", "dark").
    pub tags: Vec<String>,
    pub min_motion: Option<f32>,
    pub max_motion: Option<f32>,
    pub min_quality: Option<f32>,
    pub min_duration_secs: Option<f64>,
    pub max_duration_secs: Option<f64>,
    /// "energy" | "quality" | "motion" | "duration"
    pub sort: String,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClipHit {
    pub fingerprint: String,
    pub path: std::path::PathBuf,
    pub shared: bool,
    pub scene_index: u32,
    pub start: Tick,
    pub end: Tick,
    pub motion: f32,
    pub energy: f32,
    pub quality: f32,
    pub tags: Vec<String>,
    /// Relevance in 0..1 (fraction of matched text terms, then the sort key).
    pub relevance: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryResult {
    pub hits: Vec<ClipHit>,
    pub searched_videos: usize,
    /// Videos in the library without a scene analysis (analyze them to make them searchable).
    pub unanalyzed: Vec<std::path::PathBuf>,
    pub semantic_search_available: bool,
}

pub fn search<'a>(
    q: &ClipQuery,
    videos: impl Iterator<Item = (&'a ProfileAsset, Option<VideoAnalysis>)>,
) -> DiscoveryResult {
    let terms: Vec<String> = q
        .text
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    let mut hits = Vec::new();
    let mut unanalyzed = Vec::new();
    let mut searched = 0;
    for (asset, analysis) in videos {
        let Some(a) = analysis else {
            unanalyzed.push(asset.path.clone());
            continue;
        };
        searched += 1;
        let name = asset
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        for s in a.scenes.iter().filter(|s| s.usable) {
            let dur = (s.end - s.start).as_seconds_f64();
            if q.min_motion.is_some_and(|m| s.motion < m)
                || q.max_motion.is_some_and(|m| s.motion > m)
                || q.min_quality.is_some_and(|m| s.quality < m)
                || q.min_duration_secs.is_some_and(|m| dur < m)
                || q.max_duration_secs.is_some_and(|m| dur > m)
                || !q.tags.iter().all(|t| s.tags.iter().any(|x| x == t))
            {
                continue;
            }
            let matched = terms
                .iter()
                .filter(|t| {
                    name.contains(t.as_str()) || s.tags.iter().any(|x| x.contains(t.as_str()))
                })
                .count();
            if !terms.is_empty() && matched == 0 {
                continue;
            }
            let text_rel = if terms.is_empty() {
                1.0
            } else {
                matched as f32 / terms.len() as f32
            };
            hits.push(ClipHit {
                fingerprint: asset.fingerprint.clone(),
                path: asset.path.clone(),
                shared: asset.shared,
                scene_index: s.index,
                start: s.start,
                end: s.end,
                motion: s.motion,
                energy: s.energy,
                quality: s.quality,
                tags: s.tags.clone(),
                relevance: text_rel,
            });
        }
    }
    let key = |h: &ClipHit| -> f32 {
        match q.sort.as_str() {
            "quality" => h.quality,
            "motion" => h.motion,
            "duration" => (h.end - h.start).as_seconds_f64() as f32,
            _ => h.energy,
        }
    };
    hits.sort_by(|a, b| {
        (b.relevance, key(b))
            .partial_cmp(&(a.relevance, key(a)))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(if q.limit == 0 { 200 } else { q.limit.min(1000) });
    DiscoveryResult {
        hits,
        searched_videos: searched,
        unanalyzed,
        semantic_search_available: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::Rational;
    use studio_vision::Scene;

    fn scene(i: u32, motion: f32, tags: &[&str], dur: f64) -> Scene {
        Scene {
            index: i,
            start: Tick::from_seconds_f64(i as f64 * 10.0),
            end: Tick::from_seconds_f64(i as f64 * 10.0 + dur),
            motion,
            motion_peak: motion,
            motion_peak_t: None,
            impacts: vec![],
            brightness: 0.5,
            contrast: 0.2,
            saturation: 0.3,
            sharpness: 0.5,
            black_ratio: 0.0,
            stability: 1.0,
            camera_dx: 0.0,
            camera_dy: 0.0,
            energy: motion,
            quality: 0.6,
            focus_x: 0.5,
            focus_y: 0.5,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            usable: true,
        }
    }

    #[test]
    fn filters_and_ranks() {
        let asset = ProfileAsset {
            fingerprint: "f".into(),
            path: "C:/clips/drift_night.mp4".into(),
            kind: "video".into(),
            duration_s: 30.0,
            shared: false,
            owner_profile: "p".into(),
        };
        let other = ProfileAsset {
            fingerprint: "g".into(),
            path: "C:/clips/forest.mp4".into(),
            kind: "video".into(),
            duration_s: 30.0,
            shared: true,
            owner_profile: "q".into(),
        };
        let va = |scenes| VideoAnalysis {
            version: 1,
            analysis_fps: Rational::FPS_30,
            frame_count: 1,
            width: 1,
            height: 1,
            cuts: vec![],
            scenes,
            curves: Default::default(),
        };
        let vids = vec![
            (
                &asset,
                Some(va(vec![
                    scene(0, 0.9, &["high_motion", "pan_right", "dark"], 3.0),
                    scene(1, 0.1, &["low_motion"], 6.0),
                ])),
            ),
            (&other, Some(va(vec![scene(0, 0.7, &["high_motion"], 2.0)]))),
        ];
        let r = search(
            &ClipQuery {
                tags: vec!["high_motion".into()],
                ..Default::default()
            },
            vids.clone().into_iter(),
        );
        assert_eq!(r.hits.len(), 2);
        assert!(r.hits[0].energy >= r.hits[1].energy);
        let r = search(
            &ClipQuery {
                text: "drift".into(),
                ..Default::default()
            },
            vids.clone().into_iter(),
        );
        assert!(r.hits.iter().all(|h| h.fingerprint == "f"));
        let r = search(
            &ClipQuery {
                min_duration_secs: Some(5.0),
                ..Default::default()
            },
            vids.clone().into_iter(),
        );
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].scene_index, 1);
        let r = search(
            &ClipQuery {
                text: "dark".into(),
                max_motion: Some(0.5),
                ..Default::default()
            },
            vids.into_iter(),
        );
        assert!(r.hits.is_empty());
        assert!(!r.semantic_search_available);
    }
}
