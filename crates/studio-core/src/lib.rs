//! Shared primitives for AI Auto Edit Studio.
//!
//! * [`time`]   – integer timebase (flicks) and rational frame rates.
//! * [`error`]  – structured, user-presentable errors.
//! * [`rng`]    – deterministic, version-stable PRNG used for reproducible edit plans.
//! * [`cancel`] – cooperative cancellation + progress reporting for long-running work.
//! * [`ids`]    – identifier helpers.

pub mod cancel;
pub mod error;
pub mod ids;
pub mod rng;
pub mod time;

pub use cancel::{CancelToken, Progress, ProgressSink};
pub use error::{ErrorCode, StudioError, StudioResult};
pub use rng::StudioRng;
pub use time::{Rational, Tick, TICKS_PER_SECOND};
