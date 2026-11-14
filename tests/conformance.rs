//! Conformance tests for Rust port slice 1.
//!
//! The recordings in tests/conformance/port1_cases.json were
//! produced by running the landed Python package over the case
//! inputs, and these tests compare the Rust answers against the
//! recordings at content level. Each case that needs a directory
//! tree recreates its recorded tree, symlink included, in a
//! fresh temporary directory and stands that directory in for
//! the recorded base in the inputs and in the answers alike.
//! The tests run offline and never call the Python.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use blindgate::{
    looks_like_strace, parse_strace_log, parse_trace, read_set_from_log_file,
    read_trace_from_log_file, refuse_unshaped_trace, run_traced,
};

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    List(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> JsonParser<'a> {
    fn new(text: &'a str) -> Self {
        JsonParser {
            bytes: text.as_bytes(),
            at: 0,
        }
    }

    fn parse(&mut self) -> Json {
        let value = self.value();
        self.ws();
        assert_eq!(self.at, self.bytes.len(), "trailing text after JSON value");
        value
    }

    fn ws(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn value(&mut self) -> Json {
        self.ws();
        match self.bytes[self.at] {
            b'{' => {
                self.at += 1;
                let mut map = BTreeMap::new();
                self.ws();
                if self.bytes[self.at] == b'}' {
                    self.at += 1;
                    return Json::Object(map);
                }
                loop {
                    let key = match self.value() {
                        Json::Text(key) => key,
                        other => panic!("object key is not a string: {other:?}"),
                    };
                    self.ws();
                    assert_eq!(self.bytes[self.at], b':');
                    self.at += 1;
                    let item = self.value();
                    map.insert(key, item);
                    self.ws();
                    match self.bytes[self.at] {
                        b',' => self.at += 1,
                        b'}' => {
                            self.at += 1;
                            return Json::Object(map);
                        }
                        other => panic!("unexpected byte in object: {other}"),
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.ws();
                if self.bytes[self.at] == b']' {
                    self.at += 1;
                    return Json::List(items);
                }
                loop {
                    items.push(self.value());
                    self.ws();
                    match self.bytes[self.at] {
                        b',' => self.at += 1,
                        b']' => {
                            self.at += 1;
                            return Json::List(items);
                        }
                        other => panic!("unexpected byte in list: {other}"),
                    }
                }
            }
            b'"' => Json::Text(self.string()),
            b'-' | b'0'..=b'9' => {
                let start = self.at;
                while self.at < self.bytes.len()
                    && matches!(
                        self.bytes[self.at],
                        b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'
                    )
                {
                    self.at += 1;
                }
                Json::Number(
                    std::str::from_utf8(&self.bytes[start..self.at])
                        .expect("number is ascii")
                        .to_string(),
                )
            }
            b't' => {
                self.literal("true");
                Json::Bool(true)
            }
            b'f' => {
                self.literal("false");
                Json::Bool(false)
            }
            b'n' => {
                self.literal("null");
                Json::Null
            }
            other => panic!("unexpected byte starting a value: {other}"),
        }
    }

    fn literal(&mut self, word: &str) {
        assert!(
            self.bytes[self.at..].starts_with(word.as_bytes()),
            "bad literal at {}",
            self.at
        );
        self.at += word.len();
    }

    fn hex4(&mut self) -> u32 {
        let hex = std::str::from_utf8(&self.bytes[self.at..self.at + 4])
            .expect("unicode escape is ascii");
        let code = u32::from_str_radix(hex, 16).expect("unicode escape");
        self.at += 4;
        code
    }

    fn string(&mut self) -> String {
        assert_eq!(self.bytes[self.at], b'"');
        self.at += 1;
        let mut out = String::new();
        loop {
            let byte = self.bytes[self.at];
            self.at += 1;
            match byte {
                b'"' => return out,
                b'\\' => {
                    let escaped = self.bytes[self.at];
                    self.at += 1;
                    match escaped {
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'\\' => out.push('\\'),
                        b'"' => out.push('"'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'u' => {
                            let first = self.hex4();
                            if (0xD800..0xDC00).contains(&first)
                                && self.bytes[self.at] == b'\\'
                                && self.bytes[self.at + 1] == b'u'
                            {
                                self.at += 2;
                                let second = self.hex4();
                                let combined =
                                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                                out.push(
                                    char::from_u32(combined).expect("unicode escape is a char"),
                                );
                            } else {
                                out.push(char::from_u32(first).expect("unicode escape is a char"));
                            }
                        }
                        other => panic!("bad escape byte: {other}"),
                    }
                }
                _ => {
                    let start = self.at - 1;
                    let mut end = self.at;
                    while end < self.bytes.len() && (self.bytes[end] & 0xC0) == 0x80 {
                        end += 1;
                    }
                    self.at = end;
                    out.push_str(
                        std::str::from_utf8(&self.bytes[start..end]).expect("utf8 in string"),
                    );
                }
            }
        }
    }
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(map) => map.get(key),
            _ => None,
        }
    }

    fn as_text(&self) -> &str {
        match self {
            Json::Text(text) => text,
            other => panic!("expected a string, got {other:?}"),
        }
    }

    fn as_list(&self) -> &[Json] {
        match self {
            Json::List(items) => items,
            other => panic!("expected a list, got {other:?}"),
        }
    }

    fn as_bool(&self) -> bool {
        match self {
            Json::Bool(value) => *value,
            other => panic!("expected a bool, got {other:?}"),
        }
    }

    fn text_list(&self) -> Vec<String> {
        self.as_list()
            .iter()
            .map(|item| item.as_text().to_string())
            .collect()
    }
}

fn recordings() -> Vec<Json> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("conformance")
        .join("port1_cases.json");
    let text = fs::read_to_string(&path).expect("the recordings file stands in the repo");
    let parsed = JsonParser::new(&text).parse();
    parsed
        .get("cases")
        .expect("the recordings carry cases")
        .as_list()
        .to_vec()
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn fresh_base() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("blindgate-conformance-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the case base");
    fs::canonicalize(&dir).expect("the case base resolves")
}

fn substitute(text: &str, base: &Path, root: &Path) -> String {
    text.replace("{ROOT}", &root.to_string_lossy())
        .replace("{BASE}", &base.to_string_lossy())
}

fn build_setup(base: &Path, setup: &[Json]) {
    for op in setup {
        if let Some(rel) = op.get("mkdir") {
            fs::create_dir_all(base.join(rel.as_text())).expect("mkdir op");
        } else if let Some(write) = op.get("write") {
            let items = write.as_list();
            let target = base.join(items[0].as_text());
            fs::create_dir_all(target.parent().expect("write op parent")).expect("write parent");
            fs::write(&target, items[1].as_text()).expect("write op");
        } else if let Some(link) = op.get("symlink") {
            let items = link.as_list();
            let target = items[0]
                .as_text()
                .replace("{BASE}", &base.to_string_lossy());
            let link_path = base.join(items[1].as_text());
            fs::create_dir_all(link_path.parent().expect("symlink parent"))
                .expect("symlink parent");
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &link_path).expect("symlink op");
        } else {
            panic!("unknown setup op {op:?}");
        }
    }
}

struct CaseRun {
    base: PathBuf,
    root: PathBuf,
    trace: String,
}

fn run_case(case: &Json) -> CaseRun {
    let base = fresh_base();
    let empty: Vec<Json> = Vec::new();
    let setup = case
        .get("setup")
        .map(|item| item.as_list())
        .unwrap_or(&empty);
    build_setup(&base, setup);
    let root_rel = case.get("root").map(|item| item.as_text()).unwrap_or("");
    let root = if root_rel.is_empty() {
        base.clone()
    } else {
        base.join(root_rel)
    };
    let trace = if let Some(file) = case.get("trace_file") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file.as_text());
        fs::read_to_string(&path).expect("the recorded trace file stands in the repo")
    } else {
        substitute(
            case.get("trace").expect("case trace").as_text(),
            &base,
            &root,
        )
    };
    CaseRun { base, root, trace }
}

fn expected(case: &Json, key: &str, run: &CaseRun) -> Vec<String> {
    case.get(key)
        .map(|item| {
            item.text_list()
                .iter()
                .map(|text| substitute(text, &run.base, &run.root))
                .collect()
        })
        .unwrap_or_default()
}

fn cases_of_kind(kind: &str) -> Vec<Json> {
    recordings()
        .into_iter()
        .filter(|case| case.get("kind").map(|item| item.as_text()) == Some(kind))
        .collect()
}

#[test]
fn port_read_cases_match_the_python_record() {
    let cases = cases_of_kind("parse");
    assert!(!cases.is_empty(), "the recordings hold parse cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text();
        let run = run_case(case);
        let result = parse_trace(&run.trace, &run.root);
        match case.get("expected_error") {
            Some(error) => {
                let err = result.expect_err(&format!("case {name} refuses"));
                assert_eq!(err.message(), error.as_text(), "case {name}");
            }
            None => {
                let read = result.unwrap_or_else(|err| panic!("case {name} errors: {err}"));
                assert_eq!(
                    read.reads,
                    expected(case, "expected_reads", &run),
                    "case {name} read set"
                );
                assert_eq!(
                    read.outside_reads,
                    expected(case, "expected_outside", &run),
                    "case {name} outside reads"
                );
            }
        }
        let _ = fs::remove_dir_all(&run.base);
    }
}

fn slice16_recordings() -> Vec<Json> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("conformance")
        .join("slice16_cases.json");
    let text = fs::read_to_string(&path).expect("the slice 16 recordings file stands in the repo");
    let parsed = JsonParser::new(&text).parse();
    parsed
        .get("cases")
        .expect("the recordings carry cases")
        .as_list()
        .to_vec()
}

#[test]
fn slice16_exit_state_cases_match_the_contract() {
    let cases: Vec<Json> = slice16_recordings()
        .into_iter()
        .filter(|case| case.get("kind").map(|item| item.as_text()) == Some("exit_state"))
        .collect();
    assert!(!cases.is_empty(), "the recordings hold exit state cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text();
        let run = run_case(case);
        let read = parse_trace(&run.trace, &run.root)
            .unwrap_or_else(|err| panic!("case {name} errors: {err}"));
        assert_eq!(
            read.reads,
            expected(case, "expected_reads", &run),
            "case {name} read set"
        );
        assert_eq!(
            read.outside_reads,
            expected(case, "expected_outside", &run),
            "case {name} outside reads"
        );
        let _ = fs::remove_dir_all(&run.base);
    }
}

#[test]
fn port_parse_strace_log_cases_match_the_python_record() {
    let cases = cases_of_kind("parse_strace_log");
    assert!(!cases.is_empty(), "the recordings hold slice 1 name cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text();
        let run = run_case(case);
        let result = parse_strace_log(&run.trace, &run.root);
        match case.get("expected_error") {
            Some(error) => {
                let err = result.expect_err(&format!("case {name} refuses"));
                assert_eq!(err.message(), error.as_text(), "case {name}");
            }
            None => {
                let reads = result.unwrap_or_else(|err| panic!("case {name} errors: {err}"));
                assert_eq!(
                    reads,
                    expected(case, "expected_reads", &run),
                    "case {name} read set"
                );
            }
        }
        let _ = fs::remove_dir_all(&run.base);
    }
}

#[test]
fn port_classification_cases_match_the_python_record() {
    let cases = cases_of_kind("classify");
    assert!(
        !cases.is_empty(),
        "the recordings hold classification cases"
    );
    for case in &cases {
        let name = case.get("name").unwrap().as_text();
        let run = run_case(case);
        let expected = case
            .get("expected_looks")
            .expect("classification expectation")
            .as_bool();
        assert_eq!(
            looks_like_strace(&run.trace),
            expected,
            "case {name} classification"
        );
        let _ = fs::remove_dir_all(&run.base);
    }
}

#[test]
fn port_refusal_cases_match_the_python_record() {
    let mut refused = 0;
    for case in recordings() {
        let Some(error) = case.get("expected_error") else {
            continue;
        };
        refused += 1;
        let name = case.get("name").unwrap().as_text();
        let run = run_case(&case);
        let err = refuse_unshaped_trace(&run.trace, "log")
            .expect_err(&format!("case {name} refuses at the shape reading"));
        assert_eq!(err.message(), error.as_text(), "case {name}");
        let _ = fs::remove_dir_all(&run.base);
    }
    assert!(refused > 0, "the recordings hold refusal cases");
}

// ---------------------------------------------------------------------
// Port slice 2 conformance: the trace state machinery.
//
// The recordings in tests/conformance/port2_cases.json were
// produced by probe_gen2.py in the Round 16 scratch, run
// against the landed Python package over case inputs drawn
// from the Python tests of the ported machinery. The two
// cases marked python_crash record the contract's stated
// limit: the Python raises uncaught on a return token its
// base zero conversion refuses, and the recorded expectation
// is the sanctioned answer the contract states for the port,
// the decimal the token spells read as a success. Every other
// case compares the Rust answers against the Python record
// at content level, reads and outside reads alike.

fn recordings2() -> Vec<Json> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("conformance")
        .join("port2_cases.json");
    let text = fs::read_to_string(&path).expect("the slice 2 recordings stand in the repo");
    let parsed = JsonParser::new(&text).parse();
    parsed
        .get("cases")
        .expect("the slice 2 recordings carry cases")
        .as_list()
        .to_vec()
}

fn cases_of_kind2(kind: &str) -> Vec<Json> {
    recordings2()
        .into_iter()
        .filter(|case| case.get("kind").map(|item| item.as_text()) == Some(kind))
        .collect()
}

fn assert_port2_case(case: &Json) {
    let name = case.get("name").unwrap().as_text();
    assert!(
        case.get("python_error").is_none(),
        "case {name} records a Python error the slice does not sanction: {:?}",
        case.get("python_error")
    );
    let run = run_case(case);
    let result = parse_trace(&run.trace, &run.root);
    let read = result.unwrap_or_else(|err| panic!("case {name} errors: {err}"));
    assert_eq!(
        read.reads,
        expected(case, "expected_reads", &run),
        "case {name} read set"
    );
    assert_eq!(
        read.outside_reads,
        expected(case, "expected_outside", &run),
        "case {name} outside reads"
    );
    let _ = fs::remove_dir_all(&run.base);
}

fn run_port2_kind(kind: &str) {
    let cases = cases_of_kind2(kind);
    assert!(
        !cases.is_empty(),
        "the slice 2 recordings hold {kind} cases"
    );
    for case in &cases {
        assert_port2_case(case);
    }
}

#[test]
fn port_directory_state_cases_match_the_python_record() {
    run_port2_kind("directory");
}

#[test]
fn port_descriptor_state_cases_match_the_python_record() {
    run_port2_kind("descriptor");
}

#[test]
fn port_dup_and_fcntl_cases_match_the_python_record() {
    run_port2_kind("dup_fcntl");
}

#[test]
fn port_fork_sharing_cases_match_the_python_record() {
    run_port2_kind("fork");
}

#[test]
fn port_exec_closing_cases_match_the_python_record() {
    run_port2_kind("exec");
}

#[test]
fn port_split_line_cases_match_the_python_record() {
    run_port2_kind("split");
}

// ---------------------------------------------------------------------
// Port slice 3 conformance: the file reading entries and the
// live capture.
//
// The recordings in tests/conformance/port3_cases.json were
// produced by probe_gen3.py in the Round 17 scratch, run
// against the landed Python package. The byte cases carry the
// owner probe record's byte cases as hex and are read through
// read_set_from_log_file, so the recorded read set pins the
// replacement decoding the parse downstream actually saw. The
// file cases read whole texts from files through
// read_trace_from_log_file. The error cases pin the file
// entry's refusal wordings. The permission denied case is
// marked constructed, with the provenance PORT3-3 states: a
// capture running as root reads through mode bits, so no real
// denial can be driven through the entry on this machine, and
// the case is checked against the stated template at the
// test's own base while the entries unit tests pin the port's
// wording construction to the same template. The live case
// runs the port's own capture against the recorded command
// and compares the distilled pair, guarded on strace
// availability as the annex states.

fn recordings3() -> Vec<Json> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("conformance")
        .join("port3_cases.json");
    let text = fs::read_to_string(&path).expect("the slice 3 recordings stand in the repo");
    let parsed = JsonParser::new(&text).parse();
    parsed
        .get("cases")
        .expect("the slice 3 recordings carry cases")
        .as_list()
        .to_vec()
}

fn cases_of_kind3(kind: &str) -> Vec<Json> {
    recordings3()
        .into_iter()
        .filter(|case| case.get("kind").map(|item| item.as_text()) == Some(kind))
        .collect()
}

fn hex_bytes(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    assert!(
        bytes.len().is_multiple_of(2),
        "a hex recording has even length"
    );
    (0..bytes.len())
        .step_by(2)
        .map(|at| {
            let pair = std::str::from_utf8(&bytes[at..at + 2]).expect("a hex pair is ascii");
            u8::from_str_radix(pair, 16).expect("a hex pair parses")
        })
        .collect()
}

struct FileCase {
    base: PathBuf,
    root: PathBuf,
    log: PathBuf,
    run: CaseRun,
}

fn stand_file_case(case: &Json) -> FileCase {
    let base = fresh_base();
    let empty: Vec<Json> = Vec::new();
    let setup = case
        .get("setup")
        .map(|item| item.as_list())
        .unwrap_or(&empty);
    build_setup(&base, setup);
    let root_rel = case.get("root").map(|item| item.as_text()).unwrap_or("");
    let root = if root_rel.is_empty() {
        base.clone()
    } else {
        base.join(root_rel)
    };
    let log = base.join("handed.log");
    let run = CaseRun {
        base: base.clone(),
        root: root.clone(),
        trace: String::new(),
    };
    FileCase {
        base,
        root,
        log,
        run,
    }
}

#[test]
fn port_replacement_decoding_cases_match_the_python_record() {
    let cases = cases_of_kind3("bytes");
    assert!(!cases.is_empty(), "the slice 3 recordings hold byte cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text();
        let stood = stand_file_case(case);
        let data = hex_bytes(case.get("log_hex").expect("case log hex").as_text());
        fs::write(&stood.log, &data).expect("write the case log");
        let result = read_set_from_log_file(&stood.log, &stood.root);
        match case.get("expected_error") {
            Some(error) => {
                let err = result.expect_err(&format!("case {name} refuses"));
                assert_eq!(err.message(), error.as_text(), "case {name}");
            }
            None => {
                let reads = result.unwrap_or_else(|err| panic!("case {name} errors: {err}"));
                assert_eq!(
                    reads,
                    expected(case, "expected_reads", &stood.run),
                    "case {name} read set"
                );
            }
        }
        let _ = fs::remove_dir_all(&stood.base);
    }
}

#[test]
fn port_file_reading_cases_match_the_python_record() {
    let cases = cases_of_kind3("file");
    assert!(!cases.is_empty(), "the slice 3 recordings hold file cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text();
        let stood = stand_file_case(case);
        let text = substitute(
            case.get("log_text").expect("case log text").as_text(),
            &stood.base,
            &stood.root,
        );
        fs::write(&stood.log, text).expect("write the case log");
        let result = read_trace_from_log_file(&stood.log, &stood.root);
        match case.get("expected_error") {
            Some(error) => {
                let err = result.expect_err(&format!("case {name} refuses"));
                assert_eq!(err.message(), error.as_text(), "case {name}");
            }
            None => {
                let read = result.unwrap_or_else(|err| panic!("case {name} errors: {err}"));
                assert_eq!(
                    read.reads,
                    expected(case, "expected_reads", &stood.run),
                    "case {name} read set"
                );
                assert_eq!(
                    read.outside_reads,
                    expected(case, "expected_outside", &stood.run),
                    "case {name} outside reads"
                );
                let reads = read_set_from_log_file(&stood.log, &stood.root)
                    .unwrap_or_else(|err| panic!("case {name} reads entry errors: {err}"));
                assert_eq!(
                    reads,
                    expected(case, "expected_reads", &stood.run),
                    "case {name} read set at the reads entry"
                );
            }
        }
        let _ = fs::remove_dir_all(&stood.base);
    }
}

#[test]
fn port_file_error_wording_cases_match_the_python_record() {
    let cases = cases_of_kind3("error");
    assert!(!cases.is_empty(), "the slice 3 recordings hold error cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text();
        let stood = stand_file_case(case);
        let log_rel = case.get("log_rel").expect("case log path").as_text();
        let log = if log_rel.is_empty() {
            stood.base.clone()
        } else {
            stood.base.join(log_rel)
        };
        let want = substitute(
            case.get("expected_error").expect("case wording").as_text(),
            &stood.base,
            &stood.root,
        );
        let constructed = case
            .get("constructed")
            .map(|item| item.as_bool())
            .unwrap_or(false);
        if constructed {
            // This arm checks the recording against the stated
            // template only, and it does not drive the port
            // entry. The port's own construction is pinned by
            // the entries unit test over from_raw_os_error(13),
            // so no reader is invited to believe the entry was
            // driven here.
            let spelled = log.to_string_lossy().into_owned();
            assert_eq!(
                want,
                format!(
                    "cannot read trace log {spelled} ([Errno 13] Permission denied: '{spelled}')"
                ),
                "case {name} recorded wording matches the stated template"
            );
        } else {
            let err = read_trace_from_log_file(&log, &stood.root)
                .expect_err(&format!("case {name} refuses"));
            assert_eq!(err.message(), want, "case {name}");
            let err = read_set_from_log_file(&log, &stood.root)
                .expect_err(&format!("case {name} refuses at the reads entry"));
            assert_eq!(err.message(), want, "case {name} reads entry");
        }
        let _ = fs::remove_dir_all(&stood.base);
    }
}

#[test]
fn port_live_capture_pair_matches_the_python_record() {
    let cases = cases_of_kind3("live");
    assert!(!cases.is_empty(), "the slice 3 recordings hold live cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text();
        let stood = stand_file_case(case);
        let command: Vec<std::ffi::OsString> = case
            .get("command")
            .expect("case command")
            .text_list()
            .iter()
            .map(|part| std::ffi::OsString::from(substitute(part, &stood.base, &stood.root)))
            .collect();
        let read = match run_traced(&command, &stood.root) {
            Ok(read) => read,
            Err(err)
                if err.message()
                    == "strace is not available on this machine, so no read set can be captured" =>
            {
                let _ = fs::remove_dir_all(&stood.base);
                continue;
            }
            Err(err) => panic!("case {name} errors: {err}"),
        };
        assert_eq!(
            read.reads,
            expected(case, "expected_reads", &stood.run),
            "case {name} read set"
        );
        assert_eq!(
            read.outside_reads,
            expected(case, "expected_outside", &stood.run),
            "case {name} outside reads"
        );
        let leftovers: Vec<String> = fs::read_dir(&stood.root)
            .expect("read the capture root")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|file| file.starts_with(".blindgate-trace-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "case {name} leaves trace temp files behind: {leftovers:?}"
        );
        let _ = fs::remove_dir_all(&stood.base);
    }
}

// ---------------------------------------------------------------------
// Port slice 4: the judgement stack and the evidence format (PORT4).
//
// The recordings in tests/conformance/port4_cases.json were
// produced by running the landed Python package over the case
// inputs, except the evidence documents, which PORT4-7 states
// have no Python emitter and which the generator computed from
// the contract text and checked against the acceptance shape
// the consumer's contract states. These tests compare the
// Rust answers against the recordings and never call the
// Python.

use blindgate::value::float_repr;
use blindgate::{
    analyse, analyse_judged, analyse_with_outside, load_config, load_workflow_jobs, parse_json,
    parse_yaml, path_matches_surface, render_evidence, render_json, render_text, unknown_jobs,
    Check, CheckResult, Judgement, JudgementPart, TraceSource, Value,
};

fn recordings4() -> Vec<Json> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("conformance")
        .join("port4_cases.json");
    let text = fs::read_to_string(&path).expect("the slice 4 recordings file stands in the repo");
    let parsed = JsonParser::new(&text).parse();
    parsed
        .get("cases")
        .expect("the recordings carry cases")
        .as_list()
        .to_vec()
}

fn cases_of_kind4(kind: &str) -> Vec<Json> {
    recordings4()
        .into_iter()
        .filter(|case| case.get("kind").map(|item| item.as_text()) == Some(kind))
        .collect()
}

fn json_object(pairs: Vec<(&str, Json)>) -> Json {
    let mut map = BTreeMap::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Json::Object(map)
}

fn json_usize(value: &Json) -> usize {
    match value {
        Json::Number(text) => text.parse().expect("the recorded number is a count"),
        other => panic!("expected a number, got {other:?}"),
    }
}

fn substitute4(text: &str, base: &Path) -> String {
    text.replace("{BASE}", &base.to_string_lossy())
}

fn build_setup4(base: &Path, setup: &[Json]) {
    for op in setup {
        if let Some(rel) = op.get("mkdir") {
            fs::create_dir_all(base.join(rel.as_text())).expect("mkdir op");
        } else if let Some(write) = op.get("write") {
            let items = write.as_list();
            let target = base.join(items[0].as_text());
            fs::create_dir_all(target.parent().expect("write op parent")).expect("write parent");
            fs::write(&target, items[1].as_text()).expect("write op");
        } else if let Some(hexed) = op.get("write_bytes_hex") {
            let items = hexed.as_list();
            let target = base.join(items[0].as_text());
            fs::create_dir_all(target.parent().expect("write op parent")).expect("write parent");
            let digits = items[1].as_text();
            let mut bytes = Vec::new();
            let mut chars = digits.chars();
            while let (Some(high), Some(low)) = (chars.next(), chars.next()) {
                let pair: String = [high, low].iter().collect();
                bytes.push(u8::from_str_radix(&pair, 16).expect("the recorded bytes are hex"));
            }
            fs::write(&target, bytes).expect("write bytes op");
        } else if let Some(link) = op.get("symlink") {
            let items = link.as_list();
            let target = items[0]
                .as_text()
                .replace("{BASE}", &base.to_string_lossy());
            let link_path = base.join(items[1].as_text());
            fs::create_dir_all(link_path.parent().expect("symlink parent"))
                .expect("symlink parent");
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &link_path).expect("symlink op");
        } else {
            panic!("unknown setup op {op:?}");
        }
    }
}

fn stand4(case: &Json) -> PathBuf {
    let base = fresh_base();
    let empty: Vec<Json> = Vec::new();
    let setup = case
        .get("setup")
        .map(|item| item.as_list())
        .unwrap_or(&empty);
    build_setup4(&base, setup);
    base
}

/// Encode a port value in the recordings' tagged shape, the
/// shape the generator's encode_value writes.
fn encode_port_value(value: &Value) -> Json {
    match value {
        Value::Null => json_object(vec![("t", Json::Text("null".to_string()))]),
        Value::Bool(flag) => json_object(vec![
            ("t", Json::Text("bool".to_string())),
            ("v", Json::Bool(*flag)),
        ]),
        Value::Int(number) => json_object(vec![
            ("t", Json::Text("int".to_string())),
            ("v", Json::Text(number.to_string())),
        ]),
        Value::Float(number) => json_object(vec![
            ("t", Json::Text("float".to_string())),
            ("v", Json::Text(float_repr(*number))),
        ]),
        Value::Text(text) => json_object(vec![
            ("t", Json::Text("text".to_string())),
            ("v", Json::Text(text.clone())),
        ]),
        Value::Timestamp(text) => json_object(vec![
            ("t", Json::Text("timestamp".to_string())),
            ("v", Json::Text(text.clone())),
        ]),
        Value::List(items) => json_object(vec![
            ("t", Json::Text("list".to_string())),
            (
                "v",
                Json::List(items.iter().map(encode_port_value).collect()),
            ),
        ]),
        Value::Map(pairs) => json_object(vec![
            ("t", Json::Text("map".to_string())),
            (
                "v",
                Json::List(
                    pairs
                        .iter()
                        .map(|(key, item)| {
                            Json::List(vec![encode_port_value(key), encode_port_value(item)])
                        })
                        .collect(),
                ),
            ),
        ]),
    }
}

fn build_check4(encoded: &Json) -> Check {
    Check {
        name: encoded.get("name").unwrap().as_text().to_string(),
        job: encoded.get("job").unwrap().as_text().to_string(),
        surface: encoded.get("surface").unwrap().text_list(),
        trace: match encoded.get("trace") {
            Some(Json::Text(text)) => Some(text.clone()),
            _ => None,
        },
        trace_path: None,
    }
}

fn build_checks4(case: &Json) -> Vec<Check> {
    case.get("checks")
        .unwrap()
        .as_list()
        .iter()
        .map(build_check4)
        .collect()
}

fn build_source4(encoded: &Json) -> Option<TraceSource> {
    match encoded {
        Json::Object(_) => Some(TraceSource {
            kind: encoded.get("kind").unwrap().as_text().to_string(),
            detail: encoded.get("detail").unwrap().as_text().to_string(),
        }),
        _ => None,
    }
}

fn build_judgement4(encoded: &Json) -> Judgement {
    Judgement::new(
        encoded.get("read_set").unwrap().text_list(),
        build_source4(encoded.get("source").unwrap()),
        encoded.get("outside").unwrap().text_list(),
    )
}

fn build_results4(case: &Json) -> Vec<CheckResult> {
    let checks = build_checks4(case);
    match case.get("mode").unwrap().as_text() {
        "analyse" => analyse(&checks, &case.get("read_set").unwrap().text_list()),
        "analyse_with_outside" => analyse_with_outside(
            &checks,
            &case.get("read_set").unwrap().text_list(),
            &case.get("outside").unwrap().text_list(),
        ),
        "analyse_judged" => {
            let judgements: Vec<Judgement> = case
                .get("judgements")
                .unwrap()
                .as_list()
                .iter()
                .map(build_judgement4)
                .collect();
            analyse_judged(&checks, &judgements).expect("the recorded judgements stand")
        }
        "none" => Vec::new(),
        other => panic!("unknown results mode {other}"),
    }
}

fn result_from_encoding(encoded: &Json) -> CheckResult {
    CheckResult {
        name: encoded.get("name").unwrap().as_text().to_string(),
        job: encoded.get("job").unwrap().as_text().to_string(),
        surface: encoded.get("surface").unwrap().text_list(),
        read_set: encoded.get("read_set").unwrap().text_list(),
        matched: encoded.get("matched").unwrap().text_list(),
        verdict: encoded.get("verdict").unwrap().as_text().to_string(),
        trace_source: build_source4(encoded.get("trace_source").unwrap()),
        outside_reads: encoded.get("outside_reads").unwrap().text_list(),
    }
}

fn assert_result4(port: &CheckResult, encoded: &Json, name: &str) {
    assert_eq!(
        port,
        &result_from_encoding(encoded),
        "case {name} result differs"
    );
}

/// The comparison every file case refusal runs. A case with
/// an expected error compares the port's words exactly, a
/// case with a port prefix holds the Python's own error or
/// crash beside the prefix the port's refusal must stand
/// under, and anything else is a success case.
enum Refusal {
    Exact(String),
    Prefix(String),
    Success,
}

fn refusal4(case: &Json, base: &Path) -> Refusal {
    if let Some(expected) = case.get("expected_error") {
        Refusal::Exact(substitute4(expected.as_text(), base))
    } else if let Some(prefix) = case.get("port_prefix") {
        Refusal::Prefix(substitute4(prefix.as_text(), base))
    } else {
        Refusal::Success
    }
}

fn assert_refusal4(message: &str, refusal: &Refusal, name: &str) {
    match refusal {
        Refusal::Exact(expected) => {
            assert_eq!(message, expected, "case {name} refusal words differ")
        }
        Refusal::Prefix(prefix) => {
            assert!(
                message.starts_with(prefix.as_str()) && message.ends_with(')'),
                "case {name} refusal {message:?} does not stand under the recorded prefix"
            )
        }
        Refusal::Success => panic!("case {name} was recorded as a success and refused"),
    }
}

#[test]
fn port_config_cases_match_the_python_record() {
    let cases = cases_of_kind4("config");
    assert!(!cases.is_empty(), "the recordings hold config cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text().to_string();
        let base = stand4(case);
        let path = base.join(case.get("config_rel").unwrap().as_text());
        let refusal = refusal4(case, &base);
        match load_config(&path) {
            Err(error) => assert_refusal4(error.message(), &refusal, &name),
            Ok(checks) => {
                if let Refusal::Exact(_) | Refusal::Prefix(_) = refusal {
                    panic!("case {name} was recorded as a refusal and loaded");
                }
                let expected = case.get("expected_checks").unwrap().as_list();
                assert_eq!(checks.len(), expected.len(), "case {name} check count");
                for (port, recorded) in checks.iter().zip(expected.iter()) {
                    assert_eq!(
                        port.name,
                        recorded.get("name").unwrap().as_text(),
                        "case {name}"
                    );
                    assert_eq!(
                        port.job,
                        recorded.get("job").unwrap().as_text(),
                        "case {name}"
                    );
                    assert_eq!(
                        port.surface,
                        recorded.get("surface").unwrap().text_list(),
                        "case {name}"
                    );
                    let recorded_trace = match recorded.get("trace") {
                        Some(Json::Text(text)) => Some(text.clone()),
                        _ => None,
                    };
                    assert_eq!(port.trace, recorded_trace, "case {name} trace");
                    let recorded_path = match recorded.get("trace_path") {
                        Some(Json::Text(text)) => Some(substitute4(text, &base)),
                        _ => None,
                    };
                    let port_path = port
                        .trace_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned());
                    assert_eq!(port_path, recorded_path, "case {name} trace path");
                }
                if let Some(workflow_rel) = case.get("workflow_rel") {
                    let jobs = load_workflow_jobs(&base.join(workflow_rel.as_text()))
                        .expect("the recorded workflow loads");
                    let unknown: Vec<String> = unknown_jobs(&checks, &jobs)
                        .iter()
                        .map(|check| check.name.clone())
                        .collect();
                    assert_eq!(
                        unknown,
                        case.get("expected_unknown").unwrap().text_list(),
                        "case {name} unknown jobs"
                    );
                }
            }
        }
    }
}

#[test]
fn port_workflow_cases_match_the_python_record() {
    let cases = cases_of_kind4("workflow");
    assert!(!cases.is_empty(), "the recordings hold workflow cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text().to_string();
        let base = stand4(case);
        let path = base.join(case.get("workflow_rel").unwrap().as_text());
        let refusal = refusal4(case, &base);
        match load_workflow_jobs(&path) {
            Err(error) => assert_refusal4(error.message(), &refusal, &name),
            Ok(jobs) => {
                if let Refusal::Exact(_) | Refusal::Prefix(_) = refusal {
                    panic!("case {name} was recorded as a refusal and loaded");
                }
                let port: Vec<String> = jobs.iter().cloned().collect();
                assert_eq!(
                    port,
                    case.get("expected_jobs").unwrap().text_list(),
                    "case {name} jobs"
                );
            }
        }
    }
}

fn value_depth(value: &Value) -> usize {
    match value {
        Value::Map(pairs) => {
            1 + pairs
                .iter()
                .map(|(_, item)| value_depth(item))
                .max()
                .unwrap_or(0)
        }
        _ => 0,
    }
}

#[test]
fn port_yaml_value_cases_match_the_python_record() {
    let mut cases = cases_of_kind4("yaml");
    cases.extend(cases_of_kind4("yaml_scalar"));
    assert!(!cases.is_empty(), "the recordings hold yaml cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text().to_string();
        let input = case.get("input").unwrap().as_text();
        let result = parse_yaml(input);
        if case.get("python_error").is_some() {
            assert!(result.is_err(), "case {name} refuses in the port too");
        } else if case
            .get("port_refuses")
            .map(|item| item.as_bool())
            .unwrap_or(false)
        {
            assert!(result.is_err(), "case {name} refuses in the port");
        } else {
            let value = result.unwrap_or_else(|err| panic!("case {name} errors: {err}"));
            assert_eq!(
                encode_port_value(&value),
                *case.get("expected_value").unwrap(),
                "case {name} value differs"
            );
        }
    }
    let depth_cases = cases_of_kind4("yaml_depth");
    assert!(!depth_cases.is_empty(), "the recordings hold depth cases");
    for case in &depth_cases {
        let name = case.get("name").unwrap().as_text().to_string();
        // The case records its depth, and the nested text
        // is rebuilt from it, one k key per level with a
        // leaf at the bottom, as the generator built it.
        let depth = json_usize(case.get("depth").expect("case depth"));
        let mut input = String::new();
        for level in 0..depth {
            input.push_str(&"  ".repeat(level));
            input.push_str("k:\n");
        }
        input.push_str(&"  ".repeat(depth));
        input.push_str("leaf\n");
        let result = parse_yaml(&input);
        let port_refuses = case
            .get("port_refuses")
            .map(|item| item.as_bool())
            .unwrap_or(false);
        if port_refuses {
            assert!(result.is_err(), "case {name} refuses in the port");
        } else {
            let value = result.unwrap_or_else(|err| panic!("case {name} errors: {err}"));
            assert_eq!(value_depth(&value), depth, "case {name} nesting depth");
        }
    }
}

#[test]
fn port_analysis_cases_match_the_python_record() {
    let matches = cases_of_kind4("match");
    assert!(!matches.is_empty(), "the recordings hold match cases");
    for case in &matches {
        let name = case.get("name").unwrap().as_text();
        assert_eq!(
            path_matches_surface(
                case.get("path").unwrap().as_text(),
                case.get("pattern").unwrap().as_text(),
            ),
            case.get("expected").unwrap().as_bool(),
            "case {name} surface match"
        );
    }
    let analyses = cases_of_kind4("analysis");
    assert!(!analyses.is_empty(), "the recordings hold analysis cases");
    for case in &analyses {
        let name = case.get("name").unwrap().as_text().to_string();
        let results = build_results4(case);
        let expected = case.get("expected_results").unwrap().as_list();
        assert_eq!(results.len(), expected.len(), "case {name} result count");
        for (port, recorded) in results.iter().zip(expected.iter()) {
            assert_result4(port, recorded, &name);
        }
    }
    let guards = cases_of_kind4("guard");
    assert!(!guards.is_empty(), "the recordings hold guard cases");
    for case in &guards {
        let name = case.get("name").unwrap().as_text().to_string();
        match case.get("which").unwrap().as_text() {
            "parallel" => {
                let checks = build_checks4(case);
                let judgements: Vec<Judgement> = case
                    .get("judgements")
                    .unwrap()
                    .as_list()
                    .iter()
                    .map(build_judgement4)
                    .collect();
                let error =
                    analyse_judged(&checks, &judgements).expect_err("the parallel guard fires");
                assert_eq!(
                    error.message(),
                    case.get("expected_error").unwrap().as_text(),
                    "case {name} guard words"
                );
            }
            "triple" => {
                // The Python raises this guard's words
                // itself on a judgement that is not a
                // triple. In the port a malformed triple
                // cannot be a Judgement value, so the
                // guard stands at the parts constructor,
                // and the words are the Python's own.
                let error =
                    Judgement::from_parts(vec![JudgementPart::ReadSet(vec!["a".to_string()])])
                        .expect_err("the triple guard fires");
                assert_eq!(
                    error.message(),
                    case.get("expected_error").unwrap().as_text(),
                    "case {name} guard words"
                );
            }
            other => panic!("unknown guard {other}"),
        }
    }
}

#[test]
fn port_report_cases_match_the_python_record() {
    let cases = cases_of_kind4("report");
    assert!(!cases.is_empty(), "the recordings hold report cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text().to_string();
        let results = build_results4(case);
        assert_eq!(
            render_text(&results),
            case.get("expected_text").unwrap().as_text(),
            "case {name} text render"
        );
        assert_eq!(
            render_json(&results),
            case.get("expected_json").unwrap().as_text(),
            "case {name} json render"
        );
    }
}

/// The acceptance shape the evidence consumer's contract
/// states, checked over a document the port's own JSON
/// reader parsed. The format name stands, the traces list
/// carries one object per trace, and each object holds a
/// check and a surface and an observation and a detail,
/// all nonempty text.
fn evidence_shape_stands(text: &str) {
    let document = parse_json(text).expect("the evidence document parses");
    assert_eq!(
        document.get("format").and_then(|value| value.as_text()),
        Some("blindgate-evidence/1"),
        "the format name stands"
    );
    let traces = document
        .get("traces")
        .and_then(|value| value.as_list())
        .expect("the traces stand as a list");
    for trace in traces {
        for field in ["check", "surface", "observation", "detail"] {
            let value = trace.get(field).and_then(|item| item.as_text());
            assert!(
                matches!(value, Some(text) if !text.is_empty()),
                "a trace's {field} is nonempty text"
            );
        }
    }
}

#[test]
fn port_evidence_documents_match_the_stated_format() {
    let cases = cases_of_kind4("evidence");
    assert!(!cases.is_empty(), "the recordings hold evidence cases");
    for case in &cases {
        let name = case.get("name").unwrap().as_text().to_string();
        let results = build_results4(case);
        let rendered = render_evidence(&results);
        assert_eq!(
            rendered,
            case.get("expected_document").unwrap().as_text(),
            "case {name} evidence document"
        );
        evidence_shape_stands(&rendered);
    }
    let witnesses = cases_of_kind4("witness");
    assert!(!witnesses.is_empty(), "the recordings hold witnesses");
    for case in &witnesses {
        let name = case.get("name").unwrap().as_text().to_string();
        let document = case.get("document").unwrap().as_text();
        evidence_shape_stands(document);
        let duplicated = document.matches("\"format\"").count() > 1;
        assert_eq!(
            duplicated,
            case.get("raw_duplicate_format_key").unwrap().as_bool(),
            "case {name} duplicate format key standing"
        );
    }
}
