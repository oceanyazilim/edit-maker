//! Evaluation of a clip's visual state at a timeline time (pure function;
//! the single source of truth for effect semantics).

use studio_core::Tick;
use studio_timeline::{Clip, EffectKind};

#[derive(Clone, Debug, PartialEq)]
pub struct ClipState {
    /// Total zoom (framing × keyframes × punches).
    pub zoom: f32,
    /// Normalized source point placed at canvas centre.
    pub center_x: f32,
    pub center_y: f32,
    /// Additional offset in canvas fractions (keyframes).
    pub offset_x: f32,
    pub offset_y: f32,
    pub rotation_deg: f32,
    /// Shake displacement in fractions of canvas height.
    pub shake_x: f32,
    pub shake_y: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub vignette: f32,
    /// White overlay alpha.
    pub flash: f32,
    /// Black overlay alpha.
    pub black: f32,
    /// RGB split offset in fractions of canvas width.
    pub rgb_split: f32,
    /// Glitch strength 0..1, seed and time bucket (changes ~15×/s).
    pub glitch: f32,
    pub glitch_seed: u32,
    pub glitch_phase: u32,
    /// Radial chromatic aberration (scale delta).
    pub chroma: f32,
    /// Directional blur vector in fractions of canvas width.
    pub blur_x: f32,
    pub blur_y: f32,
    /// Radial zoom-blur streak length (fraction).
    pub zoom_blur: f32,
}

impl ClipState {
    pub fn identity() -> Self {
        Self {
            zoom: 1.0,
            center_x: 0.5,
            center_y: 0.5,
            offset_x: 0.0,
            offset_y: 0.0,
            rotation_deg: 0.0,
            shake_x: 0.0,
            shake_y: 0.0,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            vignette: 0.0,
            flash: 0.0,
            black: 0.0,
            rgb_split: 0.0,
            glitch: 0.0,
            glitch_seed: 0,
            glitch_phase: 0,
            chroma: 0.0,
            blur_x: 0.0,
            blur_y: 0.0,
            zoom_blur: 0.0,
        }
    }
    /// Whether multi-tap sampling (blur / chroma / glitch) is needed.
    pub fn has_spatial_fx(&self) -> bool {
        self.glitch > 1e-3
            || self.chroma > 1e-5
            || self.blur_x.abs() + self.blur_y.abs() > 1e-4
            || self.zoom_blur > 1e-4
    }
    /// Whether per-pixel colour work is needed.
    pub fn has_color_ops(&self) -> bool {
        self.brightness.abs() > 1e-4
            || (self.contrast - 1.0).abs() > 1e-4
            || (self.saturation - 1.0).abs() > 1e-4
            || self.vignette > 1e-4
            || self.flash > 1e-4
            || self.black > 1e-4
    }
}

fn hash_phase(seed: u32, k: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ k.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    (x as f32 / u32::MAX as f32) * std::f32::consts::TAU
}

/// Visual state of `clip` at timeline time `t`. Effects are evaluated with their
/// local time clamped into the clip so transition handles show a stable state.
pub fn clip_state(clip: &Clip, t: Tick) -> ClipState {
    let mut s = ClipState::identity();
    let local = (t - clip.start)
        .clamp(Tick::ZERO, clip.duration - Tick(1))
        .max(Tick::ZERO);
    let f = &clip.framing;
    let (kz, kx, ky) = clip.transform_at(local);
    s.zoom = f.zoom.max(0.05) * kz;
    s.center_x = f.center_x;
    s.center_y = f.center_y;
    s.offset_x = kx;
    s.offset_y = ky;
    s.rotation_deg = f.rotation_deg;
    let secs = local.as_seconds_f64() as f32;
    for e in &clip.effects {
        let Some(p) = e.progress(local) else { continue };
        let et = (local - e.offset).as_seconds_f64() as f32;
        match e.kind {
            EffectKind::ZoomPunch { amount } => {
                // Instant attack, smooth decay reaching exactly 0 at the end.
                s.zoom *= 1.0 + amount * (-4.0 * p).exp() * (1.0 - p);
            }
            EffectKind::Shake {
                amplitude,
                frequency_hz,
                seed,
            } => {
                let decay = (1.0 - p).powf(1.5);
                let w = std::f32::consts::TAU * frequency_hz * et;
                let nx =
                    (w + hash_phase(seed, 1)).sin() + 0.5 * (2.3 * w + hash_phase(seed, 2)).sin();
                let ny = (1.1 * w + hash_phase(seed, 3)).sin()
                    + 0.5 * (2.7 * w + hash_phase(seed, 4)).sin();
                s.shake_x += amplitude * decay * nx / 1.5;
                s.shake_y += amplitude * decay * ny / 1.5;
            }
            EffectKind::Flash { strength } => {
                s.flash = s.flash.max(strength * (1.0 - p) * (1.0 - p));
            }
            EffectKind::RgbSplit { offset } => {
                s.rgb_split += offset * (1.0 - p);
            }
            EffectKind::Color {
                brightness,
                contrast,
                saturation,
            } => {
                s.brightness += brightness;
                s.contrast *= contrast;
                s.saturation *= saturation;
            }
            EffectKind::Vignette { strength } => {
                s.vignette = s.vignette.max(strength);
            }
            EffectKind::Fade { fade_in } => {
                let a = if fade_in { 1.0 - p } else { p };
                s.black = s.black.max(a.clamp(0.0, 1.0));
            }
            EffectKind::Glitch { strength, seed } => {
                let g = strength * (1.0 - p).powf(1.2);
                if g > s.glitch {
                    s.glitch = g;
                    s.glitch_seed = seed;
                    s.glitch_phase = (et * 15.0).floor() as u32;
                }
            }
            EffectKind::ChromaticAberration { amount } => {
                s.chroma += amount * (1.0 - p);
            }
            EffectKind::MotionBlur { amount, angle_deg } => {
                let a = amount * (1.0 - p) * (1.0 - p);
                let r = angle_deg.to_radians();
                s.blur_x += a * r.cos();
                s.blur_y += a * r.sin();
            }
            EffectKind::ZoomBlur { amount } => {
                s.zoom_blur += amount * (1.0 - p) * (1.0 - p);
            }
        }
    }
    let _ = secs;
    s
}

/// Upper bound of the zoom a clip reaches (used to pick decode headroom).
pub fn max_zoom(clip: &Clip) -> f32 {
    let kmax = clip.keyframes.iter().map(|k| k.zoom).fold(1.0f32, f32::max);
    let punch: f32 = clip
        .effects
        .iter()
        .filter_map(|e| {
            if let EffectKind::ZoomPunch { amount } = e.kind {
                Some(amount)
            } else {
                None
            }
        })
        .fold(0.0, f32::max);
    clip.framing.zoom.max(0.05) * kmax * (1.0 + punch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_timeline::{EffectInstance, EffectKind};

    fn s(x: f64) -> Tick {
        Tick::from_seconds_f64(x)
    }

    #[test]
    fn punch_and_flash_decay_to_zero() {
        let mut c = Clip::new("c", "a", s(0.0), s(2.0), s(0.0));
        c.effects.push(EffectInstance {
            id: "z".into(),
            kind: EffectKind::ZoomPunch { amount: 0.2 },
            offset: s(0.5),
            duration: s(0.5),
            version: 1,
            trigger: None,
        });
        c.effects.push(EffectInstance {
            id: "f".into(),
            kind: EffectKind::Flash { strength: 1.0 },
            offset: s(0.5),
            duration: s(0.5),
            version: 1,
            trigger: None,
        });
        assert_eq!(clip_state(&c, s(0.4)).zoom, 1.0);
        let at = clip_state(&c, s(0.5));
        assert!((at.zoom - 1.2).abs() < 1e-4 && (at.flash - 1.0).abs() < 1e-4);
        let near_end = clip_state(&c, s(0.999));
        assert!(near_end.zoom < 1.001 && near_end.flash < 0.001);
        assert!(max_zoom(&c) >= 1.2);
    }

    #[test]
    fn new_effects_decay_and_are_deterministic() {
        let mut c = Clip::new("c", "a", s(0.0), s(2.0), s(0.0));
        for (id, kind) in [
            (
                "g",
                EffectKind::Glitch {
                    strength: 0.8,
                    seed: 3,
                },
            ),
            ("c", EffectKind::ChromaticAberration { amount: 0.02 }),
            (
                "m",
                EffectKind::MotionBlur {
                    amount: 0.05,
                    angle_deg: 0.0,
                },
            ),
            ("z", EffectKind::ZoomBlur { amount: 0.1 }),
        ] {
            c.effects.push(EffectInstance {
                id: id.into(),
                kind,
                offset: s(0.0),
                duration: s(0.5),
                version: 1,
                trigger: None,
            });
        }
        let a = clip_state(&c, s(0.0));
        assert!((a.glitch - 0.8).abs() < 1e-4 && (a.chroma - 0.02).abs() < 1e-5);
        assert!(
            (a.blur_x - 0.05).abs() < 1e-5
                && a.blur_y.abs() < 1e-6
                && (a.zoom_blur - 0.1).abs() < 1e-5
        );
        assert!(a.has_spatial_fx());
        assert_eq!(a, clip_state(&c, s(0.0)));
        // Glitch pattern changes over time (new bucket ~every 67 ms).
        assert_ne!(clip_state(&c, s(0.1)).glitch_phase, a.glitch_phase);
        let end = clip_state(&c, s(0.499));
        assert!(end.glitch < 0.01 && end.chroma < 0.0001 && end.blur_x < 0.0001);
        assert!(!clip_state(&c, s(1.0)).has_spatial_fx());
    }

    #[test]
    fn shake_is_deterministic() {
        let mut c = Clip::new("c", "a", s(0.0), s(2.0), s(0.0));
        c.effects.push(EffectInstance {
            id: "s".into(),
            kind: EffectKind::Shake {
                amplitude: 0.02,
                frequency_hz: 12.0,
                seed: 7,
            },
            offset: s(0.0),
            duration: s(1.0),
            version: 1,
            trigger: None,
        });
        let a = clip_state(&c, s(0.1));
        let b = clip_state(&c, s(0.1));
        assert_eq!(a, b);
        assert!(a.shake_x.abs() > 0.0 && a.shake_x.abs() <= 0.02);
    }

    #[test]
    fn fades() {
        let mut c = Clip::new("c", "a", s(0.0), s(2.0), s(0.0));
        c.effects.push(EffectInstance {
            id: "f".into(),
            kind: EffectKind::Fade { fade_in: true },
            offset: s(0.0),
            duration: s(1.0),
            version: 1,
            trigger: None,
        });
        assert!((clip_state(&c, s(0.0)).black - 1.0).abs() < 1e-4);
        assert!((clip_state(&c, s(0.5)).black - 0.5).abs() < 1e-3);
        assert_eq!(clip_state(&c, s(1.5)).black, 0.0);
    }
}
