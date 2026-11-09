//! The workflow reading PORT4-4 states.
//!
//! A workflow file is read as UTF-8 text and parsed as
//! the YAML subset whatever its suffix, and the job ids
//! it declares are its jobs mapping's keys coerced by
//! CPython's str rules, the same coercion the contract's
//! probes recorded.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::entries::{normalize_path, os_error_inner};
use crate::errors::BlindgateError;
use crate::value::{float_repr, Value};
use crate::yaml::parse_yaml;

/// Load the set of job ids a workflow file declares.
///
/// The refusal wordings are the Python's own for the read
/// failure and the shape refusals. The parse failure
/// keeps its outer wording and carries the port's own
/// inner text, under the same stated limit as the config
/// reading, and a file that is not valid UTF-8 refuses in
/// the port's own words, where the Python lets the decode
/// failure escape uncaught.
pub fn load_workflow_jobs(path: &Path) -> Result<BTreeSet<String>, BlindgateError> {
    let converted = normalize_path(path);
    let spelled = converted.to_string_lossy().into_owned();
    let bytes = fs::read(&converted).map_err(|err| {
        BlindgateError::new(format!(
            "cannot read workflow file {spelled} ({})",
            os_error_inner(&err, &spelled)
        ))
    })?;
    let text = String::from_utf8(bytes).map_err(|err| {
        BlindgateError::new(format!(
            "workflow file {spelled} does not hold valid UTF-8 text ({err})"
        ))
    })?;
    let data = parse_yaml(&text).map_err(|inner| {
        BlindgateError::new(format!(
            "workflow file {spelled} is not valid YAML ({inner})"
        ))
    })?;
    if data.as_map().is_none() {
        return Err(BlindgateError::new(format!(
            "workflow file {spelled} does not hold a mapping"
        )));
    }
    let jobs = match data.get("jobs") {
        Some(Value::Map(jobs)) if !jobs.is_empty() => jobs,
        _ => {
            return Err(BlindgateError::new(format!(
                "workflow file {spelled} declares no jobs"
            )));
        }
    };
    let mut ids = BTreeSet::new();
    for (key, _) in jobs {
        ids.insert(coerce_job_id(key));
    }
    Ok(ids)
}

/// Coerce a jobs key to its job id under CPython's str rules.
///
/// A text stands as itself, a bool spells True or False,
/// an integer spells its decimal digits, a float spells
/// the Python float spelling, a null spells None, and a
/// timestamp spells the CPython str form it carries.
fn coerce_job_id(key: &Value) -> String {
    match key {
        Value::Text(text) => text.clone(),
        Value::Bool(value) => {
            if *value {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Int(value) => value.to_string(),
        Value::Float(value) => float_repr(*value),
        Value::Null => "None".to_string(),
        Value::Timestamp(form) => form.clone(),
        Value::List(_) | Value::Map(_) => {
            // CPython refuses an unhashable key while the
            // document is being constructed, so no workflow
            // the reader accepts can carry one here.
            String::new()
        }
    }
}
