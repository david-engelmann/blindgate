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
                            let hex = std::str::from_utf8(&self.bytes[self.at..self.at + 4])
                                .expect("unicode escape is ascii");
                            let code = u32::from_str_radix(hex, 16).expect("unicode escape");
                            self.at += 4;
                            out.push(char::from_u32(code).expect("unicode escape is a char"));
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
        let command: Vec<String> = case
            .get("command")
            .expect("case command")
            .text_list()
            .iter()
            .map(|part| substitute(part, &stood.base, &stood.root))
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
