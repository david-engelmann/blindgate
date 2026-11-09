//! The blindgate command line, the full shell PORT5 states.
//!
//! The shell reads the process arguments as bytes and scans
//! them under the argparse model the probe record pins, then
//! composes the library in the Python main's order, the root
//! resolved first, the workflow and the config loaded, the
//! trace-for overrides parsed against the resolved config
//! folder, one judgement built per check with the shared
//! source lazy and a handed over trace read eagerly, and the
//! report shape chosen by whether any check carried its own
//! trace and whether the shared triple named outside reads.
//! A completed report prints with exactly one trailing
//! newline past the render and exits 0, and every refusal
//! exits 2, the argparse refusals as the usage block and one
//! error line on stderr and the tool refusals as one error
//! line carrying the library's words.
//!
//! The byte discipline PORT5-5 states runs through the scan
//! and the composition. Argument words reach the filesystem
//! and the traced subprocess as the bytes typed, through
//! OsString paths and command words end to end. Where a word
//! must stand inside a text, the shell decodes it the way
//! CPython's surrogateescape decoding does, and the decoded
//! form records beside each character whether it stands for
//! itself or for one stray byte, so no later pass ever has
//! to guess which characters are bytes. Each surface then
//! spells the decoded form the way the Python's own handlers
//! spell the string it holds. The refusal streams decode
//! with backslashreplace, so a stray byte stands as its lone
//! surrogate escape and every real character stands as
//! itself. The report stream decodes with surrogateescape,
//! so a stray byte stands raw in the text report, and the
//! JSON writer spells a stray byte as the same lone
//! surrogate escape. A name holding a real character from
//! the private use block the stray spellings borrow stands
//! as itself on every surface, because the decoded form
//! never confuses a character with a byte.
//!
//! The library spells an argument path inside its refusal
//! wordings through a lossy rendering, which turns a byte
//! that forms no valid UTF-8 into the replacement character
//! and loses which byte stood there. The shell repairs those
//! wordings at the boundary from the paths it handed over,
//! putting the decoded path back in the outer wording under
//! the refusal stream's spelling and the CPython repr
//! spelling, lone surrogates escaped, back inside the
//! operating system error, which is the standing PORT5-5
//! split between the outer wording and the inner spelling.

use std::env;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use blindgate::entries::resolve_root;
use blindgate::{
    analyse, analyse_judged, analyse_with_outside, load_config, load_workflow_jobs,
    read_trace_from_log_file, run_traced, unknown_jobs, BlindgateError, Check, CheckResult,
    Judgement, TraceSource, SOURCE_OWN_TRACE, SOURCE_SHARED_RUN, SOURCE_SHARED_TRACE,
};

// The generated CPython printable table, shared with the
// library through this include so the shell's repr spells
// from the same ranges the library's repr spells from.
include!("repr_table.rs");

/// The usage block every argparse refusal prints.
///
/// The text stands verbatim from the probe record, two
/// lines, 149 bytes with both newlines. The probe record
/// and PORT5-1 name the block as 172 bytes, and the bytes
/// the Python prints measure 149, which the unit tests in
/// this file pin, so the text stands and the recorded count
/// is the part that is off.
const USAGE: &str = "usage: blindgate [-h] --workflow WORKFLOW --config CONFIG [--trace TRACE]\n                 [--trace-for JOB=PATH] [--run ...] [--json] [--root ROOT]\n";

/// The help text, printed to stdout with exit 0.
///
/// The text stands verbatim from the probe record, the
/// usage block and then the description and the options
/// listing, 866 bytes whole.
const HELP: &str = "usage: blindgate [-h] --workflow WORKFLOW --config CONFIG [--trace TRACE]\n                 [--trace-for JOB=PATH] [--run ...] [--json] [--root ROOT]\n\nIt traces what a gate actually opened while it ran and diffs that against the\nsurface the gate claims to certify.\n\noptions:\n  -h, --help            show this help message and exit\n  --workflow WORKFLOW   GitHub Actions workflow file\n  --config CONFIG       config file declaring the checks\n  --trace TRACE         an existing strace openat log to analyse\n  --trace-for JOB=PATH  a per job strace log that overrides the config trace\n                        for that job, repeatable\n  --run ...             a command to run under strace and analyse\n  --json                render the report as JSON\n  --root ROOT           repo root the read set is scoped to, defaults to the\n                        working directory\n";

/// The first placeholder character of the byte scheme.
///
/// A byte from 0x80 through 0xFF that forms no valid UTF-8
/// sequence stands inside a decoded word as the character
/// at this base plus the byte's offset above 0x80.
const PLACEHOLDER_BASE: u32 = 0xE000;

/// A decoded argument word, with its stray bytes marked.
///
/// The text holds one placeholder character from the
/// private use block U+E000 through U+E07F for each stray
/// byte, the byte's own offset above 0x80 carried in the
/// character, and the marks record beside each character
/// whether it stands for itself or for a stray byte. A real
/// character from that block carries no mark, so a character
/// and a byte never stand for each other on any surface.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WordText {
    text: String,
    strays: Vec<bool>,
}

impl WordText {
    /// The refusal stream spelling of the decoded word.
    ///
    /// A stray byte stands as its lone surrogate escape and
    /// every real character stands as itself, which is the
    /// backslashreplace spelling the Python's refusal
    /// streams produce over the surrogateescape string.
    fn stderr_spelling(&self) -> String {
        let mut out = String::with_capacity(self.text.len());
        for (ch, stray) in self.text.chars().zip(self.strays.iter()) {
            if *stray {
                out.push_str(&format!("\\udc{:02x}", stray_byte(ch)));
            } else {
                out.push(ch);
            }
        }
        out
    }

    /// The CPython repr spelling of the decoded word.
    ///
    /// A stray byte stands as its lone surrogate escape,
    /// which is how repr spells the surrogate the byte
    /// decodes to, and every real character stands under
    /// the repr rules the shell's cpython_repr applies.
    fn repr_spelling(&self) -> String {
        let quote = repr_quote(&self.text);
        let mut out = String::with_capacity(self.text.len() + 2);
        out.push(quote);
        for (ch, stray) in self.text.chars().zip(self.strays.iter()) {
            if *stray {
                out.push_str(&format!("\\udc{:02x}", stray_byte(ch)));
            } else {
                push_repr_char(&mut out, ch, quote);
            }
        }
        out.push(quote);
        out
    }

    /// The JSON content spelling of the decoded word.
    ///
    /// The spelling follows the report writer's own rules
    /// for the characters of a string, with a stray byte
    /// standing as its lone surrogate escape, which is how
    /// CPython's json spells the surrogate the byte decodes
    /// to. The surrounding quotes are the writer's, and
    /// this spells only the content between them.
    fn json_spelling(&self) -> String {
        let mut out = String::with_capacity(self.text.len());
        for (ch, stray) in self.text.chars().zip(self.strays.iter()) {
            if *stray {
                out.push_str(&format!("\\udc{:02x}", stray_byte(ch)));
            } else {
                push_json_char(&mut out, ch);
            }
        }
        out
    }

    /// Whether the decoded word is exactly this library text.
    ///
    /// A word holding a stray byte never matches, because
    /// the library text holds real characters where the
    /// word holds bytes the typist never decoded.
    fn matches(&self, other: &str) -> bool {
        self.strays.iter().all(|stray| !stray) && self.text == other
    }

    /// Join decoded words with one separator character.
    fn join(words: &[WordText], separator: char) -> WordText {
        let mut text = String::new();
        let mut strays: Vec<bool> = Vec::new();
        for (index, word) in words.iter().enumerate() {
            if index > 0 {
                text.push(separator);
                strays.push(false);
            }
            text.push_str(&word.text);
            strays.extend(word.strays.iter().copied());
        }
        WordText { text, strays }
    }
}

/// The placeholder character standing for one stray byte.
fn placeholder_for(byte: u8) -> char {
    char::from_u32(PLACEHOLDER_BASE + (byte as u32 - 0x80)).expect("the placeholder block is valid")
}

/// The byte a marked placeholder character stands for.
fn stray_byte(ch: char) -> u8 {
    (ch as u32 - PLACEHOLDER_BASE) as u8 + 0x80
}

/// Decode argument bytes the way surrogateescape decodes.
///
/// A well formed UTF-8 sequence decodes to its scalar, and
/// each byte that forms no valid sequence stands as its
/// placeholder character with its stray mark set, one per
/// byte, so the decoding is lossless and the marks say
/// which characters stand for bytes.
fn decode_word(bytes: &[u8]) -> WordText {
    let mut text = String::with_capacity(bytes.len());
    let mut strays: Vec<bool> = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let starter = bytes[at];
        if starter < 0x80 {
            text.push(starter as char);
            strays.push(false);
            at += 1;
            continue;
        }
        let continuations = match starter {
            0xC2..=0xDF => 1,
            0xE0..=0xEF => 2,
            0xF0..=0xF4 => 3,
            _ => 0,
        };
        if continuations > 0 && at + continuations < bytes.len() {
            if let Ok(valid) = std::str::from_utf8(&bytes[at..=at + continuations]) {
                text.push_str(valid);
                strays.push(false);
                at += continuations + 1;
                continue;
            }
        }
        text.push(placeholder_for(starter));
        strays.push(true);
        at += 1;
    }
    WordText { text, strays }
}

/// Append one real character under the JSON string rules.
///
/// The rules are the report writer's own rules for the
/// characters of a string, the quote and the backslash
/// escaped, the whitespace controls as their short escapes,
/// the printable ASCII band standing raw, and every other
/// character as its unicode escape, with a surrogate pair
/// for a character above the basic multilingual plane.
fn push_json_char(out: &mut String, ch: char) {
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

/// The bytes of one process argument word.
#[cfg(unix)]
fn word_bytes(word: &OsString) -> Vec<u8> {
    word.as_os_str().as_bytes().to_vec()
}

/// The bytes of one process argument word.
#[cfg(not(unix))]
fn word_bytes(word: &OsString) -> Vec<u8> {
    word.to_string_lossy().into_owned().into_bytes()
}

/// A path built from raw argument bytes.
#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

/// A path built from raw argument bytes.
#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// An argument word built from raw bytes.
#[cfg(unix)]
fn word_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}

/// An argument word built from raw bytes.
#[cfg(not(unix))]
fn word_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

/// The quote CPython repr wraps a string in.
///
/// The single quote wraps by default, and the double quote
/// wraps when the string holds a single quote and no double
/// quote.
fn repr_quote(text: &str) -> char {
    if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    }
}

/// Append one real character under the repr rules.
///
/// The rules are the library's own cpython_repr rules, the
/// chosen quote and backslashes escaped, the whitespace
/// controls as their escape spellings, and any other
/// character CPython counts as nonprintable as an escape
/// under the generated table in repr_table, a hexadecimal
/// escape with two digits below U+0100, a unicode escape
/// with four digits below U+10000, and a long unicode
/// escape with eight digits above that.
fn push_repr_char(out: &mut String, ch: char, quote: char) {
    match ch {
        '\\' => out.push_str("\\\\"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        ch if ch == quote => {
            out.push('\\');
            out.push(ch);
        }
        ch if !cpython_printable(ch) => {
            let code = ch as u32;
            if code < 0x100 {
                out.push_str(&format!("\\x{code:02x}"));
            } else if code < 0x10000 {
                out.push_str(&format!("\\u{code:04x}"));
            } else {
                out.push_str(&format!("\\U{code:08x}"));
            }
        }
        ch => out.push(ch),
    }
}

/// Spell a name the way CPython repr spells a string.
///
/// The name stands as real characters throughout, and each
/// character goes through the repr rules above under the
/// quote the whole string chooses.
fn cpython_repr(text: &str) -> String {
    let quote = repr_quote(text);
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        push_repr_char(&mut out, ch, quote);
    }
    out.push(quote);
    out
}

/// Normalize a path the way the library's reading entries do.
///
/// Dot components drop, duplicate separators collapse, parent
/// components stand as spelled, a trailing slash drops, and
/// the empty path reads as a single dot. The library applies
/// the same conversion before it reads or spells a path, so
/// the shell's repair works over the same converted form.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// Replace every standing of one byte string with another.
fn replace_bytes(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut at = 0usize;
    while at < haystack.len() {
        if haystack[at..].starts_with(needle) {
            out.extend_from_slice(replacement);
            at += needle.len();
        } else {
            out.push(haystack[at]);
            at += 1;
        }
    }
    out
}

/// The repair record for one path the shell handed over.
///
/// The library spells the path lossily inside its refusal
/// wordings, and the record holds the lossy spelling, the
/// repr of that spelling as it stands inside the operating
/// system error half of the wording, the repr the Python
/// spells over the surrogateescape decoding of the true
/// path, and the outer spelling the refusal stream's
/// backslashreplace handler gives that same decoding, so
/// the boundary pass can put each half back the way
/// PORT5-5 states, the outer wording carrying the decoded
/// path spelled and the inner error carrying its repr.
struct PathRepair {
    lossy: String,
    lossy_repr: String,
    true_repr: String,
    outer_spelled: String,
}

impl PathRepair {
    /// Build the record for a path in raw argument bytes.
    fn for_bytes(bytes: &[u8]) -> PathRepair {
        let normalized = normalize_path(&path_from_bytes(bytes));
        let raw = word_bytes(&normalized.as_os_str().to_os_string());
        let lossy = String::from_utf8_lossy(&raw).into_owned();
        let lossy_repr = cpython_repr(&lossy);
        let decoded = decode_word(&raw);
        let true_repr = decoded.repr_spelling();
        let outer_spelled = decoded.stderr_spelling();
        PathRepair {
            lossy,
            lossy_repr,
            true_repr,
            outer_spelled,
        }
    }

    /// Build the record for a path the library holds.
    fn for_path(path: &Path) -> PathRepair {
        PathRepair::for_bytes(word_bytes(&path.as_os_str().to_os_string()).as_slice())
    }

    /// Whether this path's lossy spelling loses any byte.
    fn bites(&self) -> bool {
        self.lossy.contains('\u{FFFD}')
    }

    /// Repair one library refusal message at the boundary.
    ///
    /// A record that bites nowhere leaves the message
    /// standing. Otherwise the repr half is repaired first,
    /// while the lossy spelling still stands inside it to be
    /// found whole, and then the outer standings of the lossy
    /// spelling become the spelled decoded path.
    fn apply(&self, message: &[u8]) -> Vec<u8> {
        if !self.bites() {
            return message.to_vec();
        }
        let inner = replace_bytes(
            message,
            self.lossy_repr.as_bytes(),
            self.true_repr.as_bytes(),
        );
        replace_bytes(&inner, self.lossy.as_bytes(), self.outer_spelled.as_bytes())
    }
}

/// Repair one library message against every handed over path.
fn repair_message(message: &str, repairs: &[PathRepair]) -> Vec<u8> {
    let mut bytes = message.as_bytes().to_vec();
    for repair in repairs {
        bytes = repair.apply(&bytes);
    }
    bytes
}

/// The stderr bytes of a tool refusal with no named owner.
///
/// The line is the error prefix and the message and one
/// newline. A message the shell built itself stands with
/// its shell held words already spelled for the refusal
/// stream by the word spelling that built the line, and a
/// library message passes through the path repair the
/// caller applies before this point.
fn plain_refusal(message: &str) -> Vec<u8> {
    let mut out = b"error, ".to_vec();
    out.extend_from_slice(message.as_bytes());
    out.push(b'\n');
    out
}

/// The stderr bytes of a library refusal, repaired.
///
/// The wording may carry an owner prefix the shell built,
/// which stands ahead of the library's words with a comma
/// and a space, exactly as the Python's read wrapper builds
/// it.
fn library_refusal(owner: Option<&str>, error: &BlindgateError, repairs: &[PathRepair]) -> Vec<u8> {
    let mut out = b"error, ".to_vec();
    if let Some(owner) = owner {
        out.extend_from_slice(owner.as_bytes());
        out.extend_from_slice(b", ");
    }
    out.extend_from_slice(&repair_message(error.message(), repairs));
    out.push(b'\n');
    out
}

/// The stderr bytes of an argparse refusal.
///
/// The usage block stands first, then the error line built
/// from the same usage constant's program name, then one
/// newline. The line stands with its shell held words
/// already spelled for the refusal stream by the word
/// spelling that built it.
fn argparse_refusal(line: &str) -> Vec<u8> {
    let mut out = USAGE.as_bytes().to_vec();
    out.extend_from_slice(b"blindgate: error: ");
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
    out
}

/// One long flag of the parser, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Long {
    Help,
    Workflow,
    Config,
    Trace,
    TraceFor,
    Run,
    Json,
    Root,
}

/// The long flags in the parser's declaration order.
///
/// The help flag stands first because argparse adds it
/// ahead of the declared flags, and the ambiguity wordings
/// name their matches in this order.
const LONGS: [(&str, Long); 8] = [
    ("help", Long::Help),
    ("workflow", Long::Workflow),
    ("config", Long::Config),
    ("trace", Long::Trace),
    ("trace-for", Long::TraceFor),
    ("run", Long::Run),
    ("json", Long::Json),
    ("root", Long::Root),
];

/// The long name of one flag, as the parser spells it.
fn long_name(flag: Long) -> &'static str {
    LONGS
        .iter()
        .find(|(_, candidate)| *candidate == flag)
        .map(|(name, _)| *name)
        .expect("every flag stands in the table")
}

/// How a long name part resolves against the table.
enum Resolution {
    /// No long flag matches, so the token is an unknown.
    Unknown,
    /// Exactly one long flag matches.
    One(Long),
    /// Two or more long flags match, named in order.
    Ambiguous(Vec<&'static str>),
}

/// Resolve a long name part, exact match first.
///
/// An exact match wins even when it is a prefix of another
/// flag's name, and otherwise every flag the part is a
/// prefix of matches, in declaration order.
fn resolve_long(name: &[u8]) -> Resolution {
    for (spelled, flag) in LONGS {
        if spelled.as_bytes() == name {
            return Resolution::One(flag);
        }
    }
    let matches: Vec<&'static str> = LONGS
        .iter()
        .filter(|(spelled, _)| spelled.as_bytes().starts_with(name))
        .map(|(spelled, _)| *spelled)
        .collect();
    match matches.len() {
        0 => Resolution::Unknown,
        1 => Resolution::One(
            LONGS
                .iter()
                .find(|(spelled, _)| *spelled == matches[0])
                .map(|(_, flag)| *flag)
                .expect("a match stands in the table"),
        ),
        _ => Resolution::Ambiguous(matches),
    }
}

/// Whether a word reads as a negative number.
///
/// A dash and then digits with an optional fraction, or a
/// dash and a dot and then digits, the whole word and
/// nothing else, as the standing Wave B shells read it.
fn is_negative_number(bytes: &[u8]) -> bool {
    if bytes.first() != Some(&b'-') {
        return false;
    }
    let rest = &bytes[1..];
    let digits = |run: &[u8]| !run.is_empty() && run.iter().all(|byte| byte.is_ascii_digit());
    if let Some(after_dot) = rest.strip_prefix(b".") {
        return digits(after_dot);
    }
    let whole: usize = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if whole == 0 {
        return false;
    }
    if whole == rest.len() {
        return true;
    }
    if rest[whole] == b'.' {
        return digits(&rest[whole + 1..]);
    }
    false
}

/// Whether a dash led word segments into an option.
///
/// A long word segments when the part before its first
/// equals sign, or the whole word when no equals sign
/// stands, is a prefix of a long flag's name, the whole
/// name included. A single dash word segments when its
/// second byte is h, the only short flag, whatever follows
/// it. A word that segments is read by argparse as an
/// option standing in the value's place, never as a value.
fn segments_into_option(bytes: &[u8]) -> bool {
    if bytes.len() > 2 && bytes.starts_with(b"--") {
        let body = &bytes[2..];
        let name = match body.iter().position(|byte| *byte == b'=') {
            Some(position) => &body[..position],
            None => body,
        };
        LONGS
            .iter()
            .any(|(spelled, _)| spelled.as_bytes().starts_with(name))
    } else if bytes.len() > 1 {
        bytes[1] == b'h'
    } else {
        false
    }
}

/// Whether a word standing after a valued flag counts as its value.
///
/// A word that opens with a dash is no value, unless it is
/// the lone dash, or it segments into no option and reads
/// as a negative number, or it segments into no option and
/// holds a space, the one byte U+0020, anywhere inside it,
/// and the empty word counts as a value, as argparse reads
/// them. A tab or any other whitespace inside a dash led
/// word does not make it a value.
fn is_value_like(bytes: &[u8]) -> bool {
    match bytes.first() {
        None => true,
        Some(b'-') => {
            bytes.len() == 1
                || (!segments_into_option(bytes)
                    && (is_negative_number(bytes) || bytes.contains(&b' ')))
        }
        Some(_) => true,
    }
}

/// The values the scan collects.
#[derive(Default)]
struct Scan {
    workflow: Option<Vec<u8>>,
    config: Option<Vec<u8>>,
    trace: Option<Vec<u8>>,
    trace_for: Vec<Vec<u8>>,
    run: Option<Vec<Vec<u8>>>,
    json: bool,
    root: Option<Vec<u8>>,
}

/// What the scan decides.
enum ScanOutcome {
    /// The parse stands, and the composition runs.
    Ready(Scan),
    /// The help text prints and the process exits 0.
    Help,
    /// An argparse refusal line, printed behind the usage.
    Refusal(String),
}

/// Find the first ambiguous long spelling on the line.
///
/// The Python's parser classifies every token before it
/// consumes any of them, and the classification stops at
/// the first bare separator, so an ambiguous spelling
/// refuses ahead of every other event on the line wherever
/// it stands, as a flag, as a value, or as a remainder word.
/// A token names a candidate when its long name part, the
/// token up to its first equals sign, is a prefix of two or
/// more long flags and stands as no flag's exact name. A
/// single dash token can never be ambiguous, because the
/// help flag is the only short flag on the line.
fn prescan_ambiguity(tokens: &[Vec<u8>]) -> Option<ScanOutcome> {
    for token in tokens {
        if token == b"--" {
            break;
        }
        if token.len() <= 2 || !token.starts_with(b"--") {
            continue;
        }
        let body = &token[2..];
        let name = match body.iter().position(|byte| *byte == b'=') {
            Some(position) => &body[..position],
            None => body,
        };
        if LONGS.iter().any(|(spelled, _)| spelled.as_bytes() == name) {
            continue;
        }
        let matches: Vec<&'static str> = LONGS
            .iter()
            .filter(|(spelled, _)| spelled.as_bytes().starts_with(name))
            .map(|(spelled, _)| *spelled)
            .collect();
        if matches.len() > 1 {
            let named = matches
                .iter()
                .map(|spelled| format!("--{spelled}"))
                .collect::<Vec<String>>()
                .join(", ");
            return Some(ScanOutcome::Refusal(format!(
                "ambiguous option: {} could match {}",
                decode_word(token).stderr_spelling(),
                named
            )));
        }
    }
    None
}

/// Scan the argument tokens under the probed argparse model.
///
/// The scan runs in the two phases the Python's parser
/// runs. The classification pass stands first, and an
/// ambiguous long spelling refuses there, ahead of every
/// other event on the line. Every later event fires in
/// scan order as the consumption reaches it. The help
/// answers the moment its token is consumed, a missing
/// value refuses at its flag, and an ignored explicit
/// argument refuses at its flag. Unknown tokens are
/// collected and answered last, behind the required check,
/// which names the missing required flags in declaration
/// order. The bare separator is consumed where it stands,
/// and every later token is an unknown, with the separator
/// itself named first among the poisoned tail when any tail
/// stands, and named at its slot when the tail stands empty
/// and the required check lets the line stand. The spaced run flag collects the words up to
/// the first bare separator, and the separator and every
/// word behind it stand as unknowns in their places. The
/// equals run takes exactly its equals word, and the scan
/// continues behind it.
fn scan(tokens: &[Vec<u8>]) -> ScanOutcome {
    if let Some(outcome) = prescan_ambiguity(tokens) {
        return outcome;
    }
    let mut scan = Scan::default();
    let mut unknowns: Vec<Vec<u8>> = Vec::new();
    let mut after_separator = false;
    let mut separator_slot: Option<usize> = None;
    let mut index = 0usize;

    // Collect one unknown token, first planting the bare
    // separator ahead of the first token of its poisoned
    // tail, at the slot the separator stood in.
    fn note_unknown(unknowns: &mut Vec<Vec<u8>>, separator_slot: &mut Option<usize>, token: &[u8]) {
        if let Some(slot) = separator_slot.take() {
            unknowns.insert(slot, b"--".to_vec());
        }
        unknowns.push(token.to_vec());
    }

    while index < tokens.len() {
        let token = &tokens[index];
        if after_separator {
            note_unknown(&mut unknowns, &mut separator_slot, token);
            index += 1;
            continue;
        }
        if token == b"--" {
            after_separator = true;
            separator_slot = Some(unknowns.len());
            index += 1;
            continue;
        }
        if token.len() > 2 && token.starts_with(b"--") {
            let body = &token[2..];
            let (name, explicit) = match body.iter().position(|byte| *byte == b'=') {
                Some(position) => (&body[..position], Some(body[position + 1..].to_vec())),
                None => (body, None),
            };
            if name.is_empty() {
                note_unknown(&mut unknowns, &mut separator_slot, token);
                index += 1;
                continue;
            }
            match resolve_long(name) {
                Resolution::Unknown => {
                    note_unknown(&mut unknowns, &mut separator_slot, token);
                    index += 1;
                    continue;
                }
                Resolution::Ambiguous(matches) => {
                    let named = matches
                        .iter()
                        .map(|spelled| format!("--{spelled}"))
                        .collect::<Vec<String>>()
                        .join(", ");
                    return ScanOutcome::Refusal(format!(
                        "ambiguous option: {} could match {}",
                        decode_word(token).stderr_spelling(),
                        named
                    ));
                }
                Resolution::One(Long::Help) => {
                    if let Some(value) = &explicit {
                        return ScanOutcome::Refusal(format!(
                            "argument -h/--help: ignored explicit argument {}",
                            decode_word(value).repr_spelling()
                        ));
                    }
                    return ScanOutcome::Help;
                }
                Resolution::One(Long::Json) => {
                    if let Some(value) = &explicit {
                        return ScanOutcome::Refusal(format!(
                            "argument --json: ignored explicit argument {}",
                            decode_word(value).repr_spelling()
                        ));
                    }
                    scan.json = true;
                    index += 1;
                    continue;
                }
                Resolution::One(Long::Run) => {
                    match &explicit {
                        // The equals form takes exactly its
                        // equals word, and the scan continues
                        // behind it as it does behind any flag.
                        Some(value) => {
                            scan.run = Some(vec![value.clone()]);
                            index += 1;
                            continue;
                        }
                        None => {
                            let rest = &tokens[index + 1..];
                            match rest.iter().position(|word| word == b"--") {
                                Some(0) => {
                                    note_unknown(&mut unknowns, &mut separator_slot, b"--");
                                    for tail in &rest[1..] {
                                        note_unknown(&mut unknowns, &mut separator_slot, tail);
                                    }
                                }
                                Some(at) => {
                                    scan.run = Some(rest[..at].to_vec());
                                    note_unknown(&mut unknowns, &mut separator_slot, b"--");
                                    for tail in &rest[at + 1..] {
                                        note_unknown(&mut unknowns, &mut separator_slot, tail);
                                    }
                                }
                                None => {
                                    scan.run = Some(rest.to_vec());
                                }
                            }
                            index = tokens.len();
                            continue;
                        }
                    }
                }
                Resolution::One(flag) => {
                    let value = match &explicit {
                        Some(value) => value.clone(),
                        None => match tokens.get(index + 1) {
                            Some(next) if is_value_like(next) => {
                                index += 1;
                                next.clone()
                            }
                            _ => {
                                return ScanOutcome::Refusal(format!(
                                    "argument --{}: expected one argument",
                                    long_name(flag)
                                ));
                            }
                        },
                    };
                    match flag {
                        Long::Workflow => scan.workflow = Some(value),
                        Long::Config => scan.config = Some(value),
                        Long::Trace => scan.trace = Some(value),
                        Long::TraceFor => scan.trace_for.push(value),
                        Long::Root => scan.root = Some(value),
                        _ => unreachable!("the valued flags are the ones left"),
                    }
                    index += 1;
                    continue;
                }
            }
        }
        if token.len() > 1 && token[0] == b'-' {
            if token[1] == b'h' {
                let rest = &token[2..];
                if rest.is_empty() {
                    return ScanOutcome::Help;
                } else if rest[0] == b'=' {
                    return ScanOutcome::Refusal(format!(
                        "argument -h/--help: ignored explicit argument {}",
                        decode_word(&rest[1..]).repr_spelling()
                    ));
                } else if rest[0] == b'-' {
                    return ScanOutcome::Refusal(format!(
                        "argument -h/--help: ignored explicit argument {}",
                        decode_word(rest).repr_spelling()
                    ));
                } else {
                    return ScanOutcome::Help;
                }
            }
            note_unknown(&mut unknowns, &mut separator_slot, token);
            index += 1;
            continue;
        }
        note_unknown(&mut unknowns, &mut separator_slot, token);
        index += 1;
    }

    // A separator whose tail stands empty is still named
    // by the unrecognized report, at the slot it stood in.
    // The required check below answers first when a
    // required flag is missing, so the naming never
    // reaches past it.
    if let Some(slot) = separator_slot.take() {
        unknowns.insert(slot, b"--".to_vec());
    }

    let mut missing: Vec<&'static str> = Vec::new();
    if scan.workflow.is_none() {
        missing.push("--workflow");
    }
    if scan.config.is_none() {
        missing.push("--config");
    }
    if !missing.is_empty() {
        return ScanOutcome::Refusal(format!(
            "the following arguments are required: {}",
            missing.join(", ")
        ));
    }
    if !unknowns.is_empty() {
        let named = unknowns
            .iter()
            .map(|token| decode_word(token).stderr_spelling())
            .collect::<Vec<String>>()
            .join(" ");
        return ScanOutcome::Refusal(format!("unrecognized arguments: {named}"));
    }
    ScanOutcome::Ready(scan)
}

/// One trace-for override, its display text and its read path.
///
/// The display is the path half as given, held as the
/// shell's decoded word with its stray marks standing, so
/// every surface spells it from the marks, and the read
/// path is the path half as raw bytes resolved against the
/// config file's folder when it is relative.
struct Override {
    display: WordText,
    display_raw: Vec<u8>,
    read_path: PathBuf,
}

/// Parse the trace-for overrides in the Python's order.
///
/// Each value partitions on its first equals sign, and a
/// value with no equals sign, an empty job, or an empty path
/// refuses in the standing wording, and a job no check runs
/// refuses ahead of the repeated job refusal, exactly as
/// the Python orders them. The refusals are shell built
/// lines, returned ready for the plain refusal shape.
fn parse_trace_for(
    values: &[Vec<u8>],
    checks: &[Check],
    config_dir: &Path,
) -> Result<Vec<(WordText, Override)>, Vec<u8>> {
    let mut overrides: Vec<(WordText, Override)> = Vec::new();
    for value in values {
        let partition = value.iter().position(|byte| *byte == b'=');
        let (job_bytes, path_bytes) = match partition {
            Some(position) => (&value[..position], &value[position + 1..]),
            None => (&value[..], &[][..]),
        };
        if partition.is_none() || job_bytes.is_empty() || path_bytes.is_empty() {
            return Err(plain_refusal(&format!(
                "trace-for value {} is not written as a job id, an equals sign, and a path",
                decode_word(value).stderr_spelling()
            )));
        }
        let job = decode_word(job_bytes);
        if !checks.iter().any(|check| job.matches(&check.job)) {
            return Err(plain_refusal(&format!(
                "trace-for names job {}, which no check in the config runs",
                job.stderr_spelling()
            )));
        }
        if overrides.iter().any(|(held, _)| *held == job) {
            return Err(plain_refusal(&format!(
                "trace-for names job {} more than once",
                job.stderr_spelling()
            )));
        }
        let read_path = if path_bytes.first() == Some(&b'/') {
            path_from_bytes(path_bytes)
        } else {
            config_dir.join(path_from_bytes(path_bytes))
        };
        overrides.push((
            job,
            Override {
                display: decode_word(path_bytes),
                display_raw: path_bytes.to_vec(),
                read_path,
            },
        ));
    }
    Ok(overrides)
}

/// The shared triple a check with no trace of its own uses.
///
/// The read set, the source kind, the source detail as the
/// shell's decoded word with the raw bytes it stands for,
/// and the outside reads, built at most once by the
/// composition and shared by every check that falls back
/// to it.
struct Shared {
    read_set: Vec<String>,
    kind: String,
    detail: WordText,
    raw_detail: Vec<u8>,
    outside_reads: Vec<String>,
}

/// Build the shared triple from the trace or the run.
///
/// A handed over trace is read from its file, and otherwise
/// the run words reach the capture as the bytes typed. The
/// Python's shared source drops one leading separator word
/// from the run before it captures, and the port drops it
/// here, ahead of the capture and ahead of the joining of
/// the detail. Only an equals run can deliver that word,
/// because a spaced remainder ends at its separator
/// instead. The source details are shell held texts, the
/// trace value as given and the run words joined by single
/// spaces.
fn build_shared(
    trace: Option<&[u8]>,
    run: Option<&[Vec<u8>]>,
    root: &Path,
) -> Result<Shared, BlindgateError> {
    match trace {
        Some(bytes) => {
            let read = read_trace_from_log_file(&path_from_bytes(bytes), root)?;
            Ok(Shared {
                read_set: read.reads,
                kind: SOURCE_SHARED_TRACE.to_string(),
                detail: decode_word(bytes),
                raw_detail: bytes.to_vec(),
                outside_reads: read.outside_reads,
            })
        }
        None => {
            let mut words: Vec<OsString> = run
                .unwrap_or(&[])
                .iter()
                .map(|word| word_from_bytes(word))
                .collect();
            if words
                .first()
                .map(|word| word_bytes(word) == b"--")
                .unwrap_or(false)
            {
                words.remove(0);
            }
            let read = run_traced(&words, root)?;
            let detail = WordText::join(
                &words
                    .iter()
                    .map(|word| decode_word(&word_bytes(word)))
                    .collect::<Vec<WordText>>(),
                ' ',
            );
            let raw_detail = words
                .iter()
                .map(word_bytes)
                .collect::<Vec<Vec<u8>>>()
                .join(&[b' '][..]);
            Ok(Shared {
                read_set: read.reads,
                kind: SOURCE_SHARED_RUN.to_string(),
                detail,
                raw_detail,
                outside_reads: read.outside_reads,
            })
        }
    }
}

/// One shell held detail standing inside the results.
///
/// The composition plants a sentinel string as the detail
/// of every judgement whose detail the shell holds, and
/// this record keeps the decoded word and the raw bytes
/// the detail stands for, so the rendering can put the
/// detail back under each surface's own spelling once the
/// library has rendered everything else.
struct DetailSub {
    sentinel: String,
    word: WordText,
    raw: Vec<u8>,
}

/// Render the results with the shell held details restored.
///
/// The library renders the results whole, with each shell
/// held detail standing as its sentinel. A sentinel the
/// rendered text carries more often than its carriers
/// explain is colliding with a real name in the report, so
/// the sentinel is lengthened in the results and the
/// rendering runs again, until every sentinel stands
/// exactly where its carriers put it. The text rendering
/// then takes the raw bytes the detail stands for, and the
/// JSON rendering takes the decoded word's JSON spelling,
/// so no surface ever spells a shell held detail from a
/// text it cannot tell apart from a real character.
fn render_report(results: &mut [CheckResult], subs: &[DetailSub], json: bool) -> Vec<u8> {
    fn carriers(results: &[CheckResult], sentinel: &str) -> usize {
        results
            .iter()
            .filter(|result| {
                result
                    .trace_source
                    .as_ref()
                    .map(|source| source.detail == sentinel)
                    .unwrap_or(false)
            })
            .count()
    }
    let mut sentinels: Vec<String> = subs.iter().map(|sub| sub.sentinel.clone()).collect();
    let mut rendered = String::new();
    for _round in 0..32 {
        rendered = if json {
            blindgate::render_json(results)
        } else {
            blindgate::render_text(results)
        };
        let unsettled = sentinels.iter().enumerate().find(|(_, sentinel)| {
            let held = carriers(results, sentinel);
            held > 0
                && rendered.matches(sentinel.as_str()).count() != held * (if json { 2 } else { 1 })
        });
        match unsettled {
            None => break,
            Some((index, _)) => {
                let longer = format!("{}x", sentinels[index]);
                for result in results.iter_mut() {
                    if let Some(source) = &mut result.trace_source {
                        if source.detail == sentinels[index] {
                            source.detail = longer.clone();
                        }
                    }
                }
                sentinels[index] = longer;
            }
        }
    }
    if json {
        let mut text = rendered;
        for (index, sub) in subs.iter().enumerate() {
            if carriers(results, &sentinels[index]) > 0 {
                text = text.replace(&sentinels[index], &sub.word.json_spelling());
            }
        }
        text.into_bytes()
    } else {
        let mut bytes = rendered.into_bytes();
        for (index, sub) in subs.iter().enumerate() {
            if carriers(results, &sentinels[index]) > 0 {
                bytes = replace_bytes(&bytes, sentinels[index].as_bytes(), &sub.raw);
            }
        }
        bytes
    }
}

/// Compose the report under PORT5-3, or refuse.
///
/// The root resolves first, and inside the one error guard
/// the composition runs in the Python main's order, as the
/// contract section states it. The success value is the
/// report's stdout bytes, render and one trailing newline
/// included, and the failure value is the refusal's whole
/// stderr bytes.
fn compose(scan: &Scan) -> Result<Vec<u8>, Vec<u8>> {
    let workflow = scan.workflow.as_deref().unwrap_or(&[]);
    let config = scan.config.as_deref().unwrap_or(&[]);
    let root: PathBuf = match &scan.root {
        Some(bytes) => resolve_root(&path_from_bytes(bytes)),
        None => env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
    };
    // The truthiness the Python reads, where an empty trace
    // value and an empty run list both read as not given.
    let trace: Option<&[u8]> = scan.trace.as_deref().filter(|value| !value.is_empty());
    let run: Option<&[Vec<u8>]> = scan.run.as_deref().filter(|words| !words.is_empty());

    let mut repairs = vec![
        PathRepair::for_bytes(workflow),
        PathRepair::for_bytes(config),
    ];

    if trace.is_some() && run.is_some() {
        return Err(plain_refusal("give exactly one of --trace or --run"));
    }
    let jobs = load_workflow_jobs(&path_from_bytes(workflow))
        .map_err(|error| library_refusal(None, &error, &repairs))?;
    let checks = load_config(&path_from_bytes(config))
        .map_err(|error| library_refusal(None, &error, &repairs))?;
    let unknown = unknown_jobs(&checks, &jobs);
    if !unknown.is_empty() {
        let entries = unknown
            .iter()
            .map(|check| format!("{} (job {})", check.name, check.job))
            .collect::<Vec<String>>()
            .join(", ");
        return Err(plain_refusal(&format!(
            "config names jobs that the workflow does not declare, {entries}"
        )));
    }
    let config_path = path_from_bytes(config);
    let config_absolute = if config_path.is_absolute() {
        config_path
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(&config_path)
    };
    let config_resolved = resolve_root(&config_absolute);
    let config_dir = config_resolved
        .parent()
        .map(|parent| parent.to_path_buf())
        .unwrap_or(config_resolved);
    let overrides = parse_trace_for(&scan.trace_for, &checks, &config_dir)?;
    for (_, over) in &overrides {
        repairs.push(PathRepair::for_path(&over.read_path));
    }
    if trace.is_none()
        && run.is_none()
        && overrides.is_empty()
        && checks.iter().all(|check| check.trace_path.is_none())
    {
        return Err(plain_refusal("give exactly one of --trace or --run"));
    }

    let has_shared = trace.is_some() || run.is_some();
    let mut shared: Option<Shared> = None;
    if let Some(bytes) = trace {
        repairs.push(PathRepair::for_bytes(bytes));
        // A handed over trace is read eagerly, even when
        // every check carries its own, so a bad shared path
        // is never ignored.
        shared = Some(
            build_shared(Some(bytes), None, &root)
                .map_err(|error| library_refusal(None, &error, &repairs))?,
        );
    }
    let mut judgements: Vec<Judgement> = Vec::new();
    let mut subs: Vec<DetailSub> = Vec::new();
    let mut override_sentinels: Vec<Option<String>> = vec![None; overrides.len()];
    let mut shared_sentinel: Option<String> = None;
    let mut any_own = false;
    for check in &checks {
        if let Some((index, (_, over))) = overrides
            .iter()
            .enumerate()
            .find(|(_, (job, _))| job.matches(&check.job))
        {
            let read = read_trace_from_log_file(&over.read_path, &root).map_err(|error| {
                library_refusal(
                    Some(&format!("the trace for job {} cannot be used", check.job)),
                    &error,
                    &repairs,
                )
            })?;
            let sentinel = override_sentinels[index]
                .get_or_insert_with(|| {
                    let sentinel = format!("blindgate-shell-detail-{}", subs.len());
                    subs.push(DetailSub {
                        sentinel: sentinel.clone(),
                        word: over.display.clone(),
                        raw: over.display_raw.clone(),
                    });
                    sentinel
                })
                .clone();
            judgements.push(Judgement::new(
                read.reads,
                Some(TraceSource {
                    kind: SOURCE_OWN_TRACE.to_string(),
                    detail: sentinel,
                }),
                read.outside_reads,
            ));
            any_own = true;
        } else if let Some(trace_path) = &check.trace_path {
            repairs.push(PathRepair::for_path(trace_path));
            let read = read_trace_from_log_file(trace_path, &root).map_err(|error| {
                library_refusal(
                    Some(&format!(
                        "check {} names a trace that cannot be used",
                        check.name
                    )),
                    &error,
                    &repairs,
                )
            })?;
            judgements.push(Judgement::new(
                read.reads,
                Some(TraceSource {
                    kind: SOURCE_OWN_TRACE.to_string(),
                    detail: check.trace.clone().unwrap_or_default(),
                }),
                read.outside_reads,
            ));
            any_own = true;
        } else if has_shared {
            if shared.is_none() {
                shared = Some(
                    build_shared(trace, run, &root)
                        .map_err(|error| library_refusal(None, &error, &repairs))?,
                );
            }
            let shared = shared.as_ref().expect("built above");
            let sentinel = shared_sentinel
                .get_or_insert_with(|| {
                    let sentinel = format!("blindgate-shell-detail-{}", subs.len());
                    subs.push(DetailSub {
                        sentinel: sentinel.clone(),
                        word: shared.detail.clone(),
                        raw: shared.raw_detail.clone(),
                    });
                    sentinel
                })
                .clone();
            judgements.push(Judgement::new(
                shared.read_set.clone(),
                Some(TraceSource {
                    kind: shared.kind.clone(),
                    detail: sentinel,
                }),
                shared.outside_reads.clone(),
            ));
        } else {
            return Err(plain_refusal(&format!(
                "check {} has no trace of its own and no shared trace or run was given",
                check.name
            )));
        }
    }

    let mut results = if any_own {
        analyse_judged(&checks, &judgements)
            .map_err(|error| library_refusal(None, &error, &repairs))?
    } else if judgements
        .first()
        .map(|first| !first.outside_reads.is_empty())
        .unwrap_or(false)
    {
        // No check carried its own trace, so the per check
        // sources are dropped on purpose to keep the report
        // in its slice 1 shape, and the shared outside reads
        // are carried into it.
        let first = judgements.first().expect("checked above");
        analyse_with_outside(&checks, &first.read_set, &first.outside_reads)
    } else {
        let empty: Vec<String> = Vec::new();
        let read_set = judgements
            .first()
            .map(|first| &first.read_set)
            .unwrap_or(&empty);
        analyse(&checks, read_set)
    };
    let mut out = render_report(&mut results, &subs, scan.json);
    out.push(b'\n');
    Ok(out)
}

/// The process entry, scan and compose and print.
///
/// The exit follows the probed standings, 0 for the help
/// and for a completed report, and 2 for every refusal, the
/// argparse shape on its own stderr and the tool shape on
/// its own.
fn main() -> ExitCode {
    let words: Vec<OsString> = env::args_os().skip(1).collect();
    let tokens: Vec<Vec<u8>> = words.iter().map(word_bytes).collect();
    match scan(&tokens) {
        ScanOutcome::Help => {
            let _ = std::io::stdout().write_all(HELP.as_bytes());
            ExitCode::SUCCESS
        }
        ScanOutcome::Refusal(line) => {
            let _ = std::io::stderr().write_all(&argparse_refusal(&line));
            ExitCode::from(2)
        }
        ScanOutcome::Ready(scan) => match compose(&scan) {
            Ok(report) => {
                let _ = std::io::stdout().write_all(&report);
                ExitCode::SUCCESS
            }
            Err(refusal) => {
                let _ = std::io::stderr().write_all(&refusal);
                ExitCode::from(2)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_and_help_stand_verbatim() {
        assert_eq!(USAGE.len(), 149);
        assert_eq!(HELP.len(), 866);
        assert!(HELP.starts_with(USAGE));
        assert!(HELP.contains("defaults to the\n                        working directory\n"));
    }

    #[test]
    fn decode_word_carries_valid_text_and_marks_stray_bytes() {
        assert_eq!(decode_word(b"plain").text, "plain");
        assert_eq!(decode_word("tr\u{00E4}ce".as_bytes()).text, "tr\u{00E4}ce");
        let decoded = decode_word(b"bad\xfflog");
        assert_eq!(decoded.text.chars().count(), 7);
        assert_eq!(
            decoded.strays,
            vec![false, false, false, true, false, false, false]
        );
        assert_eq!(decode_word(b"\xc3").text, "\u{E043}".to_string());
        assert!(decode_word(b"\xc3").strays[0]);
    }

    #[test]
    fn a_real_character_and_a_stray_byte_spell_apart_on_every_surface() {
        let real = decode_word("\u{E000}".as_bytes());
        let stray = decode_word(b"\x80");
        assert_eq!(real.text, stray.text);
        assert!(!real.strays[0]);
        assert!(stray.strays[0]);
        assert_eq!(real.stderr_spelling(), "\u{E000}");
        assert_eq!(stray.stderr_spelling(), "\\udc80");
        assert_eq!(real.json_spelling(), "\\ue000");
        assert_eq!(stray.json_spelling(), "\\udc80");
        assert!(real.matches("\u{E000}"));
        assert!(!stray.matches("\u{E000}"));
    }

    #[test]
    fn repr_spells_a_stray_byte_as_a_lone_surrogate_escape() {
        let decoded = decode_word(b"bad\xfflog");
        assert_eq!(decoded.repr_spelling(), "'bad\\udcfflog'");
        assert_eq!(cpython_repr("plain"), "'plain'");
        assert_eq!(cpython_repr("it's"), "\"it's\"");
        assert_eq!(cpython_repr("a\u{200B}b"), "'a\\u200bb'");
        assert_eq!(cpython_repr("a\u{2028}b"), "'a\\u2028b'");
        assert_eq!(cpython_repr("a\u{E001}b"), "'a\\ue001b'");
    }

    #[test]
    fn scan_refusals_and_help_follow_the_model() {
        let refuse = |tokens: &[&[u8]]| -> String {
            let owned: Vec<Vec<u8>> = tokens.iter().map(|token| token.to_vec()).collect();
            match scan(&owned) {
                ScanOutcome::Refusal(line) => line,
                _ => panic!("the scan refuses"),
            }
        };
        assert_eq!(
            refuse(&[]),
            "the following arguments are required: --workflow, --config"
        );
        assert_eq!(
            refuse(&[b"--tr", b"T"]),
            "ambiguous option: --tr could match --trace, --trace-for"
        );
        assert_eq!(
            refuse(&[b"--workflow"]),
            "argument --workflow: expected one argument"
        );
        assert_eq!(
            refuse(&[b"--json=x"]),
            "argument --json: ignored explicit argument 'x'"
        );
        // A help token scanned before an ambiguity still
        // loses to it, and an unknown ahead of the help does
        // not stop the help.
        match scan(&[b"-h".to_vec(), b"--tr".to_vec(), b"T".to_vec()]) {
            ScanOutcome::Refusal(line) => {
                assert!(line.starts_with("ambiguous option: --tr"));
            }
            _ => panic!("the ambiguity refuses ahead of the help"),
        }
        match scan(&[b"--bogus".to_vec(), b"-h".to_vec()]) {
            ScanOutcome::Help => {}
            _ => panic!("the help answers behind a deferred unknown"),
        }
        // A help consumed before a missing value or an
        // ignored explicit argument answers at its own
        // position, and a missing value or an ignored
        // explicit argument standing before an ambiguity
        // loses to the ambiguity.
        match scan(&[b"-h".to_vec(), b"--trace".to_vec()]) {
            ScanOutcome::Help => {}
            _ => panic!("the help answers ahead of the missing value"),
        }
        match scan(&[b"-h".to_vec(), b"--json=x".to_vec()]) {
            ScanOutcome::Help => {}
            _ => panic!("the help answers ahead of the ignored explicit argument"),
        }
        assert_eq!(
            refuse(&[b"--trace", b"--tr", b"T"]),
            "ambiguous option: --tr could match --trace, --trace-for"
        );
        assert_eq!(
            refuse(&[b"--json=x", b"--tr", b"T"]),
            "ambiguous option: --tr could match --trace, --trace-for"
        );
        // The separator at the head of a remainder names the
        // whole tail once the required flags stand.
        assert_eq!(
            refuse(&[
                b"--workflow",
                b"W",
                b"--config",
                b"C",
                b"--run",
                b"--",
                b"/bin/cat",
            ]),
            "unrecognized arguments: -- /bin/cat"
        );
    }
}
