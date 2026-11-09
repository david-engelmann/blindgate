//! The report rendering PORT4-6 states, and the
//! evidence document PORT4-7 states.
//!
//! The text and JSON renderings reproduce the Python
//! report module byte for byte over the same results. The
//! JSON spelling is CPython's dumps with an indent of two
//! and sorted keys. The evidence document is the port's
//! own emitter for the blindgate-evidence/1 format, whose
//! acceptance shape is the one tameshi's contract states.

use crate::analysis::{SOURCE_OWN_TRACE, SOURCE_SHARED_RUN, SOURCE_SHARED_TRACE};
use crate::model::{CheckResult, TraceSource};

/// The label a trace source renders with.
///
/// A known kind renders its label, an unknown kind
/// renders the kind itself, and a nonempty detail stands
/// after one space.
pub fn source_text(source: &TraceSource) -> String {
    let label = match source.kind.as_str() {
        SOURCE_OWN_TRACE => "own trace file",
        SOURCE_SHARED_TRACE => "shared trace",
        SOURCE_SHARED_RUN => "shared run",
        other => other,
    };
    if source.detail.is_empty() {
        label.to_string()
    } else {
        format!("{label} {}", source.detail)
    }
}

fn shows_sources(results: &[CheckResult]) -> bool {
    results.iter().any(|result| result.trace_source.is_some())
}

fn shows_outside(results: &[CheckResult]) -> bool {
    results
        .iter()
        .any(|result| !result.outside_reads.is_empty())
}

/// One result as a JSON value, under the report's field rules.
///
/// The trace source and outside reads fields stand only
/// when the flags the whole result list decides say so,
/// and a missing source renders as a null.
pub fn result_to_dict(result: &CheckResult, show_source: bool, show_outside: bool) -> Json {
    let mut fields: Vec<(String, Json)> = vec![
        ("name".to_string(), Json::Text(result.name.clone())),
        ("job".to_string(), Json::Text(result.job.clone())),
        ("verdict".to_string(), Json::Text(result.verdict.clone())),
        ("claimed_surface".to_string(), text_list(&result.surface)),
        ("read_set".to_string(), text_list(&result.read_set)),
        ("reads_in_surface".to_string(), text_list(&result.matched)),
    ];
    if show_source {
        let source = match &result.trace_source {
            None => Json::Null,
            Some(source) => Json::Object(vec![
                ("kind".to_string(), Json::Text(source.kind.clone())),
                ("detail".to_string(), Json::Text(source.detail.clone())),
                ("text".to_string(), Json::Text(source_text(source))),
            ]),
        };
        fields.push(("trace_source".to_string(), source));
    }
    if show_outside {
        fields.push((
            "outside_reads".to_string(),
            text_list(&result.outside_reads),
        ));
    }
    Json::Object(fields)
}

/// The whole report as a JSON value.
///
/// The document holds the checks and a summary counting
/// the results by verdict word.
pub fn report_to_dict(results: &[CheckResult]) -> Json {
    let show_source = shows_sources(results);
    let show_outside = shows_outside(results);
    let checks: Vec<Json> = results
        .iter()
        .map(|result| result_to_dict(result, show_source, show_outside))
        .collect();
    let mut summary: Vec<(String, Json)> = Vec::new();
    for result in results {
        match summary.iter_mut().find(|(word, _)| *word == result.verdict) {
            Some((_, Json::Int(count))) => *count += 1,
            _ => summary.push((result.verdict.clone(), Json::Int(1))),
        }
    }
    Json::Object(vec![
        ("checks".to_string(), Json::List(checks)),
        ("summary".to_string(), Json::Object(summary)),
    ])
}

/// Render the report as JSON, byte for byte as the Python does.
pub fn render_json(results: &[CheckResult]) -> String {
    dump_json(&report_to_dict(results))
}

/// Render the report as text, byte for byte as the Python does.
///
/// The text opens with its title and rule and a blank
/// line, then per result the label lines with the labels
/// padded to the standing column and the section heads,
/// each listing indented four spaces with the word none
/// for an empty list, and a blank line after each result,
/// so the text ends with a newline.
pub fn render_text(results: &[CheckResult]) -> String {
    let show_source = shows_sources(results);
    let show_outside = shows_outside(results);
    let mut lines: Vec<String> = vec![
        "blindgate report".to_string(),
        "================".to_string(),
        String::new(),
    ];
    for result in results {
        lines.push(format!("check            {}", result.name));
        lines.push(format!("job              {}", result.job));
        if show_source {
            let rendered = match &result.trace_source {
                None => "not recorded".to_string(),
                Some(source) => source_text(source),
            };
            lines.push(format!("trace source     {rendered}"));
        }
        lines.push(format!("verdict          {}", result.verdict));
        lines.push("claimed surface".to_string());
        push_listing(&mut lines, &result.surface);
        lines.push("read set".to_string());
        push_listing(&mut lines, &result.read_set);
        lines.push("reads in surface".to_string());
        push_listing(&mut lines, &result.matched);
        if show_outside {
            lines.push("outside reads".to_string());
            push_listing(&mut lines, &result.outside_reads);
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

fn push_listing(lines: &mut Vec<String>, values: &[String]) {
    if values.is_empty() {
        lines.push("    none".to_string());
    } else {
        for value in values {
            lines.push(format!("    {value}"));
        }
    }
}

/// The evidence document for a set of results.
///
/// PORT4-7 states the shape. One trace stands per check
/// result per claimed surface entry, in result order then
/// surface order, holding the check name, the surface
/// entry, the word read when at least one path in the
/// result's read set matches that surface entry under
/// the surface rule and the word not read otherwise, and
/// the result's trace source text when a source stands
/// and the words not recorded otherwise. Every field is
/// nonempty text whenever the check's own fields are,
/// which is the acceptance shape tameshi's contract
/// states for blindgate-evidence/1.
pub fn evidence_document(results: &[CheckResult]) -> Json {
    let mut traces = Vec::new();
    for result in results {
        let detail = match &result.trace_source {
            Some(source) => source_text(source),
            None => "not recorded".to_string(),
        };
        for surface in &result.surface {
            let read = result
                .read_set
                .iter()
                .any(|path| crate::analysis::path_matches_surface(path, surface));
            traces.push(Json::Object(vec![
                ("check".to_string(), Json::Text(result.name.clone())),
                ("surface".to_string(), Json::Text(surface.clone())),
                (
                    "observation".to_string(),
                    Json::Text(if read { "read" } else { "not read" }.to_string()),
                ),
                ("detail".to_string(), Json::Text(detail.clone())),
            ]));
        }
    }
    Json::Object(vec![
        (
            "format".to_string(),
            Json::Text("blindgate-evidence/1".to_string()),
        ),
        ("traces".to_string(), Json::List(traces)),
    ])
}

/// Render the evidence document with the report's JSON spelling.
pub fn render_evidence(results: &[CheckResult]) -> String {
    dump_json(&evidence_document(results))
}

fn text_list(values: &[String]) -> Json {
    Json::List(
        values
            .iter()
            .map(|value| Json::Text(value.clone()))
            .collect(),
    )
}

/// A JSON value in the report's own spelling.
///
/// Objects hold their fields in insertion order and the
/// spelling sorts them, as dumps with sorted keys does.
/// Numbers stand only as the integer counts the summary
/// carries.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Int(i64),
    Text(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// Spell a JSON value the way CPython's dumps does.
///
/// The indent is two spaces, a key's colon is followed by
/// one space, empty collections stand inline, and there
/// is no trailing newline. Strings escape as CPython's
/// ensure ascii spelling escapes them, with unicode
/// escapes for everything outside the printable ASCII
/// band and surrogate pairs for astral characters.
pub fn dump_json(value: &Json) -> String {
    let mut out = String::new();
    write_json(value, 0, &mut out);
    out
}

fn write_json(value: &Json, level: usize, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Int(count) => out.push_str(&count.to_string()),
        Json::Text(text) => write_json_string(text, out),
        Json::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                push_indent(out, level + 1);
                write_json(item, level + 1, out);
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, level);
            out.push(']');
        }
        Json::Object(fields) => {
            if fields.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut sorted: Vec<&(String, Json)> = fields.iter().collect();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            out.push_str("{\n");
            for (index, (key, item)) in sorted.iter().enumerate() {
                push_indent(out, level + 1);
                write_json_string(key, out);
                out.push_str(": ");
                write_json(item, level + 1, out);
                if index + 1 < sorted.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, level);
            out.push('}');
        }
    }
}

fn push_indent(out: &mut String, level: usize) {
    for _ in 0..(level * 2) {
        out.push(' ');
    }
}

fn write_json_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            ch if ('\u{0020}'..='\u{007E}').contains(&ch) => out.push(ch),
            ch => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let rest = code - 0x10000;
                    let high = 0xD800 + (rest >> 10);
                    let low = 0xDC00 + (rest & 0x3FF);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(
        name: &str,
        verdict: &str,
        source: Option<TraceSource>,
        outside: Vec<String>,
    ) -> CheckResult {
        CheckResult {
            name: name.to_string(),
            job: "job".to_string(),
            surface: vec!["src/**".to_string()],
            read_set: vec!["src/app.py".to_string()],
            matched: vec!["src/app.py".to_string()],
            verdict: verdict.to_string(),
            trace_source: source,
            outside_reads: outside,
        }
    }

    #[test]
    fn source_text_labels_and_details() {
        let own = TraceSource {
            kind: "own_trace".to_string(),
            detail: "trace.log".to_string(),
        };
        assert_eq!(source_text(&own), "own trace file trace.log");
        let bare = TraceSource {
            kind: "shared_run".to_string(),
            detail: String::new(),
        };
        assert_eq!(source_text(&bare), "shared run");
        let unknown = TraceSource {
            kind: "mystery".to_string(),
            detail: "d".to_string(),
        };
        assert_eq!(source_text(&unknown), "mystery d");
    }

    #[test]
    fn renders_match_the_pythons_shape() {
        let results = vec![result("unit tests", "proven", None, Vec::new())];
        let text = render_text(&results);
        assert!(
            text.starts_with("blindgate report\n================\n\ncheck            unit tests\n")
        );
        assert!(text.ends_with("reads in surface\n    src/app.py\n"));
        let json = render_json(&results);
        assert!(json.contains("\"summary\": {\n    \"proven\": 1\n  }"));
        assert!(!json.contains("trace_source"));
        // A source anywhere turns the field on everywhere.
        let mixed = vec![
            result(
                "a",
                "proven",
                Some(TraceSource {
                    kind: "own_trace".to_string(),
                    detail: "t.log".to_string(),
                }),
                Vec::new(),
            ),
            result("b", "unproven", None, Vec::new()),
        ];
        let json = render_json(&mixed);
        assert!(json.contains("\"trace_source\": null"));
        let text = render_text(&mixed);
        assert!(text.contains("trace source     not recorded"));
        // The evidence document carries the stated shape.
        let evidence = render_evidence(&mixed);
        assert!(evidence.contains("\"format\": \"blindgate-evidence/1\""));
        assert!(evidence.contains("\"observation\": \"read\""));
        assert!(evidence.contains("\"detail\": \"not recorded\""));
    }

    #[test]
    fn json_spelling_escapes_as_cpython_does() {
        let value = Json::Object(vec![(
            "k".to_string(),
            Json::Text("caf\u{00E9} \u{1F600} \u{007F}".to_string()),
        )]);
        assert_eq!(
            dump_json(&value),
            "{\n  \"k\": \"caf\\u00e9 \\ud83d\\ude00 \\u007f\"\n}"
        );
        assert_eq!(dump_json(&Json::List(Vec::new())), "[]");
        assert_eq!(dump_json(&Json::Object(Vec::new())), "{}");
    }
}
