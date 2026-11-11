//! Editing operations. All operations are frame-snapped, respect track locks and
//! keep tracks non-overlapping; on error the timeline is left unchanged.

use studio_core::{StudioError, StudioResult, Tick};

use crate::model::{Clip, Timeline};

fn locked_err() -> StudioError {
    StudioError::validation("The track is locked.")
}

fn clip_locked_err() -> StudioError {
    StudioError::validation("The clip is locked. Unlock it before changing its timing.")
}

fn overlaps(clips: &[Clip], skip_id: Option<&str>, start: Tick, end: Tick) -> bool {
    clips
        .iter()
        .any(|c| Some(c.id.as_str()) != skip_id && start < c.end() && c.start < end)
}

impl Timeline {
    fn snap_frame(&self, t: Tick) -> Tick {
        t.snap_to_frame(self.settings.fps)
    }

    pub fn insert_clip(&mut self, track_id: &str, mut clip: Clip) -> StudioResult<()> {
        clip.start = self.snap_frame(clip.start).max(Tick::ZERO);
        clip.duration = self.snap_frame(clip.duration);
        if clip.duration <= Tick::ZERO {
            return Err(StudioError::validation("Clip duration must be positive."));
        }
        let track = self
            .track_mut(track_id)
            .ok_or_else(|| StudioError::not_found("Track not found."))?;
        if track.locked {
            return Err(locked_err());
        }
        if overlaps(&track.clips, None, clip.start, clip.end()) {
            return Err(StudioError::validation(
                "The clip would overlap another clip on this track.",
            ));
        }
        track.clips.push(clip);
        track.sort();
        Ok(())
    }

    pub fn remove_clip(&mut self, clip_id: &str) -> StudioResult<Clip> {
        let (ti, ci) = self
            .find_clip(clip_id)
            .ok_or_else(|| StudioError::not_found("Clip not found."))?;
        if self.tracks[ti].locked {
            return Err(locked_err());
        }
        if self.tracks[ti].clips[ci].locked {
            return Err(clip_locked_err());
        }
        Ok(self.tracks[ti].clips.remove(ci))
    }

    /// Remove a clip and close the gap by shifting later clips on the same track left.
    pub fn ripple_delete(&mut self, clip_id: &str) -> StudioResult<Clip> {
        let (ti, ci) = self
            .find_clip(clip_id)
            .ok_or_else(|| StudioError::not_found("Clip not found."))?;
        if self.tracks[ti].locked {
            return Err(locked_err());
        }
        if self.tracks[ti].clips[ci].locked {
            return Err(clip_locked_err());
        }
        let end = self.tracks[ti].clips[ci].end();
        if self.tracks[ti]
            .clips
            .iter()
            .any(|c| c.locked && c.start >= end)
        {
            return Err(StudioError::validation(
                "Ripple delete would move a locked clip.",
            ));
        }
        let removed = self.tracks[ti].clips.remove(ci);
        for c in self.tracks[ti].clips.iter_mut() {
            if c.start >= removed.end() {
                c.start -= removed.duration;
            }
        }
        Ok(removed)
    }

    /// Move a clip to `new_start` on `track_id` (may be another track of the same kind).
    pub fn move_clip(
        &mut self,
        clip_id: &str,
        track_id: &str,
        new_start: Tick,
    ) -> StudioResult<()> {
        let (ti, ci) = self
            .find_clip(clip_id)
            .ok_or_else(|| StudioError::not_found("Clip not found."))?;
        let new_start = self.snap_frame(new_start).max(Tick::ZERO);
        let dest = self
            .tracks
            .iter()
            .position(|t| t.id == track_id)
            .ok_or_else(|| StudioError::not_found("Track not found."))?;
        if self.tracks[ti].locked || self.tracks[dest].locked {
            return Err(locked_err());
        }
        if self.tracks[ti].clips[ci].locked {
            return Err(clip_locked_err());
        }
        if self.tracks[ti].kind != self.tracks[dest].kind {
            return Err(StudioError::validation(
                "Clips can only move between tracks of the same kind.",
            ));
        }
        let dur = self.tracks[ti].clips[ci].duration;
        if overlaps(
            &self.tracks[dest].clips,
            Some(clip_id),
            new_start,
            new_start + dur,
        ) {
            return Err(StudioError::validation(
                "The clip would overlap another clip.",
            ));
        }
        let mut clip = self.tracks[ti].clips.remove(ci);
        clip.start = new_start;
        self.tracks[dest].clips.push(clip);
        self.tracks[dest].sort();
        Ok(())
    }

    /// Move the clip's in-point to `new_start` (timeline time), adjusting source in.
    pub fn trim_start(&mut self, clip_id: &str, new_start: Tick) -> StudioResult<()> {
        let (ti, ci) = self
            .find_clip(clip_id)
            .ok_or_else(|| StudioError::not_found("Clip not found."))?;
        if self.tracks[ti].locked {
            return Err(locked_err());
        }
        if self.tracks[ti].clips[ci].locked {
            return Err(clip_locked_err());
        }
        let new_start = self.snap_frame(new_start).max(Tick::ZERO);
        let c = &self.tracks[ti].clips[ci];
        let end = c.end();
        if new_start >= end {
            return Err(StudioError::validation("Trim would make the clip empty."));
        }
        let new_source_in = c.source_time(new_start);
        if new_source_in < Tick::ZERO {
            return Err(StudioError::validation(
                "Not enough source media before the clip.",
            ));
        }
        if overlaps(&self.tracks[ti].clips, Some(clip_id), new_start, end) {
            return Err(StudioError::validation(
                "Trim would overlap the previous clip.",
            ));
        }
        let c = &mut self.tracks[ti].clips[ci];
        c.source_in = new_source_in;
        c.duration = end - new_start;
        c.start = new_start;
        Ok(())
    }

    /// Move the clip's out-point to `new_end`. `max_source` bounds the source media length.
    pub fn trim_end(
        &mut self,
        clip_id: &str,
        new_end: Tick,
        max_source: Option<Tick>,
    ) -> StudioResult<()> {
        let (ti, ci) = self
            .find_clip(clip_id)
            .ok_or_else(|| StudioError::not_found("Clip not found."))?;
        if self.tracks[ti].locked {
            return Err(locked_err());
        }
        if self.tracks[ti].clips[ci].locked {
            return Err(clip_locked_err());
        }
        let new_end = self.snap_frame(new_end);
        let c = &self.tracks[ti].clips[ci];
        if new_end <= c.start {
            return Err(StudioError::validation("Trim would make the clip empty."));
        }
        if let Some(maxs) = max_source {
            if c.source_time(new_end) > maxs {
                return Err(StudioError::validation(
                    "Not enough source media after the clip.",
                ));
            }
        }
        if overlaps(&self.tracks[ti].clips, Some(clip_id), c.start, new_end) {
            return Err(StudioError::validation("Trim would overlap the next clip."));
        }
        let c = &mut self.tracks[ti].clips[ci];
        c.duration = new_end - c.start;
        Ok(())
    }

    /// Split a clip at timeline time `at`; returns the id of the new right-hand clip.
    pub fn split_clip(
        &mut self,
        clip_id: &str,
        at: Tick,
        new_id: impl Into<String>,
    ) -> StudioResult<String> {
        let (ti, ci) = self
            .find_clip(clip_id)
            .ok_or_else(|| StudioError::not_found("Clip not found."))?;
        if self.tracks[ti].locked {
            return Err(locked_err());
        }
        if self.tracks[ti].clips[ci].locked {
            return Err(clip_locked_err());
        }
        let at = self.snap_frame(at);
        let c = &self.tracks[ti].clips[ci];
        if at <= c.start || at >= c.end() {
            return Err(StudioError::validation(
                "Split point must be inside the clip.",
            ));
        }
        let mut right = c.clone();
        let new_id = new_id.into();
        right.id = new_id.clone();
        right.start = at;
        right.duration = c.end() - at;
        right.source_in = c.source_time(at);
        right.transition_in = None;
        right.fade_in = Tick::ZERO;
        let local_cut = at - c.start;
        // Effects are partitioned by their offset.
        right.effects = c
            .effects
            .iter()
            .filter(|e| e.offset >= local_cut)
            .cloned()
            .map(|mut e| {
                e.offset -= local_cut;
                e
            })
            .collect();
        right.keyframes = c
            .keyframes
            .iter()
            .filter(|k| k.t >= local_cut)
            .cloned()
            .map(|mut k| {
                k.t -= local_cut;
                k
            })
            .collect();
        let left = &mut self.tracks[ti].clips[ci];
        left.duration = local_cut;
        left.fade_out = Tick::ZERO;
        left.effects.retain(|e| e.offset < local_cut);
        left.keyframes.retain(|k| k.t < local_cut);
        self.tracks[ti].clips.insert(ci + 1, right);
        Ok(new_id)
    }

    /// Lock or unlock a clip (protects it from regeneration and timing edits).
    pub fn set_clip_locked(&mut self, clip_id: &str, locked: bool) -> StudioResult<()> {
        let (ti, ci) = self
            .find_clip(clip_id)
            .ok_or_else(|| StudioError::not_found("Clip not found."))?;
        self.tracks[ti].clips[ci].locked = locked;
        Ok(())
    }

    /// Lock every clip of a track that lies entirely inside `[from, to)`; unlock the others
    /// when `exclusive` is set. Returns the number of locked clips.
    pub fn lock_range(
        &mut self,
        track_id: &str,
        from: Tick,
        to: Tick,
        exclusive: bool,
    ) -> StudioResult<usize> {
        let t = self
            .track_mut(track_id)
            .ok_or_else(|| StudioError::not_found("Track not found."))?;
        let mut n = 0;
        for c in t.clips.iter_mut() {
            let inside = c.start >= from && c.end() <= to;
            if inside {
                c.locked = true;
                n += 1;
            } else if exclusive {
                c.locked = false;
            }
        }
        Ok(n)
    }

    /// A copy containing only `[start, end)`, shifted to start at zero. Clips crossing the
    /// boundaries are trimmed (source in adjusted by speed); effects/keyframes are re-timed
    /// and dropped when outside. Used for range export.
    pub fn subrange(&self, start: Tick, end: Tick) -> StudioResult<Timeline> {
        let start = start.max(Tick::ZERO);
        let end = end.min(self.duration());
        if end <= start {
            return Err(StudioError::validation("The selected range is empty."));
        }
        let mut out = self.clone();
        out.duration = Some(end - start);
        for t in out.tracks.iter_mut() {
            let mut kept = Vec::new();
            for c in t.clips.drain(..) {
                if c.end() <= start || c.start >= end {
                    continue;
                }
                let mut n = c.clone();
                let new_start = c.start.max(start);
                let new_end = c.end().min(end);
                let cut_front = new_start - c.start;
                n.source_in = c.source_time(new_start);
                n.start = new_start - start;
                n.duration = new_end - new_start;
                if cut_front > Tick::ZERO {
                    n.transition_in = None;
                    n.fade_in = Tick::ZERO;
                }
                n.effects = c
                    .effects
                    .iter()
                    .filter_map(|e| {
                        let s0 = e.offset - cut_front;
                        let e1 = s0 + e.duration;
                        if e1 <= Tick::ZERO || s0 >= n.duration {
                            return None;
                        }
                        let mut e2 = e.clone();
                        if s0 < Tick::ZERO {
                            // Starts before the range: keep its remaining part.
                            e2.offset = Tick::ZERO;
                            e2.duration = e1;
                        } else {
                            e2.offset = s0;
                        }
                        e2.duration = e2.duration.min(n.duration - e2.offset);
                        Some(e2)
                    })
                    .collect();
                n.keyframes = c
                    .keyframes
                    .iter()
                    .map(|k| {
                        let mut k2 = k.clone();
                        k2.t = (k.t - cut_front).max(Tick::ZERO);
                        k2
                    })
                    .collect();
                kept.push(n);
            }
            t.clips = kept;
        }
        out.markers = self
            .markers
            .iter()
            .filter(|m| m.t >= start && m.t < end)
            .cloned()
            .map(|mut m| {
                m.t -= start;
                m
            })
            .collect();
        Ok(out)
    }

    pub fn add_marker(
        &mut self,
        t: Tick,
        label: impl Into<String>,
        id: impl Into<String>,
    ) -> StudioResult<()> {
        let t = self.snap_frame(t);
        if t < Tick::ZERO {
            return Err(StudioError::validation("Markers must be at or after zero."));
        }
        self.markers.push(crate::model::Marker {
            id: id.into(),
            t,
            label: label.into(),
            kind: crate::model::MarkerKind::User,
        });
        self.markers.sort_by_key(|m| m.t);
        Ok(())
    }

    pub fn remove_marker(&mut self, id: &str) -> StudioResult<()> {
        let n = self.markers.len();
        self.markers.retain(|m| m.id != id);
        if self.markers.len() == n {
            return Err(StudioError::not_found("Marker not found."));
        }
        Ok(())
    }

    /// Add an empty track. Video tracks are added above existing video tracks (rendered on top).
    pub fn add_track(&mut self, kind: crate::model::TrackKind, name: impl Into<String>) -> String {
        use crate::model::{Track, TrackKind};
        let prefix = match kind {
            TrackKind::Video => "V",
            TrackKind::Audio => "A",
            TrackKind::Text => "T",
        };
        let mut i = self.tracks.iter().filter(|t| t.kind == kind).count() + 1;
        while self.track(&format!("{prefix}{i}")).is_some() {
            i += 1;
        }
        let id = format!("{prefix}{i}");
        let track = Track::new(id.clone(), kind, name);
        // Keep video tracks contiguous: insert after the last track of the same kind.
        let pos = self
            .tracks
            .iter()
            .rposition(|t| t.kind == kind)
            .map(|p| p + 1)
            .unwrap_or(self.tracks.len());
        let _ = TrackKind::Video;
        self.tracks.insert(pos, track);
        id
    }

    pub fn set_track_flags(
        &mut self,
        track_id: &str,
        locked: Option<bool>,
        muted: Option<bool>,
        hidden: Option<bool>,
    ) -> StudioResult<()> {
        let t = self
            .track_mut(track_id)
            .ok_or_else(|| StudioError::not_found("Track not found."))?;
        if let Some(v) = locked {
            t.locked = v;
        }
        if let Some(v) = muted {
            t.muted = v;
        }
        if let Some(v) = hidden {
            t.hidden = v;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::History;
    use studio_core::Rational;

    fn s(x: f64) -> Tick {
        Tick::from_seconds_f64(x)
    }

    fn tl() -> Timeline {
        let mut t = Timeline::new(TimelineSettings {
            fps: Rational::FPS_30,
            ..Default::default()
        });
        t.tracks.push(Track::new("v1", TrackKind::Video, "V1"));
        t.tracks.push(Track::new("v2", TrackKind::Video, "V2"));
        t.tracks.push(Track::new("a1", TrackKind::Audio, "A1"));
        t.insert_clip("v1", Clip::new("c1", "m1", s(0.0), s(2.0), s(10.0)))
            .unwrap();
        t.insert_clip("v1", Clip::new("c2", "m2", s(2.0), s(3.0), s(0.0)))
            .unwrap();
        t
    }

    #[test]
    fn insert_rejects_overlap() {
        let mut t = tl();
        assert!(t
            .insert_clip("v1", Clip::new("c3", "m", s(1.0), s(1.5), s(0.0)))
            .is_err());
        assert!(t
            .insert_clip("v2", Clip::new("c3", "m", s(1.0), s(1.5), s(0.0)))
            .is_ok());
    }

    #[test]
    fn split_preserves_source_continuity() {
        let mut t = tl();
        let id = t.split_clip("c1", s(0.5), "c1b").unwrap();
        let a = t.clip("c1").unwrap().clone();
        let b = t.clip(&id).unwrap().clone();
        assert_eq!(a.end(), b.start);
        assert_eq!(a.source_out(), b.source_in);
        assert_eq!(b.source_in, s(10.5));
        assert_eq!(a.duration + b.duration, s(2.0));
    }

    #[test]
    fn split_is_frame_snapped() {
        let mut t = tl();
        t.split_clip("c1", s(0.51), "x").unwrap();
        let b = t.clip("x").unwrap();
        assert_eq!(b.start, Tick::from_frame(15, Rational::FPS_30));
    }

    #[test]
    fn trim_start_adjusts_source() {
        let mut t = tl();
        t.trim_start("c1", s(0.5)).unwrap();
        let c = t.clip("c1").unwrap();
        assert_eq!(c.start, s(0.5));
        assert_eq!(c.source_in, s(10.5));
        assert_eq!(c.end(), s(2.0));
        // Not enough source before c2 (source_in = 0).
        assert!(t.trim_start("c2", s(1.9)).is_err());
    }

    #[test]
    fn trim_end_respects_source_length() {
        let mut t = tl();
        assert!(t.trim_end("c2", s(10.0), Some(s(4.0))).is_err());
        t.trim_end("c2", s(4.0), Some(s(4.0))).unwrap();
        assert_eq!(t.clip("c2").unwrap().duration, s(2.0));
    }

    #[test]
    fn ripple_delete_closes_gap() {
        let mut t = tl();
        t.ripple_delete("c1").unwrap();
        assert_eq!(t.clip("c2").unwrap().start, Tick::ZERO);
    }

    #[test]
    fn move_between_tracks_and_kinds() {
        let mut t = tl();
        t.move_clip("c2", "v2", s(0.0)).unwrap();
        assert_eq!(t.find_clip("c2").unwrap().0, 1);
        assert!(t.move_clip("c2", "a1", s(0.0)).is_err());
    }

    #[test]
    fn locked_tracks_reject_edits() {
        let mut t = tl();
        t.set_track_flags("v1", Some(true), None, None).unwrap();
        assert!(t.split_clip("c1", s(1.0), "x").is_err());
        assert!(t.remove_clip("c1").is_err());
    }

    #[test]
    fn clip_locks_block_timing_edits_but_not_parameters() {
        let mut t = tl();
        t.set_clip_locked("c1", true).unwrap();
        assert!(t.split_clip("c1", s(1.0), "x").is_err());
        assert!(t.trim_start("c1", s(0.5)).is_err());
        assert!(t.trim_end("c1", s(1.5), None).is_err());
        assert!(t.move_clip("c1", "v2", s(0.0)).is_err());
        assert!(t.remove_clip("c1").is_err());
        // Ripple-deleting an earlier clip would shift the locked one: refused.
        t.set_clip_locked("c1", false).unwrap();
        t.set_clip_locked("c2", true).unwrap();
        assert!(t.ripple_delete("c1").is_err());
        // Parameter edits remain possible on locked clips (done via direct mutation by callers).
        t.tracks[0].clips[1].volume = 0.5;
        assert_eq!(t.lock_range("v1", s(0.0), s(2.0), true).unwrap(), 1);
        assert!(t.clip("c1").unwrap().locked && !t.clip("c2").unwrap().locked);
    }

    #[test]
    fn subrange_trims_and_shifts() {
        let mut t = tl();
        t.tracks[0].clips[0].effects.push(crate::EffectInstance {
            id: "e".into(),
            kind: crate::EffectKind::Flash { strength: 1.0 },
            offset: s(1.0),
            duration: s(0.5),
            version: 1,
            trigger: None,
        });
        t.add_marker(s(3.0), "m", "m1").unwrap();
        let r = t.subrange(s(1.2), s(3.5)).unwrap();
        assert_eq!(r.duration(), s(2.3));
        let v = &r.tracks[0].clips;
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].start, Tick::ZERO);
        assert_eq!(v[0].duration, s(0.8));
        assert_eq!(v[0].source_in, s(11.2));
        // Flash that started at 1.0 s keeps its remaining 0.3 s from the range start.
        assert_eq!(v[0].effects[0].offset, Tick::ZERO);
        assert_eq!(v[0].effects[0].duration, s(0.3));
        assert_eq!(v[1].start, s(0.8));
        assert_eq!(v[1].duration, s(1.5));
        assert_eq!(r.markers[0].t, s(1.8));
        assert!(t.subrange(s(2.0), s(2.0)).is_err());
    }

    #[test]
    fn add_track_and_markers() {
        let mut t = tl();
        let id = t.add_track(TrackKind::Video, "Overlay");
        assert_eq!(id, "V3");
        // Inserted after the last video track, before audio.
        let pos = t.tracks.iter().position(|x| x.id == "V3").unwrap();
        assert_eq!(t.tracks[pos + 1].kind, TrackKind::Audio);
        t.add_marker(s(1.0), "a", "m").unwrap();
        t.remove_marker("m").unwrap();
        assert!(t.remove_marker("m").is_err());
    }

    #[test]
    fn undo_redo() {
        let mut t = tl();
        let mut h = History::default();
        h.record("split", &t);
        t.split_clip("c1", s(1.0), "x").unwrap();
        assert!(t.clip("x").is_some());
        h.undo(&mut t);
        assert!(t.clip("x").is_none());
        h.redo(&mut t);
        assert!(t.clip("x").is_some());
    }

    #[test]
    fn snapping() {
        let t = tl();
        assert_eq!(t.snap(s(1.98), s(0.05)), s(2.0));
        assert_eq!(t.snap(s(1.5), s(0.05)), s(1.5));
    }

    #[test]
    fn serde_roundtrip() {
        let t = tl();
        let j = serde_json::to_string(&t).unwrap();
        let back: Timeline = serde_json::from_str(&j).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn keyframe_interpolation() {
        let mut c = Clip::new("c", "m", s(0.0), s(2.0), s(0.0));
        c.keyframes = vec![
            TransformKeyframe {
                t: s(0.0),
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
                ease: Ease::Linear,
            },
            TransformKeyframe {
                t: s(2.0),
                zoom: 1.2,
                offset_x: 0.1,
                offset_y: 0.0,
                ease: Ease::Linear,
            },
        ];
        let (z, x, _) = c.transform_at(s(1.0));
        assert!((z - 1.1).abs() < 1e-5 && (x - 0.05).abs() < 1e-5);
        assert_eq!(c.transform_at(s(5.0)).0, 1.2);
    }
}
