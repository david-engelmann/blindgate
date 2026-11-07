use crate::errors::BlindgateError;

const MARKERS: [&str; 3] = ["<unfinished ...>", "--- ", "+++ "];

fn is_ws(c: char) -> bool {
    // The whitespace class of the Python patterns counts the
    // file, group, record, and unit separators as whitespace
    // beside the Unicode white space the Rust class already
    // holds, so the matchers read all four as whitespace too.
    c.is_whitespace() || ('\u{001C}'..='\u{001F}').contains(&c)
}

/// Split a trace text into its lines the way the Python splits it.
///
/// The Python reads trace text with splitlines, which breaks a
/// line at the newline and also at a bare carriage return, a
/// vertical tab, a form feed, the file, group, and record
/// separators, the next line character, and the Unicode line
/// and paragraph separators, with a carriage return and newline
/// pair read as one boundary. A text that ends on a boundary
/// holds no line after it, and an empty text holds no line.
pub fn split_lines(text: &str) -> Vec<&str> {
    fn is_boundary(c: char) -> bool {
        matches!(
            c,
            '\n' | '\r'
                | '\u{000B}'
                | '\u{000C}'
                | '\u{001C}'
                | '\u{001D}'
                | '\u{001E}'
                | '\u{0085}'
                | '\u{2028}'
                | '\u{2029}'
        )
    }
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if !is_boundary(c) {
            continue;
        }
        lines.push(&text[start..index]);
        let mut end = index + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
            chars.next();
            end += 1;
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Return the line with its pid prefix in the bare spelling.
///
/// Strace spells the process id prefix of a line as a bare number
/// in a file capture and in brackets on its standard error stream,
/// and the text after the prefix is the same in both spellings.
/// A line with no bracketed prefix comes back unchanged, and
/// bracketed text anywhere but the start of a line is data this
/// rewrite never touches.
pub fn bare_pid_form(line: &str) -> String {
    let Some(after_pid) = line.strip_prefix("[pid") else {
        return line.to_string();
    };
    let spaces = after_pid.len() - after_pid.trim_start_matches(' ').len();
    if spaces == 0 {
        return line.to_string();
    }
    let after_spaces = &after_pid[spaces..];
    let digits = after_spaces.len()
        - after_spaces
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .len();
    if digits == 0 {
        return line.to_string();
    }
    let Some(tail) = after_spaces[digits..].strip_prefix("] ") else {
        return line.to_string();
    };
    format!("{} {}", &after_spaces[..digits], tail)
}

/// Return True when a trace text holds a recognizable strace line.
///
/// A line is recognizable when it carries one of the usual strace
/// markers, the unfinished marker, the signal banner, or the exit
/// marker, or when the whole line is a syscall shape, an optional
/// process id in either of its spellings, a syscall name hard
/// against its opening parenthesis, the call, an equals sign, and
/// a return value, where the return value is a number or the
/// question mark strace prints when a return is unknown, with an
/// optional error name and message after it. A call shape buried
/// inside a longer line of prose or source code does not make the
/// line recognizable. An empty text holds no line at all and is
/// not judged by this check.
pub fn looks_like_strace(text: &str) -> bool {
    for line in split_lines(text) {
        if MARKERS.iter().any(|marker| line.contains(marker)) {
            return true;
        }
        if strace_line_shape(&bare_pid_form(line)) {
            return true;
        }
    }
    false
}

/// Refuse a handed over trace that is not strace shaped.
///
/// An empty text keeps the slice 1 meaning and passes. A non
/// empty text with no recognizable strace line is refused with a
/// named error, so a garbage file can no longer read as a genuine
/// trace that happened to record no reads.
pub fn refuse_unshaped_trace(text: &str, origin: &str) -> Result<(), BlindgateError> {
    if !text.is_empty() && !looks_like_strace(text) {
        return Err(BlindgateError::new(format!(
            "trace {origin} does not look like strace output"
        )));
    }
    Ok(())
}

fn name_len(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
        return None;
    }
    let mut end = 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    Some(end)
}

fn ws_prefix(s: &str) -> Option<&str> {
    let trimmed = s.trim_start_matches(is_ws);
    if trimmed.len() == s.len() {
        None
    } else {
        Some(trimmed)
    }
}

fn strip_pid_prefix(s: &str) -> Option<&str> {
    let digits = s.len() - s.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    ws_prefix(&s[digits..])
}

fn resumed_shape(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("<...") else {
        return false;
    };
    let Some(rest) = ws_prefix(rest) else {
        return false;
    };
    let Some(name) = name_len(rest) else {
        return false;
    };
    let Some(rest) = ws_prefix(&rest[name..]) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix("resumed>") else {
        return false;
    };
    rest.chars().all(is_ws)
}

fn return_token_len(t: &str) -> Option<usize> {
    let bytes = t.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if bytes[0] == b'?' {
        return Some(1);
    }
    if bytes[0] == b'-' {
        let mut end = 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == 1 {
            return None;
        }
        return Some(end);
    }
    if t.starts_with("0x") {
        let hex = &t[2..];
        let digits = hex.len()
            - hex
                .trim_start_matches(|c: char| c.is_ascii_hexdigit())
                .len();
        if digits > 0 {
            return Some(2 + digits);
        }
    }
    if bytes[0].is_ascii_digit() {
        let mut end = 0;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        return Some(end);
    }
    None
}

fn return_tail(t: &str) -> bool {
    let t = t.trim_start_matches(is_ws);
    let Some(t) = t.strip_prefix('=') else {
        return false;
    };
    let t = t.trim_start_matches(is_ws);
    let Some(token) = return_token_len(t) else {
        return false;
    };
    let rest = &t[token..];
    if rest.chars().all(is_ws) {
        return true;
    }
    let Some(rest) = ws_prefix(rest) else {
        return false;
    };
    let bytes = rest.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_uppercase() {
        return false;
    }
    let mut end = 1;
    while end < bytes.len()
        && (bytes[end].is_ascii_uppercase() || bytes[end].is_ascii_digit() || bytes[end] == b'_')
    {
        end += 1;
    }
    let after_name = &rest[end..];
    if after_name.chars().all(is_ws) {
        return true;
    }
    let Some(rest) = ws_prefix(after_name) else {
        return false;
    };
    if !rest.starts_with('(') {
        return false;
    }
    rest.trim_end_matches(is_ws).ends_with(')')
}

fn syscall_shape(s: &str) -> bool {
    let Some(name) = name_len(s) else {
        return false;
    };
    let rest = &s[name..];
    if !rest.starts_with('(') {
        return false;
    }
    // The pattern between the parenthesis pair is greedy, so the
    // closing parenthesis the shape settles on is the last one
    // whose tail still reads as a return.
    for (index, c) in rest.char_indices().rev() {
        if c == ')' && return_tail(&rest[index + 1..]) {
            return true;
        }
    }
    false
}

fn shape_body(s: &str) -> bool {
    resumed_shape(s) || syscall_shape(s)
}

fn strace_line_shape(line: &str) -> bool {
    let s = line.trim_start_matches(is_ws);
    if let Some(rest) = strip_pid_prefix(s) {
        if shape_body(rest) {
            return true;
        }
    }
    shape_body(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_form_rewrites_a_padded_bracket() {
        assert_eq!(
            bare_pid_form("[pid   432] openat(AT_FDCWD, \"x\", O_RDONLY) = 3"),
            "432 openat(AT_FDCWD, \"x\", O_RDONLY) = 3"
        );
    }

    #[test]
    fn bare_form_leaves_other_lines_alone() {
        assert_eq!(
            bare_pid_form("openat(AT_FDCWD, \"x\") = 3"),
            "openat(AT_FDCWD, \"x\") = 3"
        );
        assert_eq!(
            bare_pid_form("openat(AT_FDCWD, \"[pid 9] x\", O_RDONLY) = 3"),
            "openat(AT_FDCWD, \"[pid 9] x\", O_RDONLY) = 3"
        );
        assert_eq!(bare_pid_form("[pid] openat() = 3"), "[pid] openat() = 3");
        assert_eq!(
            bare_pid_form("[pid 12]openat() = 3"),
            "[pid 12]openat() = 3"
        );
    }

    #[test]
    fn resumed_line_with_a_return_is_not_a_shape() {
        assert!(!strace_line_shape(
            "4321 <... openat resumed>\"src/app.py\", O_RDONLY) = 3"
        ));
        assert!(strace_line_shape("4321 <... openat resumed>"));
    }

    #[test]
    fn prose_with_a_buried_call_is_not_a_shape() {
        assert!(!strace_line_shape(
            "the function read(file) = 3 times today"
        ));
        assert!(strace_line_shape("99 getpid() = 99"));
    }

    #[test]
    fn split_lines_breaks_on_the_python_boundaries() {
        assert_eq!(split_lines("a\nb"), ["a", "b"]);
        assert_eq!(split_lines("a\rb"), ["a", "b"]);
        assert_eq!(split_lines("a\r\nb"), ["a", "b"]);
        assert_eq!(split_lines("a\u{000B}b"), ["a", "b"]);
        assert_eq!(split_lines("a\u{000C}b"), ["a", "b"]);
        assert_eq!(split_lines("a\u{001C}b"), ["a", "b"]);
        assert_eq!(split_lines("a\u{001D}b"), ["a", "b"]);
        assert_eq!(split_lines("a\u{001E}b"), ["a", "b"]);
        assert_eq!(split_lines("a\u{0085}b"), ["a", "b"]);
        assert_eq!(split_lines("a\u{2028}b"), ["a", "b"]);
        assert_eq!(split_lines("a\u{2029}b"), ["a", "b"]);
    }

    #[test]
    fn split_lines_holds_the_boundary_edges() {
        assert_eq!(split_lines(""), Vec::<&str>::new());
        assert_eq!(split_lines("a\n"), ["a"]);
        assert_eq!(split_lines("\n"), [""]);
        assert_eq!(split_lines("a\r"), ["a"]);
        assert_eq!(split_lines("a\r\rb"), ["a", "", "b"]);
        // The unit separator is whitespace to the patterns and
        // no line boundary, so it never splits a line.
        assert_eq!(split_lines("a\u{001F}b"), ["a\u{001F}b"]);
    }

    #[test]
    fn foreign_boundaries_reveal_a_recognizable_line() {
        assert!(looks_like_strace("meeting notes\r99 getpid() = 99"));
        assert!(looks_like_strace("meeting notes\u{001C}99 getpid() = 99"));
        assert!(looks_like_strace("meeting notes\u{000B}99 getpid() = 99"));
        assert!(!looks_like_strace("meeting notes\u{001F}99 getpid() = 99"));
    }

    #[test]
    fn unit_separator_counts_as_pattern_whitespace() {
        assert!(strace_line_shape("foo() = 3\u{001F}"));
        assert!(strace_line_shape("foo() =\u{001F}3"));
    }
}
