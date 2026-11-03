//! blindgate asks what a gate read while it ran.
//!
//! This crate is the Rust port of the blindgate Python package,
//! and the landed Python is the behavioral spec it is judged
//! against. Port slice 1 covers the trace reading core. The
//! reader model in `model` is what a trace read produces, the
//! line classification in `classify` decides whether a text
//! holds a recognizable strace line and refuses one that does
//! not, the canonical scoping in `scope` resolves a traced path
//! and judges where it lands against the root, and the parse
//! entry in `parse` turns trace text into the reader model.
//!
//! Everything the port section of the contract does not name
//! stays with the Python in this slice, including directory and
//! descriptor state, split line reassembly, fork seeding and
//! sharing, the exec closings, live capture, and the workflow,
//! config, analysis, report, and command line modules whole.

pub mod classify;
pub mod errors;
pub mod model;
pub mod parse;
pub mod scope;

pub use crate::classify::{looks_like_strace, refuse_unshaped_trace};
pub use crate::errors::BlindgateError;
pub use crate::model::TraceRead;
pub use crate::parse::{parse_strace_log, parse_trace, parse_trace_with_switch};
