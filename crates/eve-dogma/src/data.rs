//! Static data (re-exported from `eve-sde`) + the generated effect code over the engine's `Fit`.
#![allow(clippy::all, dead_code)]
pub use eve_sde::*;
use crate::engine::{Fit, Src};

include!(concat!(env!("OUT_DIR"), "/effects.rs"));
