//! blindgate asks what a gate read while it ran.
//!
//! This crate is the Rust port of the blindgate Python package,
//! and the landed Python is the behavioral spec it is judged
//! against. The port covers the trace reading whole. The
//! reader model in `model` is what a trace read produces, the
//! line classification in `classify` decides whether a text
//! holds a recognizable strace line and refuses one that does
//! not, the canonical scoping in `scope` resolves a traced path
//! and judges where it lands against the root, the parse
//! entry in `parse` turns trace text into the reader model,
//! carrying the directory and descriptor state, the split
//! line reassembly, the fork seeding and sharing, and the
//! exec closings the trace records, and the entries in
//! `entries` read a trace from a file and capture one live
//! over the parse.
//!
//! Everything the port section of the contract does not name
//! stays with the Python, including the workflow, config,
//! analysis, report, and command line modules whole.

pub mod classify;
pub mod entries;
pub mod errors;
pub mod model;
pub mod parse;
pub mod scope;

pub use crate::classify::{looks_like_strace, refuse_unshaped_trace};
pub use crate::entries::{read_set_from_log_file, read_trace_from_log_file, run_traced};
pub use crate::errors::BlindgateError;
pub use crate::model::TraceRead;
pub use crate::parse::{parse_strace_log, parse_trace, parse_trace_with_switch};
