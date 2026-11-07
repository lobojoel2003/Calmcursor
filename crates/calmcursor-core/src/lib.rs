//! # calmcursor-core
//!
//! Platform-independent heart of [Calmcursor](https://github.com/lobojoel2003/Calmcursor),
//! a pointer steadier for people with hand tremor (Parkinson's disease,
//! essential tremor and others).
//!
//! The crate has no OS dependencies and is fully deterministic, so all of the
//! behaviour that matters can be unit-tested:
//!
//! * [`filter`]: an adaptive low-pass filter (the One Euro filter) that removes
//!   fast, small tremor movement while staying responsive to deliberate moves.
//! * [`engine`]: the real-time pipeline the OS hooks drive. It tracks the raw
//!   pointer, emits the smoothed one, holds the pointer still while a button is
//!   pressed ("click lock") and drops accidental double presses.
//! * [`tremor`] and [`calibration`]: measure a person's tremor from a short
//!   recording and pick filter settings tuned to them.
//! * [`simulate`]: a synthetic shaky hand used for demos and tests.
//! * [`settings`]: the persisted, versioned user configuration.

#![forbid(unsafe_code)]

pub mod calibration;
pub mod engine;
pub mod filter;
pub mod geometry;
pub mod settings;
pub mod simulate;
pub mod tremor;
pub mod version;

pub use engine::{Button, ButtonAction, Engine, EngineStats, TraceSample};
pub use filter::{OneEuro2D, OneEuroParams};
pub use geometry::{Rect, Vec2};
pub use settings::{ClickSettings, FilterSettings, Settings};
