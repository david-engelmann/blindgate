//! The recorded CLI battery, PORT5-6.
//!
//! The cases in tests/conformance/cli_cases.json were recorded
//! from the Python at the Round 18 head by the generator in
//! scratch (probe_gen5.py) and are driven here against the
//! built binary as a process, on the standing Wave B pattern.
//! Completed runs and tool refusals compare stdout and stderr
//! byte for byte. Usage refusals record stderr as null, and
//! for those the harness pins the shape instead, the usage
//! block standing at the head of the binary's stderr with one
//! error line behind it, and the binary's own no argument
//! stderr pinned byte for byte against the usage constant
//! and the required line below. Run family cases record
//! stderr as null as well, because a run's process stderr
//! carries the traced command's own stderr and strace's
//! chatter, which no recording can pin.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use blindgate::{parse_json, Value};

/// The usage block the refusals open with, verbatim.
///
/// The same text the shell holds, pinned here so the battery
/// checks the binary's stderr against an independent copy.
const USAGE: &str = "usage: blindgate [-h] --workflow WORKFLOW --config CONFIG [--trace TRACE]\n                 [--trace-for JOB=PATH] [--run ...] [--json] [--root ROOT]\n";

/// The whole stderr of the binary run with no arguments.
const NO_ARGS_STDERR: &str = "usage: blindgate [-h] --workflow WORKFLOW --config CONFIG [--trace TRACE]\n                 [--trace-for JOB=PATH] [--run ...] [--json] [--root ROOT]\nblindgate: error: the following arguments are required: --workflow, --config\n";

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key)
}

fn int_of(value: &Value) -> i64 {
    match value {
        Value::Int(int) => {
            let digits: i64 = int.digits().parse().expect("an exit code is small");
            if int.is_negative() {
                -digits
            } else {
                digits
            }
        }
        _ => panic!("expected an integer value in the battery, found {value:?}"),
    }
}

fn text(value: &Value) -> &str {
    match value {
        Value::Text(text) => text,
        _ => panic!("expected a text value in the battery, found {value:?}"),
    }
}

fn cases() -> (Vec<Value>, String) {
    let raw = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("conformance")
            .join("cli_cases.json"),
    )
    .expect("the battery file reads");
    let document = parse_json(&raw).expect("the battery parses");
    let list = match field(&document, "cases") {
        Some(Value::List(list)) => list.clone(),
        _ => panic!("the battery holds a cases list"),
    };
    let capture = match field(&document, "run_capture") {
        Some(Value::Text(capture)) => capture.clone(),
        _ => panic!("the battery names its run capture state"),
    };
    (list, capture)
}

fn decode_base64(spelled: &str) -> Vec<u8> {
    let digits: Vec<Option<u8>> = spelled
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            b'=' => None,
            _ => panic!("the battery holds base64"),
        })
        .collect();
    let mut out = Vec::new();
    for group in digits.chunks(4) {
        let a = group[0].expect("a group opens with a digit");
        let b = group[1].expect("a group holds a second digit");
        out.push((a << 2) | (b >> 4));
        if let Some(c) = group[2] {
            out.push(((b & 0x0F) << 4) | (c >> 2));
            if let Some(d) = group[3] {
                out.push(((c & 0x03) << 6) | d);
            }
        }
    }
    out
}

/// Stand one case's fixture tree in a fresh directory.
fn stand_case(case: &Value) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!("blindgate-cli-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).expect("the case directory stands");
    if let Some(Value::Map(files)) = field(case, "files") {
        for (rel, content) in files {
            let rel = text(rel);
            let raw = decode_base64(text(field(content, "b64").expect("a file holds its bytes")));
            let path = if let Some(encoded) = rel.strip_prefix("b64:") {
                base.join(path_from_bytes(&decode_base64(encoded)))
            } else {
                base.join(rel)
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("the file's folder stands");
            }
            fs::write(&path, raw).expect("the fixture file writes");
        }
    }
    if let Some(Value::List(dirs)) = field(case, "dirs") {
        for dir in dirs {
            fs::create_dir_all(base.join(text(dir))).expect("the fixture folder stands");
        }
    }
    if let Some(Value::Map(links)) = field(case, "symlinks") {
        for (rel, target) in links {
            symlink(text(target), &base.join(text(rel))).expect("the fixture link stands");
        }
    }
    base
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()))
}

#[cfg(unix)]
fn symlink(target: &str, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_target: &str, _link: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Substitute the case directory into recorded output bytes.
///
/// Recordings spell the generator's case directory as the
/// placeholder {DIR} wherever a wording carries it, and the
/// harness puts its own case directory back before comparing.
fn substitute_dir(recorded: &[u8], base: &Path) -> Vec<u8> {
    let needle = b"{DIR}";
    let replacement = base.as_os_str().as_encoded_bytes();
    if recorded.len() < needle.len() {
        return recorded.to_vec();
    }
    let mut out = Vec::with_capacity(recorded.len());
    let mut at = 0usize;
    while at < recorded.len() {
        if recorded[at..].starts_with(needle) {
            out.extend_from_slice(replacement);
            at += needle.len();
        } else {
            out.push(recorded[at]);
            at += 1;
        }
    }
    out
}

/// The argv words of one case, with the case directory in.
fn argv_of(case: &Value, base: &Path) -> Vec<std::ffi::OsString> {
    let mut words = Vec::new();
    if let Some(Value::List(tokens)) = field(case, "argv") {
        for token in tokens {
            if let Some(spelled) = field(token, "text") {
                words.push(std::ffi::OsString::from(
                    text(spelled).replace("{DIR}", &base.to_string_lossy()),
                ));
            } else if let Some(spelled) = field(token, "b64") {
                words.push(word_from_bytes(&decode_base64(text(spelled))));
            }
        }
    }
    words
}

#[cfg(unix)]
fn word_from_bytes(bytes: &[u8]) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(bytes.to_vec())
}

#[cfg(not(unix))]
fn word_from_bytes(bytes: &[u8]) -> std::ffi::OsString {
    std::ffi::OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

/// Whether a strace capture can be written on this machine.
///
/// The probe runs strace itself over a trivial command in a
/// scratch directory and reads whether the capture file
/// holds anything, which is the exact write the run family
/// depends on.
fn capture_writable() -> bool {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!("blindgate-cli-probe-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    if fs::create_dir_all(&base).is_err() {
        return false;
    }
    let capture = base.join("probe.out");
    let ran = Command::new("strace")
        .args(["-f", "-e", "trace=openat", "-o"])
        .arg(&capture)
        .arg("/bin/true")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let writable = match ran {
        Ok(_) => fs::metadata(&capture)
            .map(|meta| meta.len() > 0)
            .unwrap_or(false),
        Err(_) => false,
    };
    let _ = fs::remove_dir_all(&base);
    writable
}

fn family_census(list: &[Value]) -> BTreeMap<String, usize> {
    let mut census = BTreeMap::new();
    for case in list {
        *census
            .entry(text(field(case, "family").expect("a case holds its family")).to_string())
            .or_insert(0) += 1;
    }
    census
}

#[test]
fn battery_census_by_family() {
    let (list, _) = cases();
    let census = family_census(&list);
    let expected: BTreeMap<String, usize> = [
        ("arguments".to_string(), 47),
        ("bytes".to_string(), 29),
        ("examples".to_string(), 6),
        ("refusals".to_string(), 19),
        ("remainder".to_string(), 12),
        ("run".to_string(), 10),
    ]
    .into_iter()
    .collect();
    assert_eq!(census, expected, "the battery census by family");
    assert_eq!(list.len(), 123, "the battery holds 123 cases");
}

#[test]
fn no_arguments_refusal_matches_the_pinned_stderr() {
    let output = Command::new(env!("CARGO_BIN_EXE_blindgate"))
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stderr, NO_ARGS_STDERR.as_bytes());
    assert!(output.stdout.is_empty());
}

#[test]
fn cli_battery() {
    let (list, capture_state) = cases();
    let binary = env!("CARGO_BIN_EXE_blindgate");
    let live_capture_possible = capture_writable();
    for (index, case) in list.iter().enumerate() {
        let name = text(field(case, "name").expect("a case holds its name"));
        let family = text(field(case, "family").expect("a case holds its family"));
        let base = stand_case(case);
        let output = Command::new(binary)
            .args(argv_of(case, &base))
            .current_dir(&base)
            .output()
            .expect("the binary runs");
        let expect = field(case, "expect").expect("a case holds its expectation");
        let exit = match field(expect, "exit") {
            Some(value) => int_of(value),
            _ => panic!("case {index} ({name}) holds an exit code"),
        };
        assert_eq!(
            output.status.code(),
            Some(exit as i32),
            "case {index} ({name}) exit"
        );
        let stdout = substitute_dir(
            &decode_base64(text(
                field(expect, "stdout_b64").expect("a case holds its stdout"),
            )),
            &base,
        );
        if family == "run" && capture_state == "empty" && live_capture_possible {
            // The recordings were made where no capture could
            // be written, and this machine can write one, so
            // the live read sets legitimately differ from the
            // recorded empty ones. The live pair stands owed,
            // as the generator's note records, and the exit
            // code above is what this machine can pin.
            eprintln!(
                "case {index} ({name}) stdout stands unpinned, a live capture is possible here"
            );
        } else {
            assert_eq!(output.stdout, stdout, "case {index} ({name}) stdout");
        }
        match field(expect, "stderr_b64") {
            Some(Value::Null) => {
                if family != "run" {
                    // A usage refusal. Its shape is pinned, the
                    // usage block at the head and one error
                    // line behind it.
                    assert!(
                        output.stderr.starts_with(USAGE.as_bytes()),
                        "case {index} ({name}) stderr opens with the usage block"
                    );
                    let tail = &output.stderr[USAGE.len()..];
                    assert!(
                        tail.starts_with(b"blindgate: error: ") && tail.ends_with(b"\n"),
                        "case {index} ({name}) stderr carries one error line"
                    );
                }
            }
            Some(recorded) => {
                let stderr = substitute_dir(&decode_base64(text(recorded)), &base);
                assert_eq!(output.stderr, stderr, "case {index} ({name}) stderr");
            }
            None => panic!("case {index} ({name}) holds a stderr field"),
        }
        let _ = fs::remove_dir_all(&base);
    }
}
