//! Core building blocks shared by every Kiln crate.
//!
//! * [`handle`]: generational [`Handle`]s and the [`HandlePool`] that owns the values.
//! * [`time`]: frame [`Time`] and the deterministic [`FixedClock`] used for fixed-step updates.
//! * [`log`]: one-call, idempotent [`init_logging`].

pub mod handle;
pub mod log;
pub mod time;

pub use handle::{Handle, HandlePool};
pub use log::init_logging;
pub use time::{FixedClock, Time};
