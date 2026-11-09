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
//! The judgement stack stands in the port as well. The value
//! model in `value` is what the config and workflow documents
//! read into, with the JSON reading and CPython's float
//! spelling, the subset reader in `yaml` reads the YAML
//! documents, the readers in `config` and `workflow` validate
//! those documents into checks and job ids, the judgements in
//! `analysis` pair checks with read sets and decide verdicts,
//! and the renderings in `report` produce the report and the
//! blindgate-evidence/1 document. The command line stands
//! in the binary shell since PORT5, composing this library
//! whole, and the Python package it replaces is retired.

pub mod analysis;
pub mod classify;
pub mod config;
pub mod entries;
pub mod errors;
pub mod model;
pub mod parse;
pub mod report;
mod repr_table;
pub mod scope;
pub mod value;
pub mod workflow;
pub mod yaml;

pub use crate::analysis::{
    analyse, analyse_judged, analyse_with_outside, path_matches_surface, verdict_for, Judgement,
    JudgementPart, SOURCE_OWN_TRACE, SOURCE_SHARED_RUN, SOURCE_SHARED_TRACE,
};
pub use crate::classify::{looks_like_strace, refuse_unshaped_trace};
pub use crate::config::{load_config, unknown_jobs};
pub use crate::entries::{
    read_set_from_log_file, read_trace_from_log_file, resolve_root, run_traced,
};
pub use crate::errors::BlindgateError;
pub use crate::model::{
    Check, CheckResult, TraceRead, TraceSource, VERDICT_EMPTY, VERDICT_PROVEN, VERDICT_UNPROVEN,
};
pub use crate::parse::{parse_strace_log, parse_trace, parse_trace_with_switch};
pub use crate::report::{
    evidence_document, render_evidence, render_json, render_text, report_to_dict, result_to_dict,
    source_text,
};
pub use crate::value::{parse_json, IntVal, Value};
pub use crate::workflow::load_workflow_jobs;
pub use crate::yaml::parse_yaml;
