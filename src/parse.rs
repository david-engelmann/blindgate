use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;

use crate::classify::{bare_pid_form, refuse_unshaped_trace, split_lines};
use crate::errors::BlindgateError;
use crate::model::TraceRead;
use crate::scope::{
    canonical_scoped_based, is_within, normpath, posix_join, realpath, relative_to,
};

const READ_FLAGS: [&str; 2] = ["O_RDONLY", "O_RDWR"];
const NON_CONTENT_FLAGS: [&str; 2] = ["O_PATH", "O_DIRECTORY"];
const UNFINISHED_MARKER: &str = "<unfinished ...>";
const SPLIT_CALLS: [&str; 10] = [
    "chdir", "fchdir", "open", "openat", "openat2", "close", "dup", "dup2", "dup3", "fcntl",
];
const EXEC_CALLS: [&str; 2] = ["execve", "execveat"];

fn is_ws(c: char) -> bool {
    // The whitespace class of the Python patterns counts the
    // file, group, record, and unit separators as whitespace
    // beside the Unicode white space the Rust class already
    // holds, so the matchers read all four as whitespace too.
    c.is_whitespace() || ('\u{001C}'..='\u{001F}').contains(&c)
}

fn skip_ws(line: &str, mut index: usize) -> usize {
    for c in line[index..].chars() {
        if is_ws(c) {
            index += c.len_utf8();
        } else {
            break;
        }
    }
    index
}

fn is_read(flags: &str) -> bool {
    let parts: Vec<&str> = flags.split('|').collect();
    if NON_CONTENT_FLAGS.iter().any(|flag| parts.contains(flag)) {
        return false;
    }
    READ_FLAGS.iter().any(|flag| parts.contains(flag))
}

fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// A number read from a trace line, exact at any size.
///
/// The Python reads every number at arbitrary precision, so
/// two numbers past the machine word stay distinct there and
/// must stay distinct here. A number in this reading is only
/// ever held as a key, compared for equality, or compared
/// against zero, and never enters arithmetic, so it is held
/// as its sign and its canonical decimal magnitude, the
/// magnitude spelled with no leading zeros. Equality and
/// order on that holding are exactly the Python integer
/// comparisons, at any size a line can spell.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Num {
    negative: bool,
    digits: String,
}

impl Num {
    /// Read a magnitude spelled in decimal digits.
    ///
    /// Leading zeros fall away and an all zero spelling reads
    /// as zero, and a zero magnitude carries no sign, so a
    /// spelled minus zero reads as the zero the Python reads.
    fn from_decimal(negative: bool, digits: &str) -> Num {
        let magnitude = digits.trim_start_matches('0');
        if magnitude.is_empty() {
            return Num {
                negative: false,
                digits: "0".to_string(),
            };
        }
        Num {
            negative,
            digits: magnitude.to_string(),
        }
    }

    /// Read a magnitude spelled in hex digits.
    ///
    /// The conversion multiplies the standing decimal digits
    /// by sixteen and adds each hex digit in turn, so a hex
    /// spelling of any length reads to its exact decimal
    /// magnitude. The caller passes only hex digits, the run
    /// the token reading measured.
    fn from_hex(negative: bool, hex: &str) -> Num {
        let mut digits: Vec<u8> = vec![0];
        for c in hex.bytes() {
            let add = match c {
                b'0'..=b'9' => u16::from(c - b'0'),
                b'a'..=b'f' => u16::from(c - b'a') + 10,
                b'A'..=b'F' => u16::from(c - b'A') + 10,
                _ => 0,
            };
            let mut carry = add;
            for digit in digits.iter_mut().rev() {
                let value = u16::from(*digit) * 16 + carry;
                *digit = (value % 10) as u8;
                carry = value / 10;
            }
            while carry > 0 {
                digits.insert(0, (carry % 10) as u8);
                carry /= 10;
            }
        }
        let spelled: String = digits.iter().map(|digit| (b'0' + digit) as char).collect();
        Num::from_decimal(negative, &spelled)
    }

    fn is_zero(&self) -> bool {
        self.digits == "0"
    }

    fn is_negative(&self) -> bool {
        self.negative
    }
}

/// Order two canonical decimal magnitudes.
///
/// A longer magnitude is the larger number, and two of the
/// same length order as their spellings order.
fn magnitude_order(first: &str, second: &str) -> Ordering {
    first
        .len()
        .cmp(&second.len())
        .then_with(|| first.cmp(second))
}

impl Ord for Num {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => magnitude_order(&self.digits, &other.digits),
            (true, true) => magnitude_order(&other.digits, &self.digits),
        }
    }
}

impl PartialOrd for Num {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl From<i64> for Num {
    fn from(value: i64) -> Self {
        let text = value.to_string();
        match text.strip_prefix('-') {
            Some(digits) => Num::from_decimal(true, digits),
            None => Num::from_decimal(false, &text),
        }
    }
}

/// Read one recorded return token, in decimal or in hex.
///
/// The token may carry a minus sign on either form, the reading
/// the recorded return rule states. The reading is exact at any
/// size, because the Python keeps two numbers past the machine
/// word distinct, and a return held as a descriptor number must
/// stay the number its line spelled. A decimal token spelled
/// with a leading zero reads as the decimal it spells, the
/// stated limit the contract names,
/// because the Python conversion at base zero refuses the
/// spelling and crashes, and a crash is not a reading.
fn return_token(text: &str) -> Option<Num> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let (negative, digits) = if bytes[0] == b'-' {
        (true, &text[1..])
    } else {
        (false, text)
    };
    if let Some(hex) = digits.strip_prefix("0x") {
        let count = hex.len()
            - hex
                .trim_start_matches(|c: char| c.is_ascii_hexdigit())
                .len();
        if count > 0 {
            return Some(Num::from_hex(negative, &hex[..count]));
        }
    }
    let digits_bytes = digits.as_bytes();
    let mut count = 0;
    while count < digits_bytes.len() && digits_bytes[count].is_ascii_digit() {
        count += 1;
    }
    if count == 0 {
        return None;
    }
    Some(Num::from_decimal(negative, &digits[..count]))
}

/// Find the first recorded return a line carries after a point.
///
/// An equals sign that no return token follows is passed over,
/// and the search settles on the first equals sign a whole
/// number token does follow, the reading the Python return
/// pattern gives the tail of a call line.
fn find_return(rest: &str) -> Option<Num> {
    let bytes = rest.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'=' {
            let at = skip_ws(rest, index + 1);
            if let Some(value) = return_token(&rest[at..]) {
                return Some(value);
            }
        }
        index += 1;
    }
    None
}

/// Read a run of decimal digits as a number, exactly.
///
/// The Python reads such a run at arbitrary precision, so the
/// same digits read to the same number wherever a line spells
/// them, and two different numbers past the machine word stay
/// two numbers here as they do there.
fn parse_number(digits: &str) -> Num {
    Num::from_decimal(false, digits)
}

/// The process key of a trace line, its number or the shared one.
///
/// Lines with no process id prefix share one bucket of their
/// own, keyed by the empty key, exactly as one unnumbered
/// stream was treated by the earlier slices. Digits read in
/// ASCII only, the limit the Round 15 review fixes state.
type PidKey = Option<Num>;

fn pid_key(line: &str) -> PidKey {
    let s = line.trim_start_matches(is_ws);
    let digits = s.len() - s.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let rest = &s[digits..];
    if rest.chars().next().is_some_and(is_ws) {
        Some(parse_number(&s[..digits]))
    } else {
        None
    }
}

/// Match the prefix every call pattern opens with.
///
/// Leading whitespace and an optional process number and its
/// whitespace, and the return is the index the call name
/// starts at.
fn prefix_end(line: &str) -> usize {
    let mut index = skip_ws(line, 0);
    let digits = line[index..].len()
        - line[index..]
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .len();
    if digits > 0 {
        let after = skip_ws(line, index + digits);
        if after > index + digits {
            index = after;
        }
    }
    index
}

/// Read an identifier at a position, with no shape after it.
fn name_at(line: &str, index: usize) -> Option<(&str, usize)> {
    let bytes = line.as_bytes();
    let first = *bytes.get(index)?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let mut end = index + 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    Some((&line[index..end], end))
}

/// Read a call name hard against its opening parenthesis.
///
/// Returns the name and the index just after the parenthesis.
fn call_at(line: &str, index: usize) -> Option<(&str, usize)> {
    let (name, end) = name_at(line, index)?;
    if line.as_bytes().get(end) != Some(&b'(') {
        return None;
    }
    Some((name, end + 1))
}

/// Read the call a line names at its call position, if it names one.
fn call_name_of(line: &str) -> Option<(&str, usize)> {
    call_at(line, prefix_end(line))
}

/// Match the call a whole line pattern names, returning the
/// index just after its opening parenthesis.
fn named_call(line: &str, wanted: &str) -> Option<usize> {
    let (name, after) = call_name_of(line)?;
    if name == wanted {
        Some(after)
    } else {
        None
    }
}

/// Read a run of decimal digits at a position.
fn digits_at(line: &str, index: usize) -> Option<(&str, usize)> {
    let bytes = line.as_bytes();
    let mut end = index;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == index {
        None
    } else {
        Some((&line[index..end], end))
    }
}

/// Read a run of flag characters at a position, its end index.
fn flag_run_end(line: &str, index: usize) -> usize {
    let bytes = line.as_bytes();
    let mut end = index;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'|')
    {
        end += 1;
    }
    end
}

/// Read a quoted string at a position.
///
/// Returns the raw inner text with its escapes still spelled
/// as strace spelled them and the index just after the closing
/// quote, the shape the path patterns of the calls all carry.
fn quoted_at(line: &str, index: usize) -> Option<(&str, usize)> {
    let bytes = line.as_bytes();
    if bytes.get(index) != Some(&b'"') {
        return None;
    }
    let mut cursor = index + 1;
    loop {
        match bytes.get(cursor) {
            None => return None,
            Some(b'\\') => cursor += 2,
            Some(b'"') => return Some((&line[index + 1..cursor], cursor + 1)),
            Some(_) => cursor += 1,
        }
    }
}

/// Match the closing parenthesis a whole line pattern's tail
/// carries, returning the index just after it.
fn close_paren(line: &str, index: usize) -> Option<usize> {
    let at = skip_ws(line, index);
    if line.as_bytes().get(at) == Some(&b')') {
        Some(at + 1)
    } else {
        None
    }
}

/// Read the argument pattern the quoted path calls carry.
///
/// One argument pattern per call, held once, serves the whole
/// line reading and the keep reading alike, so the two
/// readings of a call shape cannot drift apart.
fn args_quoted_path(line: &str, index: usize) -> Option<(&str, usize)> {
    quoted_at(line, skip_ws(line, index))
}

/// Read the argument pattern the one number calls carry.
fn args_one_number(line: &str, index: usize) -> Option<(&str, usize)> {
    digits_at(line, skip_ws(line, index))
}

/// Read the argument pattern the dup pair calls carry.
fn args_pair(line: &str, index: usize) -> Option<(Num, Num, usize)> {
    let (first, end) = digits_at(line, skip_ws(line, index))?;
    let at = skip_ws(line, end);
    if line.as_bytes().get(at) != Some(&b',') {
        return None;
    }
    let (second, end) = digits_at(line, skip_ws(line, at + 1))?;
    Some((parse_number(first), parse_number(second), end))
}

/// Read the dup3 argument pattern, with its optional flags.
fn args_dup3(line: &str, index: usize) -> Option<(Num, Num, usize)> {
    let (first, second, end) = args_pair(line, index)?;
    let probe = skip_ws(line, end);
    if line.as_bytes().get(probe) == Some(&b',') {
        let flags_start = skip_ws(line, probe + 1);
        let flags_end = flag_run_end(line, flags_start);
        if flags_end > flags_start {
            return Some((first, second, flags_end));
        }
    }
    Some((first, second, end))
}

/// The fcntl readings, by the command the line carries.
///
/// The argument pattern states its third argument by command,
/// a descriptor flags value for the set flags shape and a
/// decimal requested floor for the two duplicating shapes, so
/// a line of any other command or a duplicating command
/// carrying the wrong argument kind fails the reading by shape.
enum FcntlArgs {
    SetFd(String),
    DupFd { cloexec: bool },
}

fn args_fcntl(line: &str, index: usize) -> Option<(Num, FcntlArgs, usize)> {
    let (digits, end) = digits_at(line, skip_ws(line, index))?;
    let at = skip_ws(line, end);
    if line.as_bytes().get(at) != Some(&b',') {
        return None;
    }
    let at = skip_ws(line, at + 1);
    let fd = parse_number(digits);
    if let Some(after) = literal_at(line, at, "F_SETFD") {
        let comma = skip_ws(line, after);
        if line.as_bytes().get(comma) == Some(&b',') {
            let start = skip_ws(line, comma + 1);
            let flags_end = flag_run_end(line, start);
            if flags_end > start {
                return Some((
                    fd,
                    FcntlArgs::SetFd(line[start..flags_end].to_string()),
                    flags_end,
                ));
            }
        }
    }
    for (literal, cloexec) in [("F_DUPFD_CLOEXEC", true), ("F_DUPFD", false)] {
        if let Some(after) = literal_at(line, at, literal) {
            let comma = skip_ws(line, after);
            if line.as_bytes().get(comma) == Some(&b',') {
                if let Some((_, floor_end)) = digits_at(line, skip_ws(line, comma + 1)) {
                    return Some((fd, FcntlArgs::DupFd { cloexec }, floor_end));
                }
            }
        }
    }
    None
}

fn literal_at(line: &str, index: usize, literal: &str) -> Option<usize> {
    if line[index..].starts_with(literal) {
        Some(index + literal.len())
    } else {
        None
    }
}

/// The whole line reading of a chdir line.
fn match_chdir(line: &str) -> Option<(&str, usize)> {
    let (raw, end) = args_quoted_path(line, named_call(line, "chdir")?)?;
    Some((raw, close_paren(line, end)?))
}

/// The whole line reading of a one number call, by its name.
fn match_number_call(line: &str, name: &str) -> Option<(Num, usize)> {
    let (digits, end) = args_one_number(line, named_call(line, name)?)?;
    Some((parse_number(digits), close_paren(line, end)?))
}

/// The whole line reading of a dup2 line.
fn match_dup2(line: &str) -> Option<(Num, Num, usize)> {
    args_pair(line, named_call(line, "dup2")?)
}

/// The whole line reading of a dup3 line.
fn match_dup3(line: &str) -> Option<(Num, Num, usize)> {
    args_dup3(line, named_call(line, "dup3")?)
}

/// The whole line reading of an fcntl line.
fn match_fcntl(line: &str) -> Option<(Num, FcntlArgs, usize)> {
    let (fd, args, end) = args_fcntl(line, named_call(line, "fcntl")?)?;
    Some((fd, args, close_paren(line, end)?))
}

/// The whole line reading of an open line.
///
/// The pattern ends where its flags argument ends without
/// capturing it, and the flags text is read back from the
/// matched region by the flags tail reading.
fn match_open(line: &str) -> Option<(&str, usize)> {
    let (raw, end) = args_quoted_path(line, named_call(line, "open")?)?;
    let at = skip_ws(line, end);
    if line.as_bytes().get(at) != Some(&b',') {
        return None;
    }
    let flags_start = skip_ws(line, at + 1);
    let flags_end = flag_run_end(line, flags_start);
    if flags_end == flags_start {
        return None;
    }
    Some((raw, flags_end))
}

/// The keep reading of a chdir half.
fn keep_chdir(prefix: &str) -> bool {
    let Some(after) = named_call(prefix, "chdir") else {
        return false;
    };
    match args_quoted_path(prefix, after) {
        Some((_, end)) => skip_ws(prefix, end) == prefix.len(),
        None => false,
    }
}

/// The keep reading of a one number call half, by its name.
fn keep_number_call(prefix: &str, name: &str) -> bool {
    let Some(after) = named_call(prefix, name) else {
        return false;
    };
    match args_one_number(prefix, after) {
        Some((_, end)) => skip_ws(prefix, end) == prefix.len(),
        None => false,
    }
}

/// The keep reading of a dup2 half.
fn keep_dup2(prefix: &str) -> bool {
    let Some(after) = named_call(prefix, "dup2") else {
        return false;
    };
    match args_pair(prefix, after) {
        Some((_, _, end)) => skip_ws(prefix, end) == prefix.len(),
        None => false,
    }
}

/// The keep reading of a dup3 half.
fn keep_dup3(prefix: &str) -> bool {
    let Some(after) = named_call(prefix, "dup3") else {
        return false;
    };
    match args_dup3(prefix, after) {
        Some((_, _, end)) => skip_ws(prefix, end) == prefix.len(),
        None => false,
    }
}

/// The keep reading of an fcntl half.
fn keep_fcntl(prefix: &str) -> bool {
    let Some(after) = named_call(prefix, "fcntl") else {
        return false;
    };
    match args_fcntl(prefix, after) {
        Some((_, _, end)) => skip_ws(prefix, end) == prefix.len(),
        None => false,
    }
}

struct OpenatMatch<'a> {
    dirfd: &'a str,
    raw_path: &'a str,
    flags: &'a str,
    end: usize,
}

/// Read one openat call shape starting just after its name.
///
/// The reading mirrors the Python pattern piece by piece. The
/// descriptor token runs to the first comma, the path is the
/// quoted string the call carries with its escapes still spelled
/// as strace spelled them, and the flags are the run of flag
/// characters that follows. Anything else fails the shape at
/// this position, and the search then tries a later position.
fn try_openat_at(line: &str, start: usize) -> Option<OpenatMatch<'_>> {
    let bytes = line.as_bytes();
    // The descriptor segment runs to the first comma, and any
    // segment of at least one character holds the shape, the
    // reading the Python pattern gives its lazy descriptor
    // group, so a segment of one blank stands as the token.
    let comma = line[start..].find(',').map(|at| start + at)?;
    if comma == start {
        return None;
    }
    let mut index = skip_ws(line, comma + 1);
    if bytes.get(index) != Some(&b'"') {
        return None;
    }
    let path_start = index + 1;
    let mut cursor = path_start;
    let path_end = loop {
        match bytes.get(cursor) {
            None => return None,
            Some(b'\\') => cursor += 2,
            Some(b'"') => break cursor,
            Some(_) => cursor += 1,
        }
    };
    index = skip_ws(line, path_end + 1);
    if bytes.get(index) != Some(&b',') {
        return None;
    }
    index = skip_ws(line, index + 1);
    let flags_start = index;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' || bytes[index] == b'|')
    {
        index += 1;
    }
    if index == flags_start {
        return None;
    }
    Some(OpenatMatch {
        dirfd: &line[start..comma],
        raw_path: &line[path_start..path_end],
        flags: &line[flags_start..index],
        end: index,
    })
}

fn find_openat(line: &str) -> Option<OpenatMatch<'_>> {
    let mut start = 0;
    while let Some(found) = line[start..].find("openat(") {
        let at = start + found;
        if let Some(matched) = try_openat_at(line, at + "openat(".len()) {
            return Some(matched);
        }
        start = at + 1;
    }
    None
}

/// Read one openat2 call shape starting just after its name.
///
/// The reading stands beside the openat reading and produces
/// the same match. The descriptor segment and the quoted path
/// read exactly as openat's do. The open_how struct then stands
/// in braces, and its flags field is read by name where the
/// captures print it, first in the struct, its value running
/// as the same run of flag characters openat carries. The
/// mode and resolve fields are read past to the closing brace
/// and never judged, the mode because the judgement has no
/// use for it and the resolve under the limit S17-3 states,
/// and the size argument after the struct is read past with
/// nothing asked of its value. A struct with no readable
/// flags field fails the shape, under S17-2.
fn try_openat2_at(line: &str, start: usize) -> Option<OpenatMatch<'_>> {
    let bytes = line.as_bytes();
    let comma = line[start..].find(',').map(|at| start + at)?;
    if comma == start {
        return None;
    }
    let mut index = skip_ws(line, comma + 1);
    if bytes.get(index) != Some(&b'"') {
        return None;
    }
    let path_start = index + 1;
    let mut cursor = path_start;
    let path_end = loop {
        match bytes.get(cursor) {
            None => return None,
            Some(b'\\') => cursor += 2,
            Some(b'"') => break cursor,
            Some(_) => cursor += 1,
        }
    };
    index = skip_ws(line, path_end + 1);
    if bytes.get(index) != Some(&b',') {
        return None;
    }
    index = skip_ws(line, index + 1);
    if bytes.get(index) != Some(&b'{') {
        return None;
    }
    index = skip_ws(line, index + 1);
    if !line[index..].starts_with("flags=") {
        return None;
    }
    index += "flags=".len();
    let flags_start = index;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' || bytes[index] == b'|')
    {
        index += 1;
    }
    if index == flags_start {
        return None;
    }
    let flags_end = index;
    let close = line[index..].find('}').map(|at| index + at)?;
    index = skip_ws(line, close + 1);
    if bytes.get(index) != Some(&b',') {
        return None;
    }
    index = skip_ws(line, index + 1);
    let size_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == size_start {
        return None;
    }
    Some(OpenatMatch {
        dirfd: &line[start..comma],
        raw_path: &line[path_start..path_end],
        flags: &line[flags_start..flags_end],
        end: index,
    })
}

fn find_openat2(line: &str) -> Option<OpenatMatch<'_>> {
    let mut start = 0;
    while let Some(found) = line[start..].find("openat2(") {
        let at = start + found;
        if let Some(matched) = try_openat2_at(line, at + "openat2(".len()) {
            return Some(matched);
        }
        start = at + 1;
    }
    None
}

/// Return the call name and text of a kept unfinished line.
///
/// An unfinished line is kept when it ends with the unfinished
/// marker, names one of the split calls the parse reassembles,
/// and the shape rules of that call can read the call and its
/// arguments from the half that is present, exactly as they
/// read them on a whole line. The kept text is the line with
/// the marker cut away, ready to be joined to its resumed
/// partner. Anything else returns nothing, and such a line
/// then records nothing, exactly as an unfinished line always
/// has. An unfinished line carrying a spelled return is taken
/// as a half under this same keep rule where the shape rules
/// read it, and records nothing until it is spent.
fn kept_unfinished(line: &str) -> Option<(String, String)> {
    let marker_at = line.find(UNFINISHED_MARKER)?;
    if !line[marker_at + UNFINISHED_MARKER.len()..]
        .trim_matches(is_ws)
        .is_empty()
    {
        return None;
    }
    let prefix = &line[..marker_at];
    let (name, _) = call_name_of(prefix)?;
    let readable = match name {
        "openat" => find_openat(prefix).is_some(),
        "openat2" => find_openat2(prefix).is_some(),
        "open" => match_open(prefix).is_some(),
        "chdir" => keep_chdir(prefix),
        "fchdir" => keep_number_call(prefix, "fchdir"),
        "close" => keep_number_call(prefix, "close"),
        "dup" => keep_number_call(prefix, "dup"),
        "dup2" => keep_dup2(prefix),
        "dup3" => keep_dup3(prefix),
        "fcntl" => keep_fcntl(prefix),
        _ => return None,
    };
    if !readable {
        return None;
    }
    Some((name.to_string(), prefix.to_string()))
}

/// Return the reassembled line for a resumed line, else the line.
///
/// A resumed line for one of the split calls spends the
/// unfinished line kept under its own process and its own call
/// name, whether or not the pair judges to anything, and the
/// reassembled line is the kept text joined to the part of the
/// resumed line that follows the resumed marker, which carries
/// the return. A resumed line with nothing kept for it, and
/// any other line, comes back unchanged and is read by the
/// usual branches.
fn spend_resumed(line: &str, pending: &mut HashMap<(PidKey, String), String>) -> String {
    let Some((name, end)) = resumed_marker(line) else {
        return line.to_string();
    };
    if !SPLIT_CALLS.contains(&name) {
        return line.to_string();
    }
    match pending.remove(&(pid_key(line), name.to_string())) {
        Some(prefix) => format!("{prefix}{}", &line[end..]),
        None => line.to_string(),
    }
}

/// Read the resumed marker a line opens with.
///
/// Returns the call name the marker names and the index just
/// after the marker, when the line carries one.
fn resumed_marker(line: &str) -> Option<(&str, usize)> {
    let mut index = prefix_end(line);
    if !line[index..].starts_with("<...") {
        return None;
    }
    index += "<...".len();
    let after_ws = skip_ws(line, index);
    if after_ws == index {
        return None;
    }
    let (name, name_end) = name_at(line, after_ws)?;
    let after_name = skip_ws(line, name_end);
    if after_name == name_end {
        return None;
    }
    if !line[after_name..].starts_with("resumed>") {
        return None;
    }
    Some((name, after_name + "resumed>".len()))
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Find a flags assignment in a text, its flag text.
///
/// The assignment is the word flags hard against an equals
/// sign and a run of flag characters, standing at a word
/// boundary. The closed form also requires the comma that
/// follows the flag list in the printed call.
fn find_flags_assignment(text: &str, closed: bool) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut start = 0;
    while let Some(found) = text[start..].find("flags=") {
        let at = start + found;
        if at == 0 || !is_word_byte(bytes[at - 1]) {
            let run_start = at + "flags=".len();
            let run_end = flag_run_end(text, run_start);
            if run_end > run_start && (!closed || bytes.get(run_end) == Some(&b',')) {
                return Some(&text[run_start..run_end]);
            }
        }
        start = at + 1;
    }
    None
}

/// Read a flags assignment standing at one position.
fn flags_assignment_at(text: &str, index: usize) -> Option<&str> {
    if !text[index..].starts_with("flags=") {
        return None;
    }
    if index > 0 && is_word_byte(text.as_bytes()[index - 1]) {
        return None;
    }
    let run_start = index + "flags=".len();
    let run_end = flag_run_end(text, run_start);
    if run_end == run_start {
        return None;
    }
    Some(&text[run_start..run_end])
}

/// Return the sharing a bar joined flag list names.
///
/// The pair is the descriptor table sharing CLONE_FILES names
/// and the directory state sharing CLONE_FS names, each true
/// only when the flag stands in the list as its own token.
fn sharing_from_flag_text(flag_text: &str) -> (bool, bool) {
    let tokens: Vec<&str> = flag_text.split('|').collect();
    (
        tokens.contains(&"CLONE_FILES"),
        tokens.contains(&"CLONE_FS"),
    )
}

/// Return the sharing a whole clone line's flags name.
///
/// Only a clone line carries a flags argument this parse
/// reads. Any other fork call, and a clone line with no
/// readable flags argument, names no sharing.
fn clone_line_sharing(line: &str) -> (bool, bool) {
    match call_name_of(line) {
        Some(("clone", _)) => {}
        _ => return (false, false),
    }
    match find_flags_assignment(line, false) {
        Some(text) => sharing_from_flag_text(text),
        None => (false, false),
    }
}

/// Return the text inside the braces of a clone3 struct.
///
/// The struct argument is the first brace span the call opens
/// with. The output struct strace prints after the arrow is a
/// later span and is never the span returned, and a line whose
/// first argument is a bare pointer has no span at all.
fn clone3_struct_span(line: &str) -> Option<&str> {
    let (name, after) = call_name_of(line)?;
    if name != "clone3" {
        return None;
    }
    let rest = &line[after..];
    let arrow = rest.find("=>");
    let opening = rest.find('{')?;
    if let Some(arrow) = arrow {
        if arrow < opening {
            return None;
        }
    }
    let bytes = rest.as_bytes();
    let mut depth = 0i32;
    let mut index = opening;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[opening + 1..index]);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// Return the sharing a whole clone3 line's struct names.
///
/// Only the flags field inside the braces of the struct
/// argument is read, and nowhere else on the line, and only
/// at the top level of the struct, never inside a span the
/// struct itself nests. A clone3 line whose struct is not
/// decoded, or whose span holds no flags field, names no
/// sharing.
fn clone3_line_sharing(line: &str) -> (bool, bool) {
    let Some(span) = clone3_struct_span(line) else {
        return (false, false);
    };
    let bytes = span.as_bytes();
    let mut depth = 0i32;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ if depth == 0 => {
                if let Some(text) = flags_assignment_at(span, index) {
                    return sharing_from_flag_text(text);
                }
            }
            _ => {}
        }
        index += 1;
    }
    (false, false)
}

/// Return the sharing a whole fork line names, by call name.
///
/// A clone names its sharing in its flags argument and a
/// clone3 in the flags field of its struct argument. Every
/// other fork call names no sharing.
fn fork_line_sharing(line: &str, name: &str) -> (bool, bool) {
    match name {
        "clone" => clone_line_sharing(line),
        "clone3" => clone3_line_sharing(line),
        _ => (false, false),
    }
}

/// Return the sharing an unfinished clone line's flags name.
///
/// The half is kept for its flag text alone, and only when the
/// flag list stands complete in the half, closed by the comma
/// that follows it in the printed call. Anything else returns
/// nothing, and the split clone then reads as a copy.
fn kept_clone_half(line: &str) -> Option<(bool, bool)> {
    let marker_at = line.find(UNFINISHED_MARKER)?;
    if !line[marker_at + UNFINISHED_MARKER.len()..]
        .trim_matches(is_ws)
        .is_empty()
    {
        return None;
    }
    let prefix = &line[..marker_at];
    let (name, _) = call_name_of(prefix)?;
    if name != "clone" {
        return None;
    }
    find_flags_assignment(prefix, true).map(sharing_from_flag_text)
}

/// Return the sharing an unfinished clone3 line's flags name.
///
/// The half is kept for its flag text alone, and only when the
/// struct flags list stands complete in the half, closed by
/// the comma or the closing brace that follows it in the
/// printed call. Anything else returns nothing, and the split
/// clone3 then reads as a copy.
fn kept_clone3_half(line: &str) -> Option<(bool, bool)> {
    let marker_at = line.find(UNFINISHED_MARKER)?;
    if !line[marker_at + UNFINISHED_MARKER.len()..]
        .trim_matches(is_ws)
        .is_empty()
    {
        return None;
    }
    let prefix = &line[..marker_at];
    let (name, _) = call_name_of(prefix)?;
    if name != "clone3" {
        return None;
    }
    let opening = prefix.find('{')?;
    if !prefix[opening..].starts_with("{flags=") {
        return None;
    }
    let run_start = opening + "{flags=".len();
    let run_end = flag_run_end(prefix, run_start);
    if run_end == run_start {
        return None;
    }
    match prefix.as_bytes().get(run_end) {
        Some(b',') | Some(b'}') => Some(sharing_from_flag_text(&prefix[run_start..run_end])),
        _ => None,
    }
}

/// Return the number a pid changed tail names, when a line carries one.
///
/// The tail is the marker strace prints when an exec renumbers
/// its process, and it stands at the end of its line with
/// nothing but whitespace behind it.
fn pid_changed_tail(line: &str) -> Option<Num> {
    let needle = "<pid changed to ";
    let mut start = 0;
    while let Some(found) = line[start..].find(needle) {
        let at = start + found;
        if let Some((digits, end)) = digits_at(line, at + needle.len()) {
            if let Some(tail) = line[end..].strip_prefix(" ...>") {
                if tail.trim_matches(is_ws).is_empty() {
                    return Some(parse_number(digits));
                }
            }
        }
        start = at + 1;
    }
    None
}

/// Return the start and completion numbers and name of an exec half.
///
/// A half of an exec family call is kept on its identity
/// alone, the process its line names and the call name,
/// because no reading in this parse consumes the arguments
/// of an exec call and the return of the call stands complete
/// on the resumed half. The split can fall anywhere in the
/// argument text, so no argument shape is required of the
/// half, only that it names one of the exec calls at the call
/// position and ends with one of the two tails strace gives a
/// deferred call of the family, the unfinished marker with
/// nothing standing after it or the pid changed tail that
/// names the number the call completes under. The pair
/// returned is the number the line carries and the number the
/// call completes under, the same number for the unfinished
/// marker spelling, beside the call name the half is kept
/// under. Anything else returns nothing.
fn kept_exec_half(line: &str) -> Option<(PidKey, PidKey, &'static str)> {
    let (name, _) = call_name_of(line)?;
    let call: &'static str = match name {
        "execve" => "execve",
        "execveat" => "execveat",
        _ => return None,
    };
    let starter = pid_key(line);
    if let Some(changed) = pid_changed_tail(line) {
        return Some((starter, Some(changed), call));
    }
    let marker_at = line.find(UNFINISHED_MARKER)?;
    if !line[marker_at + UNFINISHED_MARKER.len()..]
        .trim_matches(is_ws)
        .is_empty()
    {
        return None;
    }
    Some((starter.clone(), starter, call))
}

/// Whether a line ends in the unfinished state.
///
/// The marker position is the first marker the line carries,
/// and the line is unfinished when nothing but whitespace
/// stands behind it.
fn line_is_unfinished(line: &str) -> bool {
    match line.find(UNFINISHED_MARKER) {
        Some(at) => line[at + UNFINISHED_MARKER.len()..]
            .trim_matches(is_ws)
            .is_empty(),
        None => false,
    }
}

/// Whether a line is an exit line for its process.
fn is_exit_line(line: &str) -> bool {
    let rest = &line[prefix_end(line)..];
    let Some(rest) = rest.strip_prefix("+++") else {
        return false;
    };
    let after = skip_ws(rest, 0);
    if after == 0 {
        return false;
    }
    let rest = &rest[after..];
    for word in ["exited", "killed"] {
        if let Some(tail) = rest.strip_prefix(word) {
            return tail
                .chars()
                .next()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        }
    }
    false
}

/// Read a superseded banner, its survivor and starter numbers.
///
/// The banner stands under the leader number and names the
/// number that started the call. A banner standing with no
/// process prefix is not followed and reads as nothing here.
fn superseded_match(line: &str) -> Option<(Num, Num)> {
    let start = skip_ws(line, 0);
    let (first, end) = digits_at(line, start)?;
    let at = skip_ws(line, end);
    if at == end || !line[at..].starts_with("+++") {
        return None;
    }
    let after_marks = skip_ws(line, at + 3);
    if after_marks == at + 3 {
        return None;
    }
    let literal = "superseded by execve in pid";
    if !line[after_marks..].starts_with(literal) {
        return None;
    }
    let digits_from = skip_ws(line, after_marks + literal.len());
    if digits_from == after_marks + literal.len() {
        return None;
    }
    let (second, end) = digits_at(line, digits_from)?;
    let at = skip_ws(line, end);
    if at == end || !line[at..].starts_with("+++") {
        return None;
    }
    Some((parse_number(first), parse_number(second)))
}

struct ForkMatch {
    name: String,
    child: Num,
    resumed: bool,
}

/// Read a fork line, whole form first, then the resumed form.
///
/// A fork line records the child process id as its return,
/// standing at the end of a whole line or behind the closing
/// parenthesis of a resumed one.
fn fork_match(line: &str) -> Option<ForkMatch> {
    if let Some(matched) = fork_whole(line) {
        return Some(matched);
    }
    fork_resumed(line)
}

fn fork_whole(line: &str) -> Option<ForkMatch> {
    let (name, after) = call_name_of(line)?;
    if !matches!(name, "clone" | "clone3" | "fork" | "vfork") {
        return None;
    }
    let closings: Vec<usize> = line
        .match_indices(')')
        .map(|(index, _)| index)
        .filter(|index| *index >= after)
        .collect();
    for index in closings.iter().rev() {
        if let Some(child) = return_tail_value(&line[index + 1..]) {
            return Some(ForkMatch {
                name: name.to_string(),
                child,
                resumed: false,
            });
        }
    }
    None
}

fn fork_resumed(line: &str) -> Option<ForkMatch> {
    let (name, after) = resumed_marker(line)?;
    if !matches!(name, "clone" | "clone3" | "fork" | "vfork") {
        return None;
    }
    let closings: Vec<usize> = line
        .match_indices(')')
        .map(|(index, _)| index)
        .filter(|index| *index >= after)
        .collect();
    for index in closings.iter().rev() {
        let tail = &line[index + 1..];
        let at = skip_ws(tail, 0);
        if tail.as_bytes().get(at) != Some(&b'=') {
            continue;
        }
        if let Some((digits, _)) = digits_at(tail, skip_ws(tail, at + 1)) {
            return Some(ForkMatch {
                name: name.to_string(),
                child: parse_number(digits),
                resumed: true,
            });
        }
    }
    None
}

/// Read the value a whole line tail records after its closing
/// parenthesis, an equals sign and a whole number and nothing
/// else, the return shape a fork line ends with.
fn return_tail_value(text: &str) -> Option<Num> {
    let at = skip_ws(text, 0);
    if text.as_bytes().get(at) != Some(&b'=') {
        return None;
    }
    let (digits, end) = digits_at(text, skip_ws(text, at + 1))?;
    if skip_ws(text, end) != text.len() {
        return None;
    }
    Some(parse_number(digits))
}

/// Read the whole line shape of one of the exec calls.
///
/// Returns the index just after the last closing parenthesis
/// the line carries, the end the greedy call pattern settles
/// on, when the line names the call at its call position and
/// carries a closing parenthesis at all.
fn whole_exec_end(line: &str, wanted: &str) -> Option<usize> {
    let (name, _) = call_name_of(line)?;
    if name != wanted {
        return None;
    }
    line.rfind(')').map(|index| index + 1)
}

/// Return the flags text a matched call region ends with.
///
/// Some whole line patterns end where their flags argument
/// ends without capturing it, the plain open pattern and the
/// dup3 pattern among them. The flags text of such a call is
/// the trailing comma segment of the region its pattern
/// matched, so it is read from that region and the patterns
/// themselves stay the only statements of the call shapes. A
/// region carrying no trailing flags segment yields empty
/// text.
fn argument_flags_text(line: &str, end: usize) -> &str {
    let region = &line[..end];
    let Some(comma) = region.rfind(',') else {
        return "";
    };
    let rest = region[comma + 1..].trim_start_matches(is_ws);
    let run_end = flag_run_end(rest, 0);
    if run_end > 0 && run_end == rest.len() {
        &rest[..run_end]
    } else {
        ""
    }
}

/// Whether a bar joined flags text names the close on exec token
/// as its own token.
fn carries_cloexec(flags_text: &str) -> bool {
    flags_text.split('|').any(|token| token == "O_CLOEXEC")
}

/// One recorded descriptor entry.
///
/// The canonical resolution of the directory the number was
/// opened on, whether the spelled path landed inside the root
/// once its dot segments settle, and whether the entry
/// carries the close on exec flag.
#[derive(Debug, Clone, PartialEq)]
struct FdEntry {
    resolved: String,
    inside: bool,
    cloexec: bool,
}

/// A descriptor map, held in common where a clone shares it.
///
/// The map object is the sharing unit. A process key holds
/// one map, and a sharing fork hands the child key the same
/// map while a copying fork hands it a copy, so a change
/// through either key of a shared map lands for both.
type FdMap = Rc<RefCell<BTreeMap<Num, FdEntry>>>;

/// A directory cell, held in common where a clone shares it.
///
/// The cell is the sharing unit for the directory in force,
/// one directory per cell, moved by chdir and fchdir and
/// seeded by forks under the same sharing rule as the maps.
type CwdCell = Rc<RefCell<String>>;

fn cwd_cell(cwd_by_pid: &mut HashMap<PidKey, CwdCell>, pid: PidKey, root_path: &str) -> CwdCell {
    cwd_by_pid
        .entry(pid)
        .or_insert_with(|| Rc::new(RefCell::new(root_path.to_string())))
        .clone()
}

fn fds_map(fds_by_pid: &mut HashMap<PidKey, FdMap>, pid: PidKey) -> FdMap {
    fds_by_pid
        .entry(pid)
        .or_insert_with(|| Rc::new(RefCell::new(BTreeMap::new())))
        .clone()
}

/// Record what one successful open made of a descriptor number.
///
/// When the opened path resolves to a directory standing on
/// disk, the number names that directory in the map, stored
/// as its canonical resolution beside whether the spelled
/// path landed inside the root once its dot segments settle,
/// and beside whether the entry carries the close on exec
/// flag. Any other successful opening hands the number to
/// something the map does not track, so any earlier entry
/// for the number is dropped.
fn record_opened_descriptor(
    fds: &FdMap,
    raw_path: &str,
    base: &str,
    root: &str,
    fd: Num,
    cloexec: bool,
) {
    if !raw_path.is_empty() {
        let spelled = if raw_path.starts_with('/') {
            raw_path.to_string()
        } else {
            posix_join(base, raw_path)
        };
        let resolved = realpath(&spelled);
        if Path::new(&resolved).is_dir() {
            let written = normpath(&spelled);
            let inside = is_within(&written, root);
            fds.borrow_mut().insert(
                fd,
                FdEntry {
                    resolved,
                    inside,
                    cloexec,
                },
            );
            return;
        }
    }
    fds.borrow_mut().remove(&fd);
}

/// Give the target number whatever the source number names.
///
/// A recorded source hands its directory entry to the target,
/// with the close on exec flag the creating call gives its
/// product. An unrecorded source leaves the target naming
/// the same untracked thing, so any earlier entry for the
/// target is dropped.
fn copy_descriptor(fds: &FdMap, source: Num, target: Num, cloexec: bool) {
    let entry = fds.borrow().get(&source).cloned();
    let mut map = fds.borrow_mut();
    match entry {
        Some(entry) => {
            map.insert(
                target,
                FdEntry {
                    resolved: entry.resolved,
                    inside: entry.inside,
                    cloexec,
                },
            );
        }
        None => {
            map.remove(&target);
        }
    }
}

/// Set the close on exec flag of one recorded entry.
///
/// The entry keeps the directory it names and the inside fact
/// it carries, and only its flag changes, standing when the
/// bar joined tokens of the value name FD_CLOEXEC. A number
/// that holds no entry is left alone, so the call creates
/// nothing and adopts nothing.
fn reflag_descriptor(fds: &FdMap, number: Num, flags_text: &str) {
    let mut map = fds.borrow_mut();
    if let Some(entry) = map.get_mut(&number) {
        entry.cloexec = flags_text.split('|').any(|token| token == "FD_CLOEXEC");
    }
}

/// Forget every entry of a descriptor map that carries the flag.
///
/// A successful exec closes the flagged handles of its
/// process, so the entries that carry the close on exec flag
/// are removed from the map the process holds, in a shared
/// map for both processes that hold it, and every other
/// entry stands.
fn close_flagged_entries(fds: &FdMap) {
    let mut map = fds.borrow_mut();
    let flagged: Vec<Num> = map
        .iter()
        .filter(|(_, entry)| entry.cloexec)
        .map(|(number, _)| number.clone())
        .collect();
    for number in flagged {
        map.remove(&number);
    }
}

/// Move the state of a starting process under a survivor key.
///
/// From the banner onward the state under the survivor number
/// is the state the starting number held, adopted standing in
/// common with it and not copied. A starting number holding
/// no state yet hands the survivor a fresh map and a fresh
/// cell at the root.
fn adopt_starter_state(
    cwd_by_pid: &mut HashMap<PidKey, CwdCell>,
    fds_by_pid: &mut HashMap<PidKey, FdMap>,
    survivor: PidKey,
    starter: PidKey,
    root_path: &str,
) {
    let fds = match fds_by_pid.get(&starter) {
        Some(map) => map.clone(),
        None => Rc::new(RefCell::new(BTreeMap::new())),
    };
    fds_by_pid.insert(survivor.clone(), fds);
    let cell = match cwd_by_pid.get(&starter) {
        Some(cell) => cell.clone(),
        None => Rc::new(RefCell::new(root_path.to_string())),
    };
    cwd_by_pid.insert(survivor, cell);
}

/// Clear the halves and links one process key carried.
///
/// An exit line clears the halves and links its own process
/// carried and any link naming it, and a fork seeding clears
/// the same holdings under the child number it seeds.
fn clear_process_links(
    key: PidKey,
    pending: &mut HashMap<(PidKey, String), String>,
    clone_halves: &mut HashMap<(PidKey, String), (bool, bool)>,
    exec_halves: &mut HashMap<(PidKey, String), PidKey>,
    exec_links: &mut HashMap<PidKey, PidKey>,
) {
    pending.retain(|(pid, _), _| *pid != key);
    clone_halves.remove(&(key.clone(), "clone".to_string()));
    clone_halves.remove(&(key.clone(), "clone3".to_string()));
    exec_halves.remove(&(key.clone(), "execve".to_string()));
    exec_halves.remove(&(key.clone(), "execveat".to_string()));
    exec_links.remove(&key);
    exec_links.retain(|_, value| *value != key);
}

fn absolute_root(root: &Path) -> String {
    let text = root.to_string_lossy().into_owned();
    if text.starts_with('/') {
        return text;
    }
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{}/{text}", cwd.trim_end_matches('/'))
}

/// Return the read set and the outside reads found in a log text.
///
/// The read set is the sorted list of resolved paths inside the
/// root. The outside reads are the sorted list of resolved
/// absolute paths that landed outside the root. When the shape
/// switch holds, a non empty text with no recognizable strace
/// line is refused before any read set is built.
///
/// The reading is the trace reading whole, the slice 1 reading
/// as the later fixes amended it and the state machinery this
/// slice adds. Directory state is tracked while the lines are
/// read. An exit line drops the directory state and the
/// descriptor map standing under its process's key beside the
/// halves and links it already cleared, so a number the kernel
/// hands to a later tenant reads with fresh state, while a
/// holding another process shares stands under that process's
/// key untouched. An exit line standing while an exec half
/// is kept under its process is the exec's own dance and drops
/// no map, as the amendment on the slice 16 obligations
/// states. Every process starts at the root, a chdir line with a
/// recorded success moves the directory of its own process from
/// that line onward, and a fork the trace records seeds the
/// child with the directory and the descriptor map its parent
/// held at that line, held in common with the parent when the
/// flags the fork line prints carry CLONE_FILES for the map or
/// CLONE_FS for the directory, whether those flags stand in a
/// clone flags argument or in the flags field of a decoded
/// clone3 struct, and copied otherwise. Descriptor state is
/// tracked beside it. An open or openat line that returns a
/// descriptor for a path resolving to a directory on disk
/// records that directory for the number, a close line with a
/// recorded success forgets the number, and the duplicating
/// calls and the duplicating fcntl shapes copy an entry to the
/// number they create, never to a spelled target or floor the
/// kernel did not hand out. An fcntl line in the set flags
/// shape moves the flag of a recorded entry and creates
/// nothing. Each entry carries whether it holds the close on
/// exec flag, and a successful exec closes the flagged entries
/// of its process, in the shared map itself when the map is
/// held in common. Split lines are reassembled while the lines
/// are read, the plain calls and the exec family alike, and a
/// pair a thread began completes under the leader number its
/// trace names through the superseded banner or the pid
/// changed tail, the state under the leader number adopted
/// standing in common with the starting number from the
/// banner onward. A bracketed process id prefix is rewritten
/// to the bare spelling as each line enters the parse, so a
/// bracketed line is judged with the state of the process its
/// bracket names exactly as its bare spelling is judged.
pub fn parse_trace(text: &str, root: &Path) -> Result<TraceRead, BlindgateError> {
    parse_trace_with_switch(text, root, true)
}

/// The parse entry with its shape refusal switch standing open.
///
/// A capture the tool makes itself is produced by strace and is
/// judged with no shape check standing between the capture and
/// the read set, which is the switch position the Python parse
/// entry carries for its own captures.
pub fn parse_trace_with_switch(
    text: &str,
    root: &Path,
    check_shape: bool,
) -> Result<TraceRead, BlindgateError> {
    let root_path = realpath(&absolute_root(root));
    if check_shape {
        refuse_unshaped_trace(text, "log")?;
    }
    let mut reads = BTreeSet::new();
    let mut outside = BTreeSet::new();
    let mut cwd_by_pid: HashMap<PidKey, CwdCell> = HashMap::new();
    let mut fds_by_pid: HashMap<PidKey, FdMap> = HashMap::new();
    let mut pending: HashMap<(PidKey, String), String> = HashMap::new();
    let mut clone_halves: HashMap<(PidKey, String), (bool, bool)> = HashMap::new();
    let mut exec_halves: HashMap<(PidKey, String), PidKey> = HashMap::new();
    let mut exec_links: HashMap<PidKey, PidKey> = HashMap::new();
    for spelled_line in split_lines(text) {
        let raw_line = bare_pid_form(spelled_line);
        if let Some((name, prefix)) = kept_unfinished(&raw_line) {
            pending.insert((pid_key(&raw_line), name), prefix);
            continue;
        }
        if let Some(sharing) = kept_clone_half(&raw_line) {
            clone_halves.insert((pid_key(&raw_line), "clone".to_string()), sharing);
            continue;
        }
        if let Some(sharing) = kept_clone3_half(&raw_line) {
            clone_halves.insert((pid_key(&raw_line), "clone3".to_string()), sharing);
            continue;
        }
        if let Some((starter, survivor, call)) = kept_exec_half(&raw_line) {
            exec_halves.insert((starter.clone(), call.to_string()), survivor.clone());
            exec_links.insert(survivor, starter);
            continue;
        }
        let line = spend_resumed(&raw_line, &mut pending);
        let pid = pid_key(&line);
        if is_exit_line(&line) {
            let exec_kept = exec_halves.contains_key(&(pid.clone(), "execve".to_string()))
                || exec_halves.contains_key(&(pid.clone(), "execveat".to_string()));
            clear_process_links(
                pid.clone(),
                &mut pending,
                &mut clone_halves,
                &mut exec_halves,
                &mut exec_links,
            );
            if !exec_kept {
                cwd_by_pid.remove(&pid);
                fds_by_pid.remove(&pid);
            }
            continue;
        }
        if let Some((survivor, starter)) = superseded_match(&line) {
            let survivor_key = Some(survivor);
            let starter_key = Some(starter);
            exec_links.insert(survivor_key.clone(), starter_key.clone());
            adopt_starter_state(
                &mut cwd_by_pid,
                &mut fds_by_pid,
                survivor_key.clone(),
                starter_key,
                &root_path,
            );
            pending.retain(|(owner, _), _| *owner != survivor_key);
            clone_halves.remove(&(survivor_key.clone(), "clone".to_string()));
            clone_halves.remove(&(survivor_key.clone(), "clone3".to_string()));
            exec_halves.remove(&(survivor_key.clone(), "execve".to_string()));
            exec_halves.remove(&(survivor_key, "execveat".to_string()));
            continue;
        }
        if let Some(fork) = fork_match(&line) {
            if !fork.child.is_zero() {
                let child = Some(fork.child);
                let (share_files, share_fs) =
                    if fork.resumed && (fork.name == "clone" || fork.name == "clone3") {
                        clone_halves
                            .remove(&(pid.clone(), fork.name.clone()))
                            .unwrap_or((false, false))
                    } else if fork.resumed {
                        (false, false)
                    } else {
                        clone_halves.remove(&(pid.clone(), "clone".to_string()));
                        clone_halves.remove(&(pid.clone(), "clone3".to_string()));
                        fork_line_sharing(&line, &fork.name)
                    };
                let parent_cell = cwd_cell(&mut cwd_by_pid, pid.clone(), &root_path);
                if share_fs {
                    cwd_by_pid.insert(child.clone(), parent_cell.clone());
                } else {
                    let standing = parent_cell.borrow().clone();
                    cwd_by_pid.insert(child.clone(), Rc::new(RefCell::new(standing)));
                }
                let parent_fds = fds_map(&mut fds_by_pid, pid);
                if share_files {
                    fds_by_pid.insert(child.clone(), parent_fds.clone());
                } else {
                    let snapshot = parent_fds.borrow().clone();
                    fds_by_pid.insert(child.clone(), Rc::new(RefCell::new(snapshot)));
                }
                clear_process_links(
                    child,
                    &mut pending,
                    &mut clone_halves,
                    &mut exec_halves,
                    &mut exec_links,
                );
                continue;
            }
        }
        if let Some((name, end)) = resumed_marker(&line) {
            if EXEC_CALLS.contains(&name) {
                let starter = if exec_halves.contains_key(&(pid.clone(), name.to_string())) {
                    pid.clone()
                } else {
                    exec_links.get(&pid).cloned().flatten()
                };
                if let Some(starter_pid) = starter {
                    let starter_key = Some(starter_pid);
                    if exec_halves.contains_key(&(starter_key.clone(), name.to_string())) {
                        exec_halves.remove(&(starter_key.clone(), name.to_string()));
                        exec_links.remove(&pid);
                        if starter_key != pid {
                            adopt_starter_state(
                                &mut cwd_by_pid,
                                &mut fds_by_pid,
                                pid.clone(),
                                starter_key,
                                &root_path,
                            );
                        }
                        let tail = &line[end..];
                        let region = match tail.rfind(')') {
                            Some(closing) => &tail[closing + 1..],
                            None => tail,
                        };
                        if let Some(value) = find_return(region) {
                            if !value.is_negative() {
                                if let Some(fds) = fds_by_pid.get(&pid) {
                                    close_flagged_entries(fds);
                                }
                            }
                        }
                    }
                }
                continue;
            }
        }
        if let Some(end) = whole_exec_end(&line, "execve") {
            if !line_is_unfinished(&line) {
                if let Some(value) = find_return(&line[end..]) {
                    if !value.is_negative() {
                        if let Some(fds) = fds_by_pid.get(&pid) {
                            close_flagged_entries(fds);
                        }
                    }
                }
                continue;
            }
        }
        if let Some(end) = whole_exec_end(&line, "execveat") {
            if !line_is_unfinished(&line) {
                if let Some(value) = find_return(&line[end..]) {
                    if !value.is_negative() {
                        if let Some(fds) = fds_by_pid.get(&pid) {
                            close_flagged_entries(fds);
                        }
                    }
                }
                continue;
            }
        }
        if let Some((raw, end)) = match_chdir(&line) {
            if let Some(value) = find_return(&line[end..]) {
                if !value.is_negative() {
                    let target = unescape(raw);
                    if !target.is_empty() {
                        let cell = cwd_cell(&mut cwd_by_pid, pid, &root_path);
                        let candidate = if target.starts_with('/') {
                            target
                        } else {
                            let standing = cell.borrow().clone();
                            posix_join(&standing, &target)
                        };
                        *cell.borrow_mut() = realpath(&candidate);
                    }
                }
            }
            continue;
        }
        if let Some((number, end)) = match_number_call(&line, "fchdir") {
            if let Some(value) = find_return(&line[end..]) {
                if !value.is_negative() {
                    let entry = fds_by_pid
                        .get(&pid)
                        .and_then(|fds| fds.borrow().get(&number).cloned());
                    if let Some(entry) = entry {
                        let cell = cwd_cell(&mut cwd_by_pid, pid, &root_path);
                        *cell.borrow_mut() = entry.resolved;
                    }
                }
            }
            continue;
        }
        if let Some((number, end)) = match_number_call(&line, "close") {
            if let Some(value) = find_return(&line[end..]) {
                if !value.is_negative() {
                    if let Some(fds) = fds_by_pid.get(&pid) {
                        fds.borrow_mut().remove(&number);
                    }
                }
            }
            continue;
        }
        if let Some((source, end)) = match_number_call(&line, "dup") {
            if let Some(value) = find_return(&line[end..]) {
                if !value.is_negative() {
                    let fds = fds_map(&mut fds_by_pid, pid);
                    copy_descriptor(&fds, source, value, false);
                }
            }
            continue;
        }
        let dup_pair = match_dup2(&line)
            .map(|(source, target, end)| (source, target, end, false))
            .or_else(|| match_dup3(&line).map(|(source, target, end)| (source, target, end, true)));
        if let Some((source, target, end, is_dup3)) = dup_pair {
            if let Some(value) = find_return(&line[end..]) {
                if !value.is_negative() {
                    let cloexec = is_dup3 && carries_cloexec(argument_flags_text(&line, end));
                    let fds = fds_map(&mut fds_by_pid, pid);
                    copy_descriptor(&fds, source, target, cloexec);
                }
            }
            continue;
        }
        if let Some((fd, args, end)) = match_fcntl(&line) {
            if let Some(value) = find_return(&line[end..]) {
                if !value.is_negative() {
                    match args {
                        FcntlArgs::SetFd(flags_text) => {
                            if let Some(fds) = fds_by_pid.get(&pid) {
                                reflag_descriptor(fds, fd, &flags_text);
                            }
                        }
                        FcntlArgs::DupFd { cloexec } => {
                            let fds = fds_map(&mut fds_by_pid, pid);
                            copy_descriptor(&fds, fd, value, cloexec);
                        }
                    }
                }
            }
            continue;
        }
        if let Some((raw, end)) = match_open(&line) {
            if let Some(value) = find_return(&line[end..]) {
                if !value.is_negative() {
                    let fds = fds_map(&mut fds_by_pid, pid.clone());
                    let base = cwd_cell(&mut cwd_by_pid, pid, &root_path).borrow().clone();
                    let cloexec = carries_cloexec(argument_flags_text(&line, end));
                    record_opened_descriptor(
                        &fds,
                        &unescape(raw),
                        &base,
                        &root_path,
                        value,
                        cloexec,
                    );
                }
            }
            continue;
        }
        if let Some(matched) = find_openat(&line).or_else(|| find_openat2(&line)) {
            judge_open_call(
                &matched,
                &line,
                pid,
                &mut fds_by_pid,
                &mut cwd_by_pid,
                &root_path,
                &mut reads,
                &mut outside,
            );
        }
    }
    Ok(TraceRead {
        reads: reads.into_iter().collect(),
        outside_reads: outside.into_iter().collect(),
    })
}

/// Judge one open family call the matcher has read, openat
/// or openat2 standing in one match under S17-1, so the two
/// calls share this one judgement and no twin of it stands.
/// The descriptor the call names resolves against its entry
/// or against the directory in force, a successful return
/// records a descriptor where the target stands as a
/// directory, and a read the flags earn enters the read set
/// or the outside set under the scoped canonical reading.
#[allow(clippy::too_many_arguments)]
fn judge_open_call(
    matched: &OpenatMatch<'_>,
    line: &str,
    pid: PidKey,
    fds_by_pid: &mut HashMap<PidKey, FdMap>,
    cwd_by_pid: &mut HashMap<PidKey, CwdCell>,
    root_path: &str,
    reads: &mut BTreeSet<String>,
    outside: &mut BTreeSet<String>,
) {
    let dirfd_token = matched.dirfd.trim_matches(is_ws);
    let raw_path = unescape(matched.raw_path);
    let fds = fds_map(fds_by_pid, pid.clone());
    let mut entry: Option<FdEntry> = None;
    let base = if dirfd_token == "AT_FDCWD" {
        cwd_cell(cwd_by_pid, pid, root_path).borrow().clone()
    } else if is_digit_token(dirfd_token) && fds.borrow().contains_key(&parse_number(dirfd_token)) {
        let found = fds
            .borrow()
            .get(&parse_number(dirfd_token))
            .cloned()
            .expect("the entry stands under the number just checked");
        let resolved = found.resolved.clone();
        entry = Some(found);
        resolved
    } else {
        cwd_cell(cwd_by_pid, pid, root_path).borrow().clone()
    };
    let returned = find_return(&line[matched.end..]);
    if let Some(value) = &returned {
        if !value.is_negative() {
            record_opened_descriptor(
                &fds,
                &raw_path,
                &base,
                root_path,
                value.clone(),
                carries_cloexec(matched.flags),
            );
        }
    }
    if !is_read(matched.flags) {
        return;
    }
    let Some(value) = returned else {
        return;
    };
    if value.is_negative() {
        return;
    }
    let (relative, outside_path) = canonical_scoped_based(&raw_path, root_path, Some(&base));
    if let Some(relative) = relative {
        reads.insert(relative);
    } else if let Some(outside_path) = outside_path {
        outside.insert(outside_path);
    } else if let Some(entry) = &entry {
        let through_inside_entry =
            entry.inside && !raw_path.is_empty() && !raw_path.starts_with('/');
        if through_inside_entry {
            let spelled = posix_join(&base, &raw_path);
            let resolved = realpath(&spelled);
            if resolved != root_path
                && resolved != realpath(&base)
                && relative_to(&resolved, root_path).is_none()
            {
                outside.insert(resolved);
            }
        }
    }
}

/// Return the sorted read set found in an strace log text.
///
/// The read set is one half of the pair form, kept under its
/// slice 1 name.
pub fn parse_strace_log(text: &str, root: &Path) -> Result<Vec<String>, BlindgateError> {
    Ok(parse_trace(text, root)?.reads)
}

fn is_digit_token(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_reads_the_strace_spellings() {
        assert_eq!(unescape("plain"), "plain");
        assert_eq!(unescape("a\\nb"), "a\nb");
        assert_eq!(unescape("a\\tb"), "a\tb");
        assert_eq!(unescape("a\\\"b"), "a\"b");
        assert_eq!(unescape("a\\\\b"), "a\\b");
        assert_eq!(unescape("a\\rb"), "arb");
        assert_eq!(unescape("a\\"), "a\\");
    }

    #[test]
    fn read_flags_follow_the_content_rule() {
        assert!(is_read("O_RDONLY"));
        assert!(is_read("O_RDWR|O_CLOEXEC"));
        assert!(!is_read("O_WRONLY|O_CREAT|O_TRUNC"));
        assert!(!is_read("O_PATH"));
        assert!(!is_read("O_DIRECTORY|O_RDONLY"));
        assert!(!is_read("O_CREAT"));
    }

    #[test]
    fn returns_read_whole_tokens_in_decimal_and_hex() {
        assert_eq!(find_return(") = 3"), Some(Num::from(3)));
        assert_eq!(
            find_return(") = -1 ENOENT (No such file or directory)"),
            Some(Num::from(-1))
        );
        assert_eq!(find_return(") = 0x3"), Some(Num::from(3)));
        assert_eq!(
            find_return(") = -0x1 ENOENT (No such file or directory)"),
            Some(Num::from(-1))
        );
        assert_eq!(find_return(" <unfinished ...>"), None);
        assert_eq!(find_return(""), None);
        assert_eq!(find_return(") = ?"), None);
    }

    #[test]
    fn leading_zero_returns_read_as_the_decimal_they_spell() {
        assert_eq!(find_return(") = 010"), Some(Num::from(10)));
        assert_eq!(find_return(") = 00"), Some(Num::from(0)));
    }

    #[test]
    fn numbers_past_the_machine_word_stay_distinct_and_exact() {
        let word = Num::from_decimal(false, "9223372036854775807");
        let past = Num::from_decimal(false, "9223372036854775808");
        assert_ne!(past, word);
        assert!(past > word);
        assert_eq!(find_return(") = 9223372036854775808"), Some(past.clone()));
        assert_eq!(parse_number("9223372036854775808"), past);
        assert_ne!(
            parse_number("9223372036854775808"),
            parse_number("9223372036854775807")
        );
        assert_eq!(parse_number("09223372036854775808"), past);
        assert_eq!(
            find_return(") = 0x10000000000000000"),
            Some(Num::from_decimal(false, "18446744073709551616"))
        );
        assert_eq!(
            find_return(") = -9223372036854775809"),
            Some(Num::from_decimal(true, "9223372036854775809"))
        );
        assert!(Num::from_decimal(true, "9223372036854775809") < Num::from(i64::MIN));
        assert_eq!(find_return(") = -0"), Some(Num::from(0)));
    }

    #[test]
    fn whitespace_only_descriptor_segment_still_reads() {
        let blank = find_openat("4321 openat( , \"src/app.py\", O_RDONLY) = 3")
            .expect("a blank segment holds the shape");
        assert_eq!(blank.raw_path, "src/app.py");
        let tabbed = find_openat("4321 openat(\t, \"src/app.py\", O_RDONLY) = 3")
            .expect("a tab segment holds the shape");
        assert_eq!(tabbed.raw_path, "src/app.py");
        assert!(find_openat("4321 openat(, \"src/app.py\", O_RDONLY) = 3").is_none());
    }

    #[test]
    fn unit_separator_before_a_return_still_reads() {
        assert_eq!(find_return(") =\u{001F}3"), Some(Num::from(3)));
    }

    #[test]
    fn openat_shape_needs_its_whole_call() {
        let line = "4321 openat(AT_FDCWD, \"src/app.py\", O_RDONLY) = 3";
        let matched = find_openat(line).expect("a whole call reads");
        assert_eq!(matched.raw_path, "src/app.py");
        assert_eq!(matched.flags, "O_RDONLY");
        assert!(find_openat("4321 openat(AT_FDCWD, src/app.py, O_RDONLY) = 3").is_none());
        assert!(find_openat("4321 openat(AT_FDCWD, \"src/app.py\") = 3").is_none());
        assert!(find_openat("4321 read(3, \"payload\", 7) = 7").is_none());
    }

    #[test]
    fn pid_changed_tail_names_the_completion_number() {
        assert_eq!(
            pid_changed_tail(
                "4321 execve(\"/bin/x\", [\"x\"], 0x55 /* 12 vars */ <pid changed to 4322 ...>"
            ),
            Some(Num::from(4322))
        );
        assert_eq!(pid_changed_tail("4321 execve(\"/bin/x\") = 0"), None);
        assert_eq!(
            pid_changed_tail("4321 execve(\"/bin/x\" <pid changed to 4322 ...> trailing"),
            None
        );
    }

    #[test]
    fn argument_flags_text_reads_the_trailing_segment() {
        let line = "4321 dup3(3, 4, O_CLOEXEC) = 4";
        let (_, _, end) = match_dup3(line).expect("the dup3 shape reads");
        assert_eq!(argument_flags_text(line, end), "O_CLOEXEC");
        let line = "4321 open(\"docs\", O_RDONLY|O_CLOEXEC) = 3";
        let (_, end) = match_open(line).expect("the open shape reads");
        assert_eq!(argument_flags_text(line, end), "O_RDONLY|O_CLOEXEC");
        let line = "4321 dup3(3, 4) = 4";
        let (_, _, end) = match_dup3(line).expect("the flagless dup3 shape reads");
        assert_eq!(argument_flags_text(line, end), "4");
    }

    #[test]
    fn fcntl_shapes_read_by_command() {
        let line = "4321 fcntl(3, F_SETFD, FD_CLOEXEC) = 0";
        let (fd, args, _) = match_fcntl(line).expect("the set flags shape reads");
        assert_eq!(fd, Num::from(3));
        assert!(matches!(args, FcntlArgs::SetFd(ref text) if text == "FD_CLOEXEC"));
        let line = "4321 fcntl(3, F_DUPFD_CLOEXEC, 0) = 4";
        let (_, args, _) = match_fcntl(line).expect("the cloexec dup shape reads");
        assert!(matches!(args, FcntlArgs::DupFd { cloexec: true }));
        let line = "4321 fcntl(3, F_DUPFD, 2) = 5";
        let (_, args, _) = match_fcntl(line).expect("the dup shape reads");
        assert!(matches!(args, FcntlArgs::DupFd { cloexec: false }));
        assert!(match_fcntl("4321 fcntl(3, F_GETFD) = 0").is_none());
        assert!(match_fcntl("4321 fcntl(3, F_DUPFD, FD_CLOEXEC) = 4").is_none());
    }

    #[test]
    fn kept_halves_follow_the_shape_rules() {
        let kept = kept_unfinished("4321 chdir(\"docs\" <unfinished ...>")
            .expect("a whole argument half is kept");
        assert_eq!(kept.0, "chdir");
        assert_eq!(kept.1, "4321 chdir(\"docs\" ");
        assert!(kept_unfinished("4321 chdir(\"do <unfinished ...>").is_none());
        assert!(kept_unfinished("4321 read(3, \"x\" <unfinished ...>").is_none());
        let kept = kept_unfinished("4321 fcntl(3, F_DUPFD, 0 <unfinished ...>")
            .expect("an fcntl half is kept");
        assert_eq!(kept.0, "fcntl");
    }

    #[test]
    fn clone3_sharing_reads_only_the_struct_flags_at_the_top_level() {
        let line = "4321 clone3({flags=CLONE_FILES|CLONE_FS, exit_signal=SIGCHLD} => {parent_tid=[4322]}, 88) = 4322";
        assert_eq!(clone3_line_sharing(line), (true, true));
        let line =
            "4321 clone3({flags=0, stack={flags=CLONE_FILES}} => {parent_tid=[4322]}, 88) = 4322";
        assert_eq!(clone3_line_sharing(line), (false, false));
        let line = "4321 clone3(0x7f00 => {flags=CLONE_FILES}, 88) = 4322";
        assert_eq!(clone3_line_sharing(line), (false, false));
    }

    #[test]
    fn superseded_banner_reads_both_numbers() {
        assert_eq!(
            superseded_match("4322 +++ superseded by execve in pid 4321 +++"),
            Some((Num::from(4322), Num::from(4321)))
        );
        assert_eq!(
            superseded_match("+++ superseded by execve in pid 4321 +++"),
            None
        );
    }
}
