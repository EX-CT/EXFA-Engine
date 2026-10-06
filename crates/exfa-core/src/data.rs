//! Static data (`crate::sde`, re-exported flat) + the generated effect code over the engine's `Fit`.
#![allow(clippy::all, dead_code)]
pub use crate::sde::*;
use crate::engine::{Fit, Src};

include!(concat!(env!("OUT_DIR"), "/effects.rs"));
