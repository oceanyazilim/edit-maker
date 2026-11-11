//! Canonical project timebase.
//!
//! The authoritative time unit is the *tick* (a.k.a. "flick"): 1/705_600_000 s.
//! It divides evenly into every common video frame rate (24, 25, 30, 48, 50, 60,
//! 90, 100, 120 and NTSC x/1001 variants) and every common audio sample rate
//! (8k, 16k, 22.05k, 24k, 32k, 44.1k, 48k, 88.2k, 96k, 192k), so frame and sample
//! boundaries are exactly representable. An `i64` covers ~414 years.
//!
//! Floating-point seconds are only ever used at the edges (UI display, FFmpeg
//! arguments), never as the source of truth.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

pub const TICKS_PER_SECOND: i64 = 705_600_000;

#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Tick(pub i64);

#[inline]
fn div_round_i128(n: i128, d: i128) -> i128 {
    debug_assert!(d > 0);
    if n >= 0 {
        (n + d / 2) / d
    } else {
        -((-n + d / 2) / d)
    }
}

impl Tick {
    pub const ZERO: Tick = Tick(0);
    pub const MAX: Tick = Tick(i64::MAX);

    pub fn from_seconds_f64(s: f64) -> Tick {
        Tick((s * TICKS_PER_SECOND as f64).round() as i64)
    }
    pub fn as_seconds_f64(self) -> f64 {
        self.0 as f64 / TICKS_PER_SECOND as f64
    }
    pub fn from_millis(ms: i64) -> Tick {
        Tick(ms * (TICKS_PER_SECOND / 1000))
    }
    /// Exact for all standard sample rates; rounded otherwise.
    pub fn from_samples(samples: i64, sample_rate: u32) -> Tick {
        Tick(div_round_i128(
            samples as i128 * TICKS_PER_SECOND as i128,
            sample_rate as i128,
        ) as i64)
    }
    /// Nearest sample index at `sample_rate`.
    pub fn to_samples(self, sample_rate: u32) -> i64 {
        div_round_i128(
            self.0 as i128 * sample_rate as i128,
            TICKS_PER_SECOND as i128,
        ) as i64
    }
    /// Start time of frame `frame` at `fps` (exact, no drift accumulation).
    pub fn from_frame(frame: i64, fps: Rational) -> Tick {
        Tick(div_round_i128(
            frame as i128 * TICKS_PER_SECOND as i128 * fps.den as i128,
            fps.num as i128,
        ) as i64)
    }
    /// Index of the frame that is displayed at this time (floor).
    pub fn to_frame_floor(self, fps: Rational) -> i64 {
        let n = self.0 as i128 * fps.num as i128;
        let d = TICKS_PER_SECOND as i128 * fps.den as i128;
        n.div_euclid(d) as i64
    }
    /// Index of the nearest frame boundary.
    pub fn to_frame_round(self, fps: Rational) -> i64 {
        div_round_i128(
            self.0 as i128 * fps.num as i128,
            TICKS_PER_SECOND as i128 * fps.den as i128,
        ) as i64
    }
    /// Snap to the nearest frame boundary.
    pub fn snap_to_frame(self, fps: Rational) -> Tick {
        Tick::from_frame(self.to_frame_round(fps), fps)
    }
    pub fn max(self, o: Tick) -> Tick {
        if self >= o {
            self
        } else {
            o
        }
    }
    pub fn min(self, o: Tick) -> Tick {
        if self <= o {
            self
        } else {
            o
        }
    }
    pub fn clamp(self, lo: Tick, hi: Tick) -> Tick {
        self.max(lo).min(hi)
    }
    pub fn abs(self) -> Tick {
        Tick(self.0.abs())
    }
    /// Scale by a floating factor (used for speed changes); rounds to nearest tick.
    pub fn scale_f64(self, f: f64) -> Tick {
        Tick((self.0 as f64 * f).round() as i64)
    }
}

impl Add for Tick {
    type Output = Tick;
    fn add(self, o: Tick) -> Tick {
        Tick(self.0 + o.0)
    }
}
impl Sub for Tick {
    type Output = Tick;
    fn sub(self, o: Tick) -> Tick {
        Tick(self.0 - o.0)
    }
}
impl AddAssign for Tick {
    fn add_assign(&mut self, o: Tick) {
        self.0 += o.0
    }
}
impl SubAssign for Tick {
    fn sub_assign(&mut self, o: Tick) {
        self.0 -= o.0
    }
}
impl Mul<i64> for Tick {
    type Output = Tick;
    fn mul(self, k: i64) -> Tick {
        Tick(self.0 * k)
    }
}
impl Neg for Tick {
    type Output = Tick;
    fn neg(self) -> Tick {
        Tick(-self.0)
    }
}
impl fmt::Display for Tick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.6}s", self.as_seconds_f64())
    }
}

/// Positive rational number, used for frame rates and aspect ratios.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational {
    pub num: i64,
    pub den: i64,
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

impl Rational {
    pub const FPS_24: Rational = Rational { num: 24, den: 1 };
    pub const FPS_25: Rational = Rational { num: 25, den: 1 };
    pub const FPS_30: Rational = Rational { num: 30, den: 1 };
    pub const FPS_60: Rational = Rational { num: 60, den: 1 };

    pub fn new(num: i64, den: i64) -> Option<Rational> {
        if num <= 0 || den <= 0 {
            return None;
        }
        let g = gcd(num, den);
        Some(Rational {
            num: num / g,
            den: den / g,
        })
    }
    /// Parses "30000/1001", "30" or "29.97" (decimal is approximated to x/1000).
    pub fn parse(s: &str) -> Option<Rational> {
        let s = s.trim();
        if let Some((a, b)) = s.split_once('/') {
            return Rational::new(a.trim().parse().ok()?, b.trim().parse().ok()?);
        }
        if let Ok(i) = s.parse::<i64>() {
            return Rational::new(i, 1);
        }
        let f: f64 = s.parse().ok()?;
        Rational::new((f * 1000.0).round() as i64, 1000)
    }
    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
    /// Duration of one frame in ticks (rounded; exact for standard rates).
    pub fn frame_duration(self) -> Tick {
        Tick::from_frame(1, self)
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_frame_rates_are_exact() {
        for fps in [
            Rational::new(24, 1).unwrap(),
            Rational::new(25, 1).unwrap(),
            Rational::new(30, 1).unwrap(),
            Rational::new(60, 1).unwrap(),
            Rational::new(120, 1).unwrap(),
            Rational::new(24000, 1001).unwrap(),
            Rational::new(30000, 1001).unwrap(),
            Rational::new(60000, 1001).unwrap(),
        ] {
            let exact = TICKS_PER_SECOND as i128 * fps.den as i128 % fps.num as i128 == 0;
            assert!(exact, "fps {fps} not exact");
            // No drift after one hour of frames.
            let frames = (3600.0 * fps.as_f64()).round() as i64;
            let t = Tick::from_frame(frames, fps);
            assert_eq!(t.to_frame_floor(fps), frames);
            assert_eq!(t.to_frame_round(fps), frames);
        }
    }

    #[test]
    fn standard_sample_rates_are_exact() {
        for sr in [
            8000u32, 16000, 22050, 24000, 32000, 44100, 48000, 88200, 96000, 192000,
        ] {
            assert_eq!(TICKS_PER_SECOND % sr as i64, 0, "sr {sr}");
            let t = Tick::from_samples(1_234_567, sr);
            assert_eq!(t.to_samples(sr), 1_234_567);
        }
    }

    #[test]
    fn frame_floor_and_snap() {
        let fps = Rational::FPS_30;
        let half = Tick::from_frame(10, fps) + Tick(fps.frame_duration().0 / 2 - 1);
        assert_eq!(half.to_frame_floor(fps), 10);
        assert_eq!(half.snap_to_frame(fps), Tick::from_frame(10, fps));
        assert_eq!(Tick(-1).to_frame_floor(fps), -1);
    }

    #[test]
    fn parse_rationals() {
        assert_eq!(Rational::parse("30000/1001"), Rational::new(30000, 1001));
        assert_eq!(Rational::parse("60/2"), Rational::new(30, 1));
        assert_eq!(Rational::parse("25"), Rational::new(25, 1));
        assert_eq!(Rational::parse("0/0"), None);
        assert_eq!(Rational::parse("29.97").unwrap().as_f64(), 29.97);
    }

    #[test]
    fn serde_is_plain_integer() {
        let t = Tick(42);
        assert_eq!(serde_json::to_string(&t).unwrap(), "42");
    }
}
