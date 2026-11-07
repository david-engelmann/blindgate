use std::collections::BTreeSet;
use std::path::Path;

use crate::classify::{bare_pid_form, refuse_unshaped_trace, split_lines};
use crate::errors::BlindgateError;
use crate::model::TraceRead;
use crate::scope::{canonical_scoped, realpath};

const READ_FLAGS: [&str; 2] = ["O_RDONLY", "O_RDWR"];
const NON_CONTENT_FLAGS: [&str; 2] = ["O_PATH", "O_DIRECTORY"];

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

struct OpenatMatch<'a> {
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

/// Read one recorded return token, in decimal or in hex.
///
/// The token may carry a minus sign on either form, the reading
/// the recorded return rule states. A magnitude too large for
/// the machine word saturates, which cannot move a judgement,
/// because a return is only ever compared against zero here.
fn return_token(text: &str) -> Option<i64> {
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
            let magnitude = i64::from_str_radix(&hex[..count], 16).unwrap_or(i64::MAX);
            return Some(if negative { -magnitude } else { magnitude });
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
    let magnitude: i64 = digits[..count].parse().unwrap_or(i64::MAX);
    Some(if negative { -magnitude } else { magnitude })
}

/// Find the first recorded return a line carries after a point.
///
/// An equals sign that no return token follows is passed over,
/// and the search settles on the first equals sign a whole
/// number token does follow, the reading the Python return
/// pattern gives the tail of a call line.
fn find_return(rest: &str) -> Option<i64> {
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
/// The reading is the slice 1 reading as the later fixes
/// amended it. An openat line counts as a read when its flags
/// open the path for reading and carry no non content flag, and
/// when a recorded return stands after the call and is not a
/// failure. An openat line with no recorded return counts as no
/// read. A bracketed process id prefix is rewritten to the bare
/// spelling as each line enters the parse, so a bracketed line
/// is judged exactly as its bare spelling is judged.
///
/// The state readings the later slices added stand outside
/// this entry, as the contract port section states. Directory
/// state, descriptor state, split line reassembly, fork seeding
/// and sharing, the exec closings, and live capture remain with
/// the Python in this slice.
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
    for spelled_line in split_lines(text) {
        let line = bare_pid_form(spelled_line);
        let Some(matched) = find_openat(&line) else {
            continue;
        };
        if !is_read(matched.flags) {
            continue;
        }
        let Some(returned) = find_return(&line[matched.end..]) else {
            continue;
        };
        if returned < 0 {
            continue;
        }
        let (relative, outside_path) = canonical_scoped(&unescape(matched.raw_path), &root_path);
        if let Some(relative) = relative {
            reads.insert(relative);
        } else if let Some(outside_path) = outside_path {
            outside.insert(outside_path);
        }
    }
    Ok(TraceRead {
        reads: reads.into_iter().collect(),
        outside_reads: outside.into_iter().collect(),
    })
}

/// Return the sorted read set found in an strace log text.
///
/// The read set is one half of the pair form, kept under its
/// slice 1 name.
pub fn parse_strace_log(text: &str, root: &Path) -> Result<Vec<String>, BlindgateError> {
    Ok(parse_trace(text, root)?.reads)
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
        assert_eq!(find_return(") = 3"), Some(3));
        assert_eq!(
            find_return(") = -1 ENOENT (No such file or directory)"),
            Some(-1)
        );
        assert_eq!(find_return(") = 0x3"), Some(3));
        assert_eq!(
            find_return(") = -0x1 ENOENT (No such file or directory)"),
            Some(-1)
        );
        assert_eq!(find_return(" <unfinished ...>"), None);
        assert_eq!(find_return(""), None);
        assert_eq!(find_return(") = ?"), None);
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
        assert_eq!(find_return(") =\u{001F}3"), Some(3));
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
}
