//! Static data (re-exported from `exfa-sde`) + the generated effect code over the engine's `Fit`.
#![allow(clippy::all, dead_code)]
pub use exfa_sde::*;
use crate::engine::{Fit, Src};

include!(concat!(env!("OUT_DIR"), "/effects.rs"));
