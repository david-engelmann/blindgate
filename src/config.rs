//! The config reading PORT4-3 states.
//!
//! A config is read as UTF-8 text, parsed as JSON or as
//! the YAML subset by its suffix, and validated into the
//! checks it declares, in the Python's order with the
//! Python's wordings. The reading converts the path the
//! way Path converts it before any of that, so the suffix
//! and every message carry the converted spelling.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::entries::{normalize_path, os_error_inner, resolve_root};
use crate::errors::BlindgateError;
use crate::model::Check;
use crate::value::{parse_json, Value};
use crate::yaml::parse_yaml;

/// Load the checks a config file declares.
///
/// The refusal wordings are the Python's own for the read
/// failure, the suffix refusal, and every validation step.
/// The parse failure keeps its outer wording and carries
/// the port's own inner text, because PORT4-3 states that
/// the decoders' texts are not reproduced. A file that is
/// not valid UTF-8 refuses in the port's own words, where
/// the Python lets the decode failure escape uncaught.
pub fn load_config(path: &Path) -> Result<Vec<Check>, BlindgateError> {
    let converted = normalize_path(path);
    let spelled = converted.to_string_lossy().into_owned();
    let bytes = fs::read(&converted).map_err(|err| {
        BlindgateError::new(format!(
            "cannot read config file {spelled} ({})",
            os_error_inner(&err, &spelled)
        ))
    })?;
    let text = String::from_utf8(bytes).map_err(|err| {
        BlindgateError::new(format!(
            "config file {spelled} does not hold valid UTF-8 text ({err})"
        ))
    })?;
    let suffix = path_suffix(&converted);
    let data = match suffix.as_str() {
        ".json" => parse_json(&text).map_err(|inner| {
            BlindgateError::new(format!("config file {spelled} is not valid JSON ({inner})"))
        })?,
        ".yaml" | ".yml" => parse_yaml(&text).map_err(|inner| {
            BlindgateError::new(format!("config file {spelled} is not valid YAML ({inner})"))
        })?,
        _ => {
            return Err(BlindgateError::new(format!(
                "config file {spelled} has an unsupported suffix, use .json, .yaml, or .yml"
            )));
        }
    };
    validate_config(&converted, &spelled, &data)
}

/// The checks whose job the workflow does not declare.
///
/// The order of the config is preserved, as the Python's
/// comprehension preserves it.
pub fn unknown_jobs(checks: &[Check], workflow_jobs: &BTreeSet<String>) -> Vec<Check> {
    checks
        .iter()
        .filter(|check| !workflow_jobs.contains(&check.job))
        .cloned()
        .collect()
}

/// The suffix of a converted path, lowercased.
///
/// The suffix is the last dot's tail of the final
/// component, and a name whose last dot opens the name or
/// closes it has no suffix, which is how Path reads such
/// names.
fn path_suffix(path: &Path) -> String {
    let name = match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => return String::new(),
    };
    let dot = match name.rfind('.') {
        Some(dot) => dot,
        None => return String::new(),
    };
    if dot == 0 || dot == name.len() - 1 {
        return String::new();
    }
    name[dot..].to_lowercase()
}

/// Validate a parsed config into its checks.
///
/// The steps run in the Python's order, so a config that
/// fails two checks reports the first one's wording.
fn validate_config(
    converted: &Path,
    spelled: &str,
    data: &Value,
) -> Result<Vec<Check>, BlindgateError> {
    let entries = match data.get("checks") {
        Some(Value::List(entries)) if data.as_map().is_some() => entries,
        _ => {
            return Err(BlindgateError::new(format!(
                "config file {spelled} must hold a checks list"
            )));
        }
    };
    let mut checks = Vec::new();
    for (position, entry) in entries.iter().enumerate() {
        let index = position;
        if entry.as_map().is_none() {
            return Err(BlindgateError::new(format!(
                "config check number {index} is not a mapping"
            )));
        }
        let name = match entry.get("name") {
            Some(Value::Text(name)) if !name.is_empty() => name.clone(),
            _ => {
                return Err(BlindgateError::new(format!(
                    "config check number {index} has no name"
                )));
            }
        };
        let job = match entry.get("job") {
            Some(Value::Text(job)) if !job.is_empty() => job.clone(),
            _ => {
                return Err(BlindgateError::new(format!("check {name} has no job id")));
            }
        };
        let surface = match entry.get("surface") {
            Some(Value::List(surface)) if !surface.is_empty() => {
                let mut texts = Vec::new();
                for item in surface {
                    match item {
                        Value::Text(text) if !text.is_empty() => texts.push(text.clone()),
                        _ => {
                            return Err(BlindgateError::new(format!(
                                "check {name} has a claimed surface entry that is not text"
                            )));
                        }
                    }
                }
                texts
            }
            _ => {
                return Err(BlindgateError::new(format!(
                    "check {name} declares no claimed surface"
                )));
            }
        };
        let (trace, trace_path) = match entry.get("trace") {
            None | Some(Value::Null) => (None, None),
            Some(Value::Text(trace)) if !trace.is_empty() => {
                let candidate = PathBuf::from(trace);
                let resolved = if candidate.is_absolute() {
                    candidate
                } else {
                    let parent = resolve_root(converted)
                        .parent()
                        .map(|parent| parent.to_path_buf())
                        .unwrap_or_default();
                    parent.join(&candidate)
                };
                // The carried path is the normalized form,
                // as the Python's Path conversion carries
                // it. The trace text itself stands as
                // spelled.
                let resolved = normalize_path(&resolved);
                (Some(trace.clone()), Some(resolved))
            }
            Some(_) => {
                return Err(BlindgateError::new(format!(
                    "check {name} has a trace field that is not a non empty piece of text"
                )));
            }
        };
        checks.push(Check {
            name,
            job,
            surface,
            trace,
            trace_path,
        });
    }
    if checks.is_empty() {
        return Err(BlindgateError::new(format!(
            "config file {spelled} declares no checks"
        )));
    }
    Ok(checks)
}
