//! Streaming scene analyzer: `push` frames, then `finish`.

use serde::{Deserialize, Serialize};
use studio_core::{Rational, Tick};

use crate::frame::{self, FrameFeat, GRID_H, GRID_W, HIST_BINS};
use crate::model::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VisionConfig {
    /// Absolute floor for a hard cut score (0..1).
    pub cut_min_score: f32,
    /// Adaptive threshold: local mean + k * local std.
    pub cut_k: f32,
    /// Minimum scene length in seconds.
    pub min_scene_secs: f64,
    /// Near-black luma threshold.
    pub black_luma: f32,
    pub detect_gradual: bool,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            cut_min_score: 0.30,
            cut_k: 4.0,
            min_scene_secs: 0.35,
            black_luma: 0.07,
            detect_gradual: true,
        }
    }
}

struct FrameStat {
    hist: [f32; HIST_BINS * 3],
    hist_d: f32,
    luma_d: f32,
    gm_dx: f32,
    gm_dy: f32,
    brightness: f32,
    contrast: f32,
    saturation: f32,
    sharpness: f32,
    black: bool,
    diff_grid: [f32; GRID_W * GRID_H],
    detail_grid: [f32; GRID_W * GRID_H],
}

pub struct SceneAnalyzer {
    cfg: VisionConfig,
    fps: Rational,
    w: usize,
    h: usize,
    prev: Option<FrameFeat>,
    stats: Vec<FrameStat>,
}

impl SceneAnalyzer {
    pub fn new(cfg: VisionConfig, fps: Rational, width: u32, height: u32) -> Self {
        Self {
            cfg,
            fps,
            w: width as usize,
            h: height as usize,
            prev: None,
            stats: Vec::new(),
        }
    }

    pub fn frame_bytes(&self) -> usize {
        self.w * self.h * 3
    }

    pub fn frames(&self) -> usize {
        self.stats.len()
    }

    /// Push one RGB24 frame of `width * height * 3` bytes.
    pub fn push(&mut self, rgb: &[u8]) {
        assert_eq!(rgb.len(), self.frame_bytes(), "frame size mismatch");
        let f = frame::extract(rgb, self.w, self.h);
        let (mut hist_d, mut luma_d, mut gm_dx, mut gm_dy) = (0.0, 0.0, 0.0, 0.0);
        let mut diff_grid = [0f32; GRID_W * GRID_H];
        if let Some(p) = &self.prev {
            hist_d = frame::hist_distance(&p.hist, &f.hist);
            luma_d = frame::mean_abs_diff(&p.luma, &f.luma);
            let factor = (self.w / 48).max(1);
            let (pl, dw, dh) = frame::downsample(&p.luma, self.w, self.h, factor);
            let (cl, _, _) = frame::downsample(&f.luma, self.w, self.h, factor);
            let (dx, dy, _res, _raw) = frame::global_motion(&pl, &cl, dw, dh, 3);
            gm_dx = dx / dw as f32;
            gm_dy = dy / dh as f32;
            diff_grid = frame::diff_grid(&p.luma, &f.luma, self.w, self.h);
        }
        self.stats.push(FrameStat {
            hist: f.hist,
            hist_d,
            luma_d,
            gm_dx,
            gm_dy,
            brightness: f.brightness,
            contrast: f.contrast,
            saturation: f.saturation,
            sharpness: f.sharpness,
            black: f.brightness < self.cfg.black_luma && f.contrast < 0.04,
            diff_grid,
            detail_grid: f.detail_grid,
        });
        self.prev = Some(f);
    }

    fn t(&self, frame: usize) -> Tick {
        Tick::from_frame(frame as i64, self.fps)
    }

    pub fn finish(self) -> VideoAnalysis {
        let n = self.stats.len();
        let fps_f = self.fps.as_f64();
        let st = &self.stats;
        let score: Vec<f32> = st
            .iter()
            .map(|s| 0.5 * s.hist_d + 0.5 * (s.luma_d / 0.25).min(1.0))
            .collect();
        let min_gap = ((self.cfg.min_scene_secs * fps_f).round() as usize).max(1);
        let win = (fps_f.round() as usize).max(4);

        let mut cuts: Vec<(usize, CutKind, f32)> = Vec::new();
        // Hard cuts.
        for i in 1..n {
            let lo = i.saturating_sub(win);
            let hi = (i + win + 1).min(n);
            let others: Vec<f32> = (lo..hi)
                .filter(|&j| j != i && j > 0)
                .map(|j| score[j])
                .collect();
            let (m, sd) = mean_std(&others);
            let thr = self.cfg.cut_min_score.max(m + self.cfg.cut_k * sd);
            let is_peak =
                (i < 2 || score[i] >= score[i - 1]) && (i + 1 >= n || score[i] >= score[i + 1]);
            if score[i] <= thr || !is_peak {
                continue;
            }
            // Flash rejection: frame i is an outlier but i-1 and i+1 look alike.
            if i + 1 < n
                && i >= 1
                && frame::hist_distance(&st[i - 1].hist, &st[i + 1].hist)
                    < 0.25 * st[i].hist_d.max(1e-3)
            {
                continue;
            }
            // ...or this is the return from a one-frame outlier (i-2 and i look alike).
            if i >= 2
                && frame::hist_distance(&st[i - 2].hist, &st[i].hist)
                    < 0.25 * st[i].hist_d.max(1e-3)
            {
                continue;
            }
            cuts.push((i, CutKind::Hard, score[i]));
        }
        // Black segments.
        let mut i = 0;
        while i < n {
            if st[i].black {
                let s0 = i;
                while i < n && st[i].black {
                    i += 1;
                }
                if i - s0 >= 2 {
                    if s0 > 0 {
                        cuts.push((s0, CutKind::Black, 1.0));
                    }
                    if i < n {
                        cuts.push((i, CutKind::Black, 1.0));
                    }
                }
            } else {
                i += 1;
            }
        }
        // Gradual transitions: large long-range change without a hard cut inside.
        if self.cfg.detect_gradual {
            let span = ((0.6 * fps_f).round() as usize).max(3);
            let mut long = vec![0f32; n];
            for i in span..n {
                long[i] = frame::hist_distance(&st[i - span].hist, &st[i].hist);
            }
            for i in span..n {
                let lo_w = i.saturating_sub(span);
                let hi_w = (i + span + 1).min(n);
                let peak =
                    (lo_w..hi_w).all(|j| long[j] < long[i] || (long[j] == long[i] && j >= i));
                if !peak || long[i] < 0.45 {
                    continue;
                }
                let lo = i - span + 1;
                if cuts
                    .iter()
                    .any(|c| c.0 >= lo.saturating_sub(1) && c.0 <= i + 1)
                {
                    continue;
                }
                // Change must be spread out: no single frame carries most of it.
                let max_step = st[lo..=i].iter().map(|s| s.hist_d).fold(0f32, f32::max);
                let total: f32 = st[lo..=i].iter().map(|s| s.hist_d).sum();
                if total > 0.0 && max_step / total < 0.5 {
                    cuts.push((i - span / 2, CutKind::Gradual, long[i]));
                }
            }
        }
        // Merge: sort, enforce min gap keeping the strongest (Hard/Black preferred).
        cuts.sort_by_key(|c| c.0);
        let rank = |k: CutKind| match k {
            CutKind::Hard => 2,
            CutKind::Black => 1,
            CutKind::Gradual => 0,
        };
        let mut merged: Vec<(usize, CutKind, f32)> = Vec::new();
        for c in cuts {
            if c.0 == 0 || c.0 >= n {
                continue;
            }
            if let Some(last) = merged.last_mut() {
                if c.0 - last.0 < min_gap {
                    if (rank(c.1), c.2) > (rank(last.1), last.2) && c.1 != CutKind::Black {
                        *last = c;
                    } else if c.1 == CutKind::Black && last.1 == CutKind::Black {
                        // keep both ends of a black segment only if far enough apart
                    }
                    continue;
                }
            }
            merged.push(c);
        }

        // Scenes.
        let mut bounds = vec![0usize];
        bounds.extend(merged.iter().map(|c| c.0));
        bounds.push(n);
        bounds.dedup();
        let mut scenes = Vec::new();
        for (idx, w) in bounds.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            if b <= a {
                continue;
            }
            scenes.push(self.scene_stats(idx as u32, a, b));
        }
        let cuts_out = merged
            .iter()
            .map(|&(i, kind, s)| CutEvent {
                t: self.t(i),
                kind,
                score: s,
            })
            .collect();
        let curves = FrameCurves {
            motion: st
                .iter()
                .map(|s| (s.luma_d * 1000.0).round() / 1000.0)
                .collect(),
            brightness: st
                .iter()
                .map(|s| (s.brightness * 1000.0).round() / 1000.0)
                .collect(),
            cut_score: score
                .iter()
                .map(|v| (v * 1000.0).round() / 1000.0)
                .collect(),
        };
        VideoAnalysis {
            version: VISION_VERSION,
            analysis_fps: self.fps,
            frame_count: n as u64,
            width: self.w as u32,
            height: self.h as u32,
            cuts: cuts_out,
            scenes,
            curves,
        }
    }

    fn scene_stats(&self, index: u32, a: usize, b: usize) -> Scene {
        let st = &self.stats[a..b];
        let len = st.len();
        let fps_f = self.fps.as_f64() as f32;
        // Motion excluding the boundary frame (its diff belongs to the cut).
        let motion_frames: Vec<f32> = st.iter().skip(1).map(|s| s.luma_d).collect();
        let smooth: Vec<f32> = (0..motion_frames.len())
            .map(|i| {
                let lo = i.saturating_sub(1);
                let hi = (i + 2).min(motion_frames.len());
                motion_frames[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
            })
            .collect();
        let (mmean, _) = mean_std(&motion_frames);
        let motion = (mmean / 0.08).clamp(0.0, 1.0);
        let (peak_i, peak_v) = smooth
            .iter()
            .enumerate()
            .fold(
                (None, 0f32),
                |(bi, bv), (i, &v)| if v > bv { (Some(i), v) } else { (bi, bv) },
            );
        let motion_peak = (peak_v / 0.08).clamp(0.0, 1.0);
        // Rescale fps-dependent per-frame diff into a per-second notion isn't needed: analysis fps is fixed per file.
        let med = median(&motion_frames);
        let mut impacts: Vec<(usize, f32)> = (1..motion_frames.len().saturating_sub(1))
            .filter(|&i| {
                let v = motion_frames[i];
                v > motion_frames[i - 1] && v >= motion_frames[i + 1] && v > 2.0 * med && v > 0.03
            })
            .map(|i| (i, motion_frames[i]))
            .collect();
        impacts.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap());
        impacts.truncate(5);

        let brightness = st.iter().map(|s| s.brightness).sum::<f32>() / len as f32;
        let contrast = st.iter().map(|s| s.contrast).sum::<f32>() / len as f32;
        let saturation = st.iter().map(|s| s.saturation).sum::<f32>() / len as f32;
        let sharp_rms = (st.iter().map(|s| s.sharpness).sum::<f32>() / len as f32).sqrt();
        let sharpness = (sharp_rms / 0.10).clamp(0.0, 1.0);
        let black_ratio = st.iter().filter(|s| s.black).count() as f32 / len as f32;
        let gms: Vec<(f32, f32)> = st.iter().skip(1).map(|s| (s.gm_dx, s.gm_dy)).collect();
        let jitter = if gms.len() > 1 {
            gms.windows(2)
                .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
                .sum::<f32>()
                / (gms.len() - 1) as f32
        } else {
            0.0
        };
        let stability = (1.0 - jitter / 0.03).clamp(0.0, 1.0);
        let camera_dx = if gms.is_empty() {
            0.0
        } else {
            gms.iter().map(|g| g.0).sum::<f32>() / gms.len() as f32 * fps_f
        };
        let camera_dy = if gms.is_empty() {
            0.0
        } else {
            gms.iter().map(|g| g.1).sum::<f32>() / gms.len() as f32 * fps_f
        };
        let energy = (0.6 * motion + 0.2 * (contrast / 0.25).min(1.0) + 0.2 * saturation.min(1.0))
            .clamp(0.0, 1.0);
        let exposure = 1.0 - ((brightness - 0.45).abs() / 0.45).clamp(0.0, 1.0);
        let quality =
            (0.35 * sharpness + 0.2 * exposure + 0.2 * stability + 0.25 * (1.0 - black_ratio))
                .clamp(0.0, 1.0);

        // Focus: motion + detail weighted centre of mass with a mild centre prior.
        let mut acc = [0f32; GRID_W * GRID_H];
        for s in st {
            for i in 0..acc.len() {
                acc[i] += s.diff_grid[i] * 2.0 + s.detail_grid[i] * 0.5;
            }
        }
        let (mut sx, mut sy, mut sw) = (0f32, 0f32, 0f32);
        for gy in 0..GRID_H {
            for gx in 0..GRID_W {
                let cx = (gx as f32 + 0.5) / GRID_W as f32;
                let cy = (gy as f32 + 0.5) / GRID_H as f32;
                let prior =
                    (-((cx - 0.5).powi(2) + (cy - 0.5).powi(2)) / (2.0 * 0.35 * 0.35)).exp();
                let wv = acc[gy * GRID_W + gx].powi(2) * prior;
                sx += cx * wv;
                sy += cy * wv;
                sw += wv;
            }
        }
        let (focus_x, focus_y) = if sw > 1e-9 {
            (sx / sw, sy / sw)
        } else {
            (0.5, 0.5)
        };

        let start = self.t(a);
        let end = self.t(b);
        let dur_s = (end - start).as_seconds_f64() as f32;
        let mut tags = Vec::new();
        if motion > 0.55 {
            tags.push("high_motion");
        } else if motion < 0.15 {
            tags.push("low_motion");
        }
        if brightness < 0.22 {
            tags.push("dark");
        } else if brightness > 0.7 {
            tags.push("bright");
        }
        if saturation > 0.45 {
            tags.push("colorful");
        } else if saturation < 0.08 {
            tags.push("monochrome");
        }
        if stability < 0.5 {
            tags.push("shaky");
        }
        let pan = 0.04;
        if camera_dx > pan {
            tags.push("pan_right");
        } else if camera_dx < -pan {
            tags.push("pan_left");
        }
        if camera_dy > pan {
            tags.push("tilt_down");
        } else if camera_dy < -pan {
            tags.push("tilt_up");
        }
        if camera_dx.abs() <= pan && camera_dy.abs() <= pan && stability > 0.8 {
            tags.push("static_camera");
        }
        if dur_s >= 4.0 {
            tags.push("long_take");
        }
        if !impacts.is_empty() && motion_peak > 0.6 {
            tags.push("action_peak");
        }
        Scene {
            index,
            start,
            end,
            motion,
            motion_peak,
            // smooth index i corresponds to frame a + 1 + i
            motion_peak_t: peak_i.map(|i| self.t(a + 1 + i)),
            impacts: impacts.iter().map(|(i, _)| self.t(a + 1 + i)).collect(),
            brightness,
            contrast,
            saturation,
            sharpness,
            black_ratio,
            stability,
            camera_dx,
            camera_dy,
            energy,
            quality,
            focus_x,
            focus_y,
            tags: tags.into_iter().map(String::from).collect(),
            usable: dur_s >= 0.25 && black_ratio < 0.7,
        }
    }
}

fn mean_std(v: &[f32]) -> (f32, f32) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let m = v.iter().sum::<f32>() / v.len() as f32;
    let var = v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / v.len() as f32;
    (m, var.sqrt())
}

fn median(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[s.len() / 2]
}
