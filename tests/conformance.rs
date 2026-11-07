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

use blindgate::{looks_like_strace, parse_strace_log, parse_trace, refuse_unshaped_trace};

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
                .replace("{BASE}", &base.to_string_lossy().into_owned());
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
