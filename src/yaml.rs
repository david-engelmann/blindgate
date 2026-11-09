//! The YAML subset reader PORT4-2 states.
//!
//! The subset carries block mappings and sequences at any
//! nesting the bound allows, flow mappings and sequences,
//! plain scalars, single quoted scalars, and double quoted
//! scalars with the YAML escape set, with comments, an
//! optional document start marker, and a document end
//! marker. Plain scalar resolution follows PyYAML's YAML
//! 1.1 table as the round's probes recorded it. The refused
//! family is the contract's stated port limit. Anchors and
//! aliases, merge keys, tags, block scalars, explicit keys,
//! directives, multiple documents, and tabs in indentation
//! all refuse. Nesting is bounded at four thousand and the
//! reading runs on a spawned thread with a 256 MiB stack.
//! PyYAML's error texts are not reproduced, so a refusal
//! here names the failure class in the port's own words.

use crate::value::{map_insert, run_on_parse_stack, IntVal, Value};

/// The deepest nesting the subset follows, per PORT4-2.
const YAML_DEPTH_BOUND: usize = 4_000;

/// Read a YAML text in the subset into the value model.
///
/// A text that parses to no document, empty or comments
/// only, reads as a null.
pub fn parse_yaml(text: &str) -> Result<Value, String> {
    // The stream reading strips one leading byte order
    // mark, as PyYAML's does. A second mark is data. The
    // JSON reading does not strip one, and refuses a
    // marked text on its own (pinned in the value tests).
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let owned = text.to_string();
    run_on_parse_stack(move || {
        let mut parser = Parser::new(&owned)?;
        parser.parse_document()
    })
}

#[derive(Debug, Clone)]
struct RawLine {
    indent: usize,
    text: String,
    /// The wide break that opened this line, when a line
    /// separator or paragraph separator split it from the
    /// previous line. Inside a quoted scalar the character
    /// is data, and inside a plain scalar's fold it is
    /// kept, so the split remembers which character it was.
    wide_before: Option<char>,
}

#[derive(Debug, Clone)]
struct Pend {
    indent: usize,
    text: String,
    raw: usize,
}

fn is_ignorable(line: &RawLine) -> bool {
    let trimmed = line.text.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// Cut a trailing comment from a line's text.
///
/// A hash preceded by a blank starts the comment, and a
/// hash at the start of the text is the whole comment.
fn strip_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] == b'#'
            && (index == 0 || bytes[index - 1] == b' ' || bytes[index - 1] == b'\t')
        {
            return text[..index].trim_end();
        }
    }
    text.trim_end()
}

/// The text that stands before a trailing comment starts.
///
/// A hash preceded by a blank starts the comment, as in
/// strip_comment, and the text before it is the scalar's
/// own, untrimmed, so a tab the trimming would hide still
/// stands in it.
fn before_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] == b'#'
            && (index == 0 || bytes[index - 1] == b' ' || bytes[index - 1] == b'\t')
        {
            return &text[..index];
        }
    }
    text
}

struct Parser {
    lines: Vec<RawLine>,
    pos: usize,
    pending: Option<Pend>,
}

impl Parser {
    fn new(text: &str) -> Result<Parser, String> {
        // Split the text on the break set PyYAML's scanner
        // reads (newline, carriage return, NEL, line
        // separator, paragraph separator), a CRLF pair
        // counting as one break. A line a wide break opened
        // remembers the character, because inside a quoted
        // scalar that character is data and inside a plain
        // scalar's fold it is kept, while the narrow breaks
        // fold. The break handling is this scanner rule,
        // never a text substitution.
        let mut pieces: Vec<(String, Option<char>)> = Vec::new();
        let mut current = String::new();
        let mut wide_before: Option<char> = None;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\n' | '\u{0085}' => {
                    pieces.push((std::mem::take(&mut current), wide_before.take()));
                }
                '\r' => {
                    pieces.push((std::mem::take(&mut current), wide_before.take()));
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                '\u{2028}' | '\u{2029}' => {
                    pieces.push((std::mem::take(&mut current), wide_before.take()));
                    wide_before = Some(ch);
                }
                _ => current.push(ch),
            }
        }
        pieces.push((current, wide_before));
        let mut lines = Vec::new();
        for (line, wide) in &pieces {
            let mut indent = 0usize;
            let mut tabbed = false;
            for ch in line.chars() {
                match ch {
                    ' ' => indent += 1,
                    '\t' => tabbed = true,
                    _ => break,
                }
            }
            let content = &line[indent..];
            let content = content.strip_prefix('\t').unwrap_or(content);
            if tabbed && !content.trim().is_empty() && !content.trim_start().starts_with('#') {
                return Err("the document is indented with a tab".to_string());
            }
            lines.push(RawLine {
                indent,
                text: line[indent..].to_string(),
                wide_before: *wide,
            });
        }
        Ok(Parser {
            lines,
            pos: 0,
            pending: None,
        })
    }

    fn peek(&self) -> Option<Pend> {
        if let Some(pend) = &self.pending {
            return Some(pend.clone());
        }
        let mut index = self.pos;
        while index < self.lines.len() {
            let line = &self.lines[index];
            if is_ignorable(line) {
                index += 1;
                continue;
            }
            return Some(Pend {
                indent: line.indent,
                text: line.text.clone(),
                raw: index,
            });
        }
        None
    }

    /// Consume the line peek last returned.
    fn consume(&mut self, seen: &Pend) {
        if self.pending.is_some() {
            self.pending = None;
        } else {
            self.pos = seen.raw + 1;
        }
    }

    fn parse_document(&mut self) -> Result<Value, String> {
        // The leading region. A directive refuses, a start
        // marker opens the document, and anything else is
        // the document itself.
        let mut marker_rest: Option<Pend> = None;
        if let Some(cur) = self.peek() {
            let text = cur.text.trim_end();
            if cur.indent == 0 && text.starts_with('%') {
                return Err(
                    "a directive stands in the document, which the subset does not carry"
                        .to_string(),
                );
            }
            if text == "..." {
                self.consume(&cur);
                return self.after_end_marker(Value::Null);
            }
            if is_doc_start(text) {
                self.consume(&cur);
                let rest = text[3..].trim_start().to_string();
                if !rest.is_empty() && !rest.starts_with('#') {
                    marker_rest = Some(Pend {
                        indent: cur.indent,
                        text: rest,
                        raw: cur.raw,
                    });
                }
            }
        }
        let value = match marker_rest {
            Some(pend) => {
                if starts_mapping(pend.text.trim_start()) {
                    return Err(
                        "a block mapping stands on the document start line, which the subset does not carry"
                            .to_string(),
                    );
                }
                self.pending = Some(pend);
                self.parse_block(0)?
            }
            None => {
                if self.peek().is_none() {
                    Value::Null
                } else {
                    self.parse_block(0)?
                }
            }
        };
        // The region after the document. An end marker may
        // close it, and nothing may follow either form.
        match self.peek() {
            None => Ok(value),
            Some(cur) => {
                let text = cur.text.trim_end();
                if text == "..." {
                    self.consume(&cur);
                    self.after_end_marker(value)
                } else if is_doc_start(text) {
                    Err("the text holds more than one document".to_string())
                } else {
                    Err("content stands after the document ends".to_string())
                }
            }
        }
    }

    fn after_end_marker(&mut self, value: Value) -> Result<Value, String> {
        match self.peek() {
            None => Ok(value),
            Some(_) => Err("the text holds more than one document".to_string()),
        }
    }

    fn parse_block(&mut self, depth: usize) -> Result<Value, String> {
        if depth > YAML_DEPTH_BOUND {
            return Err("the document nests deeper than the subset carries".to_string());
        }
        let cur = match self.peek() {
            Some(cur) => cur,
            None => return Ok(Value::Null),
        };
        let text = cur.text.trim_end().to_string();
        let indent = cur.indent;
        if is_seq_start(&text) {
            return self.parse_sequence(cur.indent, depth);
        }
        if starts_mapping(&text) {
            return self.parse_mapping(cur.indent, depth);
        }
        self.parse_value_from(cur, indent, depth)
    }

    /// The value standing after a key with nothing on its line.
    ///
    /// A deeper block is the value, a sequence at the key's
    /// own indentation is the value, and anything else
    /// leaves the value null.
    fn nested_value(&mut self, indent: usize, depth: usize) -> Result<Value, String> {
        let next = match self.peek() {
            Some(next) => next,
            None => return Ok(Value::Null),
        };
        if next.indent > indent {
            return self.parse_block(depth + 1);
        }
        if next.indent == indent && is_seq_start(next.text.trim_end()) {
            return self.parse_sequence(indent, depth + 1);
        }
        Ok(Value::Null)
    }

    fn parse_mapping(&mut self, indent: usize, depth: usize) -> Result<Value, String> {
        let mut entries: Vec<(Value, Value)> = Vec::new();
        while let Some(cur) = self.peek() {
            if cur.indent < indent {
                break;
            }
            if cur.indent > indent {
                return Err("a line stands deeper than the structure around it".to_string());
            }
            let text = cur.text.trim_end();
            if indent == 0 && (text == "..." || is_doc_start(text)) {
                break;
            }
            let split = match self.split_key(&cur)? {
                Some(split) => split,
                None => {
                    return Err("a mapping entry carries no colon".to_string());
                }
            };
            if split.merge_key {
                return Err(
                    "a merge key stands in the document, which the subset does not carry"
                        .to_string(),
                );
            }
            let value = if strip_comment(&split.rest).trim().is_empty() {
                self.consume(&cur);
                self.nested_value(indent, depth)?
            } else {
                let rest = Pend {
                    indent: cur.indent,
                    text: split.rest.clone(),
                    raw: cur.raw,
                };
                self.pending = Some(rest.clone());
                self.parse_value_from(rest, indent, depth)?
            };
            yaml_map_insert(&mut entries, split.key, value);
        }
        Ok(Value::Map(entries))
    }

    fn parse_sequence(&mut self, indent: usize, depth: usize) -> Result<Value, String> {
        let mut items = Vec::new();
        while let Some(cur) = self.peek() {
            if cur.indent < indent {
                break;
            }
            if cur.indent > indent {
                return Err("a line stands deeper than the structure around it".to_string());
            }
            // Only trailing spaces trim here, for the
            // reason split_key states. A trailing tab
            // reaches the entry's value reading whole.
            let text = cur.text.trim_end_matches(' ').to_string();
            if indent == 0 && (text == "..." || is_doc_start(&text)) {
                break;
            }
            if !is_seq_start(&text) {
                break;
            }
            let after_dash = &text[1..];
            let rest = after_dash.trim_start().to_string();
            if strip_comment(&rest).trim().is_empty() {
                self.consume(&cur);
                let next = self.peek();
                let item = match next {
                    Some(next) if next.indent > indent => self.parse_block(depth + 1)?,
                    _ => Value::Null,
                };
                items.push(item);
                continue;
            }
            let item_column = cur.indent + (text.len() - rest.len());
            let rest_pend = Pend {
                indent: item_column,
                text: rest.clone(),
                raw: cur.raw,
            };
            self.consume(&cur);
            self.pending = Some(rest_pend.clone());
            let item = if is_seq_start(&rest) {
                self.parse_sequence(item_column, depth + 1)?
            } else if starts_mapping(&rest) {
                self.parse_mapping(item_column, depth + 1)?
            } else {
                self.parse_value_from(rest_pend, indent, depth)?
            };
            items.push(item);
        }
        Ok(Value::List(items))
    }
}

fn is_doc_start(text: &str) -> bool {
    text == "---"
        || text.starts_with("--- ")
        || text.starts_with("---\t")
        || text.starts_with("---#")
}

fn is_seq_start(text: &str) -> bool {
    text == "-" || text.starts_with("- ") || text.starts_with("-\t")
}

/// Whether a line opens a block mapping.
///
/// A plain key ends at a colon followed by a blank or the
/// line's end, and a quoted key is judged by its own scan.
fn starts_mapping(text: &str) -> bool {
    let trimmed = text.trim_start();
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        // A flow collection opens here, and a colon inside
        // its braces is a flow entry's, not a block key's.
        return false;
    }
    if trimmed.starts_with('\'') || trimmed.starts_with('"') {
        return quoted_key_split(trimmed).is_some();
    }
    if trimmed.starts_with('?') {
        return false;
    }
    plain_key_split(trimmed).is_some()
}

/// Split a plain key from a line's text.
///
/// Returns the key text and the text after the colon. A
/// colon closes the key only when a blank or the line's
/// end follows it, and a comment closes the line without
/// a key at all.
/// The byte index of the colon that closes a plain key,
/// under the plain key rule plain_key_split states.
fn key_colon_index(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'#' && index > 0 && (bytes[index - 1] == b' ' || bytes[index - 1] == b'\t') {
            return None;
        }
        if byte == b':' {
            let follows = bytes.get(index + 1);
            if follows.is_none() || follows == Some(&b' ') || follows == Some(&b'\t') {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn plain_key_split(text: &str) -> Option<(String, String)> {
    let index = key_colon_index(text)?;
    let key = text[..index].trim_end().to_string();
    let rest = text[index + 1..].trim_start().to_string();
    Some((key, rest))
}

/// Split a quoted key from a line's text, on one line.
///
/// Returns the key text and the text after the colon when
/// the quoted scalar closes on this line and a colon
/// follows it.
fn quoted_key_split(text: &str) -> Option<(String, String)> {
    let quote = text.as_bytes()[0];
    let (key, close) = scan_quoted_line(text, 1, quote).ok()?;
    let end = close?;
    after_quoted_key(text, end, key)
}

fn after_quoted_key(text: &str, from: usize, key: String) -> Option<(String, String)> {
    let rest = text[from..].trim_start();
    if !rest.starts_with(':') {
        return None;
    }
    let after = &rest[1..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    Some((key, after.trim_start().to_string()))
}

struct SplitKey {
    key: Value,
    rest: String,
    merge_key: bool,
}

/// The way a folded scalar's segments join.
///
/// One line break folds to a space, and each further
/// break stands as one newline. A break count of zero is
/// the double quoted escaped break, which joins directly.
/// A segment a wide break opened joins with that very
/// character when it stands alone, because the wide
/// break inside a plain scalar is kept as PyYAML keeps
/// it, and the blank lines' newline rule governs when
/// further breaks stand with it.
fn fold_segments(segments: &[(usize, Option<char>, String)]) -> String {
    let mut out = String::new();
    for (index, (breaks, wide, segment)) in segments.iter().enumerate() {
        if index == 0 {
            out.push_str(segment);
            continue;
        }
        match (breaks, wide) {
            (1, Some(ch)) => out.push(*ch),
            (0, _) => {}
            (1, None) => out.push(' '),
            (more, _) => {
                for _ in 0..(more - 1) {
                    out.push('\n');
                }
            }
        }
        out.push_str(segment);
    }
    out
}

/// The text a run of line breaks folds to inside a
/// quoted scalar, under the flow scalar rule PyYAML's
/// scanner applies.
///
/// A lone narrow break folds to a space. A wide break
/// standing first in the run is kept as data, as the
/// scanner keeps it. Every further break in the run
/// stands as its transformed self, a newline for a
/// narrow break and the character itself for a wide
/// one, so a wide break at a quoted line's end, just
/// before a narrow break, leaves the wide character
/// standing in the value with one newline after it.
fn fold_break_run(run: &[char]) -> String {
    let mut out = String::new();
    if run.len() == 1 {
        if run[0] == '\n' {
            out.push(' ');
        } else {
            out.push(run[0]);
        }
        return out;
    }
    if let Some(first) = run.first() {
        if *first != '\n' {
            out.push(*first);
        }
        for ch in &run[1..] {
            out.push(*ch);
        }
    }
    out
}

/// Assemble a quoted scalar from its segments and the
/// joins the break runs before them folded to.
///
/// A one segment scalar keeps its text whole. Otherwise
/// each segment is stripped of its surrounding blanks,
/// blanks here being spaces and tabs only, because a
/// wide character standing at a segment's edge is data
/// the scanner kept, not a blank.
fn fold_quoted_segments(segments: &[(String, String)]) -> String {
    if segments.len() == 1 {
        return segments[0].1.clone();
    }
    let mut out = String::new();
    for (index, (join, segment)) in segments.iter().enumerate() {
        if index > 0 {
            out.push_str(join);
        }
        let trimmed = if index == 0 {
            segment.trim_end_matches([' ', '\t'])
        } else {
            segment.trim_matches([' ', '\t'])
        };
        out.push_str(trimmed);
    }
    out
}

/// The refusal a plain key led by a refused family marker
/// earns.
///
/// PORT4-2 states the refused family, and the family
/// stands in key position as it stands in value position,
/// so a plain key whose first character is an anchor,
/// alias, or tag marker refuses in the same words the
/// value reading uses. A reserved character or a percent
/// sign at the lead refuses with them, as PyYAML's
/// scanner refuses those token leads. A marker character
/// away from the lead is plain text, and a quoted key
/// never reaches here.
/// Insert into a map the YAML reader is building.
///
/// The reader hands back one shared NaN object for every
/// NaN spelling in a document, as PyYAML's float
/// constructor does, and a dictionary keeps the last
/// value for a key that is the identical object, so two
/// NaN keys in one document collide here. The value
/// model's own rule stands untouched, because distinct
/// NaN objects are still distinct keys everywhere else.
fn yaml_map_insert(map: &mut Vec<(Value, Value)>, key: Value, value: Value) {
    if matches!(key, Value::Float(number) if number.is_nan()) {
        for (held, existing) in map.iter_mut() {
            if matches!(held, Value::Float(number) if number.is_nan()) {
                *existing = value;
                return;
            }
        }
    }
    map_insert(map, key, value);
}

fn key_marker_refusal(text: &str) -> Option<&'static str> {
    match text.trim_start().as_bytes().first().copied() {
        Some(b'&') => Some("an anchor stands in the document, which the subset does not carry"),
        Some(b'*') => Some("an alias stands in the document, which the subset does not carry"),
        Some(b'!') => Some("a tag stands in the document, which the subset does not carry"),
        Some(b'@') | Some(b'`') => {
            Some("a scalar starts with a reserved character, which the subset does not carry")
        }
        Some(b'%') => Some("a directive stands in the document, which the subset does not carry"),
        _ => None,
    }
}

impl Parser {
    fn split_key(&self, cur: &Pend) -> Result<Option<SplitKey>, String> {
        // Only trailing spaces trim here. A trailing tab
        // stands inside the value's own text, and trimming
        // it away would hide it from the plain scalar
        // reading that refuses it.
        let text = cur.text.trim_end_matches(' ');
        let first = text.as_bytes().first().copied();
        match first {
            Some(b'?') => {
                let second = text.as_bytes().get(1).copied();
                if text.len() == 1 || second == Some(b' ') || second == Some(b'\t') {
                    return Err(
                        "an explicit key stands in the document, which the subset does not carry"
                            .to_string(),
                    );
                }
            }
            Some(b'\'') | Some(b'"') => {
                // A tab standing in the separation around
                // the colon refuses, as it does around a
                // plain key's colon.
                if let Ok((_, Some(close))) =
                    scan_quoted_line(text, 1, first.expect("a quote stands here"))
                {
                    let bytes = text.as_bytes();
                    let mut index = close;
                    let mut tabbed = false;
                    while index < bytes.len() && (bytes[index] == b' ' || bytes[index] == b'\t') {
                        tabbed |= bytes[index] == b'\t';
                        index += 1;
                    }
                    if bytes.get(index) == Some(&b':') {
                        let mut after = index + 1;
                        let mut tabbed_after = false;
                        while after < bytes.len() && (bytes[after] == b' ' || bytes[after] == b'\t')
                        {
                            tabbed_after |= bytes[after] == b'\t';
                            after += 1;
                        }
                        if tabbed || tabbed_after {
                            return Err(
                                "a tab stands in separation position, which the subset does not carry"
                                    .to_string(),
                            );
                        }
                    }
                }
                return match quoted_key_split(text) {
                    Some((key, rest)) => Ok(Some(SplitKey {
                        key: Value::Text(key),
                        rest,
                        merge_key: false,
                    })),
                    None => Ok(None),
                };
            }
            _ => {}
        }
        if let Some(words) = key_marker_refusal(text) {
            return Err(words.to_string());
        }
        match plain_key_split(text) {
            Some((key_text, rest)) => {
                if key_text.is_empty() {
                    return Err(
                        "an empty key stands in the document, which the subset does not carry"
                            .to_string(),
                    );
                }
                if let Some(colon) = key_colon_index(text) {
                    let bytes = text.as_bytes();
                    let mut after = colon + 1;
                    let mut tabbed_after = false;
                    while after < bytes.len() && (bytes[after] == b' ' || bytes[after] == b'\t') {
                        tabbed_after |= bytes[after] == b'\t';
                        after += 1;
                    }
                    if text[..colon].contains('\t') || tabbed_after {
                        return Err(
                            "a tab stands in separation position, which the subset does not carry"
                                .to_string(),
                        );
                    }
                }
                let key = resolve_plain(&key_text)?;
                Ok(Some(SplitKey {
                    key,
                    rest,
                    merge_key: key_text == "<<",
                }))
            }
            None => Ok(None),
        }
    }

    /// Parse the value whose first line stands as `first`.
    ///
    /// The block indent is the indentation of the construct
    /// that owns the value, and a plain scalar folds the
    /// lines that stand deeper than it.
    fn parse_value_from(
        &mut self,
        first: Pend,
        block_indent: usize,
        depth: usize,
    ) -> Result<Value, String> {
        if self.pending.is_some() {
            self.pending = None;
        }
        if self.pos <= first.raw {
            self.pos = first.raw + 1;
        }
        let lead = first.text.trim_start().to_string();
        let first_char = lead.as_bytes().first().copied();
        match first_char {
            Some(b'[') | Some(b'{') => self.parse_flow(&first, &lead, depth),
            Some(b'\'') | Some(b'"') => {
                let (value, remainder) = self.parse_quoted_block(&first)?;
                if !strip_comment(&remainder).trim().is_empty() {
                    return Err("content stands after a value on its line".to_string());
                }
                Ok(Value::Text(value))
            }
            Some(b'&') => {
                Err("an anchor stands in the document, which the subset does not carry".to_string())
            }
            Some(b'*') => {
                Err("an alias stands in the document, which the subset does not carry".to_string())
            }
            Some(b'!') => {
                Err("a tag stands in the document, which the subset does not carry".to_string())
            }
            Some(b'|') | Some(b'>') => Err(
                "a block scalar stands in the document, which the subset does not carry"
                    .to_string(),
            ),
            Some(b'@') | Some(b'`') => Err(
                "a scalar starts with a reserved character, which the subset does not carry"
                    .to_string(),
            ),
            Some(b'%') => Err(
                "a directive stands in the document, which the subset does not carry".to_string(),
            ),
            _ => {
                let head = strip_comment(&lead);
                if head == "-" || head.starts_with("- ") {
                    return Err(
                        "a block sequence entry stands in place of a value, which the subset does not carry"
                            .to_string(),
                    );
                }
                if head == "?" || head.starts_with("? ") {
                    return Err(
                        "an explicit key stands in the document, which the subset does not carry"
                            .to_string(),
                    );
                }
                if head == ":" || head.starts_with(": ") {
                    return Err(
                        "a mapping value stands without a key, which the subset does not carry"
                            .to_string(),
                    );
                }
                let folded = self.fold_plain(&first, &lead, block_indent)?;
                resolve_plain(&folded)
            }
        }
    }

    /// Fold a plain scalar across its continuation lines.
    fn fold_plain(
        &mut self,
        first: &Pend,
        lead: &str,
        block_indent: usize,
    ) -> Result<String, String> {
        // A tab standing inside the scalar's own text
        // refuses, as PyYAML's scanner refuses it. The
        // check reads the text before the comment starts
        // and before any trimming, because a trailing tab
        // is inside the token to the scanner even where
        // the trimming would hide it. A tab inside the
        // comment itself is comment text and stands.
        if before_comment(lead).contains('\t') {
            return Err(
                "a tab stands inside a plain scalar, which the subset does not carry".to_string(),
            );
        }
        let head = strip_comment(lead).to_string();
        if head.contains(": ") || head.ends_with(": ") {
            return Err("a mapping value stands inside a plain scalar".to_string());
        }
        let mut segments: Vec<(usize, Option<char>, String)> = vec![(0, None, head)];
        let mut last_consumed = first.raw;
        let mut index = first.raw + 1;
        loop {
            let mut breaks = 1usize;
            let mut probe = index;
            while probe < self.lines.len() && self.lines[probe].text.trim().is_empty() {
                breaks += 1;
                probe += 1;
            }
            if probe >= self.lines.len() {
                break;
            }
            let line = &self.lines[probe];
            if line.indent <= block_indent {
                break;
            }
            if line.text.trim_start().starts_with('#') {
                break;
            }
            if before_comment(&line.text).contains('\t') {
                return Err(
                    "a tab stands inside a plain scalar, which the subset does not carry"
                        .to_string(),
                );
            }
            let segment = strip_comment(&line.text).to_string();
            if segment.contains(": ") || segment.ends_with(':') {
                return Err("a mapping value stands inside a plain scalar".to_string());
            }
            let wide = if breaks == 1 { line.wide_before } else { None };
            segments.push((breaks, wide, segment));
            last_consumed = probe;
            index = probe + 1;
        }
        self.pos = last_consumed + 1;
        Ok(fold_segments(&segments))
    }

    /// Parse a quoted scalar that may span lines.
    ///
    /// Returns the scalar's text and the remainder of the
    /// line the closing quote stands on. Folding follows
    /// the flow scalar rule, each segment stripped of its
    /// surrounding blanks, and a double quoted break
    /// escaped by a trailing backslash joins directly.
    fn parse_quoted_block(&mut self, first: &Pend) -> Result<(String, String), String> {
        let quote = first.text.trim_start().as_bytes()[0];
        let mut raw_segments: Vec<(String, String)> = Vec::new();
        let mut current = first.text.trim_start().to_string();
        let mut current_raw = first.raw;
        let mut line_start = first.raw;
        let mut start = 1usize;
        let mut join_before = String::new();
        loop {
            // A wide break inside the scalar is data, so
            // the pieces a wide break split from this
            // physical line rejoin it with the very
            // character that split them.
            while current_raw + 1 < self.lines.len() {
                let Some(ch) = self.lines[current_raw + 1].wide_before else {
                    break;
                };
                let next = &self.lines[current_raw + 1];
                current.push(ch);
                for _ in 0..next.indent {
                    current.push(' ');
                }
                current.push_str(&next.text);
                current_raw += 1;
            }
            let (segment, close) = scan_quoted_line(&current, start, quote)?;
            match close {
                Some(end) => {
                    let remainder = current[end..].to_string();
                    raw_segments.push((join_before, segment));
                    self.pos = current_raw + 1;
                    return Ok((fold_quoted_segments(&raw_segments), remainder));
                }
                None => {
                    let (segment, escaped_break) = match segment.strip_suffix('\u{0001}') {
                        Some(stripped) => (stripped.to_string(), true),
                        None => (segment, false),
                    };
                    // Find the next content line.
                    let mut probe = current_raw + 1;
                    while probe < self.lines.len() && self.lines[probe].text.trim().is_empty() {
                        probe += 1;
                    }
                    if probe >= self.lines.len() {
                        return Err("a quoted scalar is not closed".to_string());
                    }
                    // The break run between this segment and
                    // the next stands from this logical
                    // line's last content piece to the next
                    // content piece, one transformed break
                    // per piece boundary, a wide break kept
                    // as itself and every narrow break a
                    // newline. Pieces of this line past its
                    // last content piece are blank wide
                    // pieces the rejoin above merged, and
                    // the wide breaks that opened them stand
                    // at the segment's end, so those breaks
                    // open the run and come off the segment.
                    let mut content_last = current_raw;
                    while content_last > line_start
                        && self.lines[content_last].text.trim().is_empty()
                    {
                        content_last -= 1;
                    }
                    let mut run: Vec<char> = Vec::new();
                    for boundary in content_last..probe {
                        run.push(self.lines[boundary + 1].wide_before.unwrap_or('\n'));
                    }
                    let mut segment = segment;
                    for _ in content_last..current_raw {
                        segment = segment
                            .trim_end_matches([' ', '\t'])
                            .trim_end_matches(['\u{2028}', '\u{2029}'])
                            .to_string();
                    }
                    raw_segments.push((join_before, segment));
                    join_before = if escaped_break {
                        // The escaped break joins directly,
                        // and any further breaks in the run
                        // stand as their transformed selves.
                        run.iter().skip(1).collect()
                    } else {
                        fold_break_run(&run)
                    };
                    current_raw = probe;
                    line_start = probe;
                    current = self.lines[probe].text.clone();
                    start = 0;
                }
            }
        }
    }
}

/// Scan one line of a quoted scalar from `start`.
///
/// Returns the segment the line contributes and the byte
/// index just past the closing quote when the scalar
/// closes on this line. An escaped break in a double
/// quoted scalar marks the segment with a sentinel the
/// caller reads, because the break joins directly.
fn scan_quoted_line(
    text: &str,
    start: usize,
    quote: u8,
) -> Result<(String, Option<usize>), String> {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut index = start;
    while index < bytes.len() {
        let byte = bytes[index];
        if quote == b'\'' {
            if byte == b'\'' {
                if bytes.get(index + 1) == Some(&b'\'') {
                    out.push('\'');
                    index += 2;
                    continue;
                }
                return Ok((out, Some(index + 1)));
            }
            let ch = text[index..].chars().next().expect("a char stands here");
            out.push(ch);
            index += ch.len_utf8();
            continue;
        }
        // Double quoted.
        if byte == b'"' {
            return Ok((out, Some(index + 1)));
        }
        if byte == b'\\' {
            if index + 1 >= bytes.len() {
                out.push('\u{0001}');
                return Ok((out, None));
            }
            let (ch, width) = decode_escape(text, index)?;
            out.push(ch);
            index += width;
            continue;
        }
        let ch = text[index..].chars().next().expect("a char stands here");
        out.push(ch);
        index += ch.len_utf8();
    }
    Ok((out, None))
}

/// Decode one double quoted escape at a backslash.
///
/// Returns the character and the width in bytes the
/// escape spans. The escape set is YAML's, and an escape
/// the set does not hold refuses, as does a unicode
/// escape naming half of a surrogate pair, which the
/// model cannot carry.
fn decode_escape(text: &str, at: usize) -> Result<(char, usize), String> {
    let bytes = text.as_bytes();
    let code = bytes[at + 1];
    let simple = match code {
        b'0' => Some('\u{0000}'),
        b'a' => Some('\u{0007}'),
        b'b' => Some('\u{0008}'),
        b't' | b'\t' => Some('\t'),
        b'n' => Some('\n'),
        b'v' => Some('\u{000B}'),
        b'f' => Some('\u{000C}'),
        b'r' => Some('\r'),
        b'e' => Some('\u{001B}'),
        b' ' => Some(' '),
        b'"' => Some('"'),
        b'/' => Some('/'),
        b'\\' => Some('\\'),
        b'N' => Some('\u{0085}'),
        b'_' => Some('\u{00A0}'),
        b'L' => Some('\u{2028}'),
        b'P' => Some('\u{2029}'),
        _ => None,
    };
    if let Some(ch) = simple {
        return Ok((ch, 2));
    }
    let digits = match code {
        b'x' => 2,
        b'u' => 4,
        b'U' => 8,
        _ => {
            return Err(
                "a double quoted scalar carries an escape the subset does not know".to_string(),
            )
        }
    };
    let span = &text[at + 2..];
    if span.len() < digits {
        return Err("a double quoted escape is not hexadecimal".to_string());
    }
    let hex = &span[..digits];
    let value = match u32::from_str_radix(hex, 16) {
        Ok(value) => value,
        Err(_) => return Err("a double quoted escape is not hexadecimal".to_string()),
    };
    if (0xD800..0xE000).contains(&value) {
        return Err("a double quoted escape names half of a surrogate pair".to_string());
    }
    match char::from_u32(value) {
        Some(ch) => Ok((ch, 2 + digits)),
        None => Err("a double quoted escape names no character".to_string()),
    }
}

/// The character stream a flow collection reads from.
///
/// The stream is built from the line the collection
/// opens on and every line after it, because a flow
/// collection may span lines. Each character knows the
/// line it stands on so the reader can hand the parser
/// back the line the collection closed on and the text
/// that follows the closing bracket on that line.
struct FlowSource {
    chars: Vec<char>,
    line_of: Vec<usize>,
    col_of: Vec<usize>,
    segs: Vec<(usize, String)>,
    pos: usize,
    prev: Option<char>,
}

impl FlowSource {
    fn build(parser: &Parser, first: &Pend, lead: &str) -> FlowSource {
        let mut segs: Vec<(usize, String)> = vec![(first.raw, lead.to_string())];
        for raw in (first.raw + 1)..parser.lines.len() {
            let line = &parser.lines[raw];
            let mut text = " ".repeat(line.indent);
            text.push_str(&line.text);
            segs.push((raw, text));
        }
        let mut chars = Vec::new();
        let mut line_of = Vec::new();
        let mut col_of = Vec::new();
        for (line_index, (raw, text)) in segs.iter().enumerate() {
            if line_index > 0 {
                // A line a wide break opened joins with that
                // character, which is data inside a flow
                // scalar, and with a newline otherwise.
                chars.push(parser.lines[*raw].wide_before.unwrap_or('\n'));
                line_of.push(line_index - 1);
                col_of.push(segs[line_index - 1].1.chars().count());
            }
            for (col, ch) in text.chars().enumerate() {
                chars.push(ch);
                line_of.push(line_index);
                col_of.push(col);
            }
        }
        FlowSource {
            chars,
            line_of,
            col_of,
            segs,
            pos: 0,
            prev: None,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.prev = Some(ch);
        self.pos += 1;
        Some(ch)
    }

    /// The line the collection closed on and its remainder.
    fn remainder(&self) -> (usize, String) {
        if self.pos == 0 {
            return (self.segs[0].0, String::new());
        }
        let closed_line = self.line_of[self.pos - 1];
        let raw = self.segs[closed_line].0;
        if self.pos < self.chars.len() && self.line_of[self.pos] == closed_line {
            let from = self.col_of[self.pos];
            let text: String = self.segs[closed_line].1.chars().skip(from).collect();
            (raw, text)
        } else {
            (raw, String::new())
        }
    }

    fn skip_space(&mut self) {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\n') | Some('\u{2028}') | Some('\u{2029}') => {
                    self.bump();
                }
                Some('#') => {
                    let comment_follows = matches!(
                        self.prev,
                        None | Some(' ')
                            | Some('\t')
                            | Some('\n')
                            | Some(',')
                            | Some('[')
                            | Some('{')
                    );
                    if !comment_follows {
                        return;
                    }
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    fn node(&mut self, depth: usize) -> Result<Value, String> {
        if depth > YAML_DEPTH_BOUND {
            return Err("the document nests deeper than the subset carries".to_string());
        }
        self.skip_space();
        match self.peek() {
            None => Err("a flow collection is not closed".to_string()),
            Some('[') => self.flow_sequence(depth),
            Some('{') => self.flow_mapping(depth),
            Some('\'') | Some('"') => Ok(Value::Text(self.flow_quoted()?)),
            Some('&') => {
                Err("an anchor stands in the document, which the subset does not carry".to_string())
            }
            Some('*') => {
                Err("an alias stands in the document, which the subset does not carry".to_string())
            }
            Some('!') => {
                Err("a tag stands in the document, which the subset does not carry".to_string())
            }
            Some('|') | Some('>') => Err(
                "a block scalar stands in the document, which the subset does not carry"
                    .to_string(),
            ),
            Some('@') | Some('`') => Err(
                "a scalar starts with a reserved character, which the subset does not carry"
                    .to_string(),
            ),
            Some('%') => Err(
                "a directive stands in the document, which the subset does not carry".to_string(),
            ),
            Some(_) => {
                let text = self.flow_plain()?;
                if text == "?" {
                    return Err(
                        "an explicit key stands in the document, which the subset does not carry"
                            .to_string(),
                    );
                }
                if text == ":" {
                    return Err(
                        "a mapping value stands without a key, which the subset does not carry"
                            .to_string(),
                    );
                }
                resolve_plain(&text)
            }
        }
    }

    fn flow_sequence(&mut self, depth: usize) -> Result<Value, String> {
        self.bump();
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(Value::List(items));
        }
        loop {
            let item_lead = self
                .chars
                .iter()
                .skip(self.pos)
                .find(|ch| !matches!(ch, ' ' | '\t' | '\n'))
                .copied();
            let quote_at = if matches!(item_lead, Some('\'') | Some('"')) {
                self.chars
                    .iter()
                    .skip(self.pos)
                    .position(|ch| !matches!(ch, ' ' | '\t' | '\n'))
                    .map(|offset| self.pos + offset)
            } else {
                None
            };
            let mut item = self.node(depth + 1)?;
            let item_end = self.pos;
            self.skip_space();
            if self.peek() == Some(':') && !matches!(item, Value::List(_) | Value::Map(_)) {
                // A single pair mapping standing in a sequence.
                self.bump();
                self.skip_space();
                let value = match self.peek() {
                    Some(',') | Some(']') => Value::Null,
                    _ => self.node(depth + 1)?,
                };
                if item == Value::Text("<<".to_string())
                    && !matches!(item_lead, Some('\'') | Some('"'))
                {
                    return Err(
                        "a merge key stands in the document, which the subset does not carry"
                            .to_string(),
                    );
                }
                if let Some(start) = quote_at {
                    if self.chars[start..item_end].contains(&'\n') {
                        return Err(
                            "a quoted key runs over more than one line, which the subset does not carry"
                                .to_string(),
                        );
                    }
                }
                let mut pair = Vec::new();
                yaml_map_insert(&mut pair, item, value);
                item = Value::Map(pair);
            }
            items.push(item);
            self.skip_space();
            match self.peek() {
                Some(',') => {
                    self.bump();
                    self.skip_space();
                    if self.peek() == Some(']') {
                        self.bump();
                        return Ok(Value::List(items));
                    }
                }
                Some(']') => {
                    self.bump();
                    return Ok(Value::List(items));
                }
                _ => return Err("a flow entry is not followed by a comma".to_string()),
            }
        }
    }

    fn flow_mapping(&mut self, depth: usize) -> Result<Value, String> {
        self.bump();
        let mut entries: Vec<(Value, Value)> = Vec::new();
        self.skip_space();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(Value::Map(entries));
        }
        loop {
            self.skip_space();
            let key = match self.peek() {
                Some('\'') | Some('"') => {
                    let start = self.pos;
                    let key = self.flow_quoted()?;
                    if self.chars[start..self.pos].contains(&'\n') {
                        return Err(
                            "a quoted key runs over more than one line, which the subset does not carry"
                                .to_string(),
                        );
                    }
                    Value::Text(key)
                }
                Some('[') | Some('{') => {
                    return Err(
                        "a flow collection stands as a key, which the subset does not carry"
                            .to_string(),
                    )
                }
                Some('&') => {
                    return Err(
                        "an anchor stands in the document, which the subset does not carry"
                            .to_string(),
                    )
                }
                Some('*') => {
                    return Err(
                        "an alias stands in the document, which the subset does not carry"
                            .to_string(),
                    )
                }
                Some('!') => {
                    return Err(
                        "a tag stands in the document, which the subset does not carry".to_string(),
                    )
                }
                Some('@') | Some('`') => return Err(
                    "a scalar starts with a reserved character, which the subset does not carry"
                        .to_string(),
                ),
                Some('%') => {
                    return Err(
                        "a directive stands in the document, which the subset does not carry"
                            .to_string(),
                    )
                }
                Some(_) => {
                    let text = self.flow_plain()?;
                    if text == "<<" {
                        return Err(
                            "a merge key stands in the document, which the subset does not carry"
                                .to_string(),
                        );
                    }
                    resolve_plain(&text)?
                }
                None => return Err("a flow collection is not closed".to_string()),
            };
            self.skip_space();
            let value = if self.peek() == Some(':') {
                self.bump();
                // A tab in the separation after the colon
                // refuses, as it does in a block mapping.
                let mut probe = self.pos;
                let mut tabbed = false;
                while let Some(ch) = self.chars.get(probe).copied() {
                    match ch {
                        '\t' => {
                            tabbed = true;
                            probe += 1;
                        }
                        ' ' => probe += 1,
                        _ => break,
                    }
                }
                if tabbed {
                    return Err(
                        "a tab stands in separation position, which the subset does not carry"
                            .to_string(),
                    );
                }
                self.skip_space();
                match self.peek() {
                    Some(',') | Some('}') => Value::Null,
                    _ => self.node(depth + 1)?,
                }
            } else if matches!(self.peek(), Some(',') | Some('}')) {
                Value::Null
            } else {
                return Err("a flow mapping entry carries no colon".to_string());
            };
            yaml_map_insert(&mut entries, key, value);
            self.skip_space();
            match self.peek() {
                Some(',') => {
                    self.bump();
                    self.skip_space();
                    if self.peek() == Some('}') {
                        self.bump();
                        return Ok(Value::Map(entries));
                    }
                }
                Some('}') => {
                    self.bump();
                    return Ok(Value::Map(entries));
                }
                _ => return Err("a flow entry is not followed by a comma".to_string()),
            }
        }
    }

    /// Read a plain scalar in flow context.
    ///
    /// The scalar ends at a flow indicator, at a colon
    /// followed by a blank or a flow indicator, or at a
    /// comment. Blanks inside fold to one space.
    fn flow_plain(&mut self) -> Result<String, String> {
        let mut out = String::new();
        let mut pending_space = false;
        while let Some(ch) = self.peek() {
            match ch {
                ',' | '[' | ']' | '{' | '}' => break,
                ':' => {
                    let next = self.chars.get(self.pos + 1).copied();
                    let ends = matches!(
                        next,
                        None | Some(' ')
                            | Some('\t')
                            | Some('\n')
                            | Some(',')
                            | Some('[')
                            | Some(']')
                            | Some('{')
                            | Some('}')
                    );
                    if ends {
                        break;
                    }
                    if pending_space && !out.is_empty() {
                        out.push(' ');
                    }
                    pending_space = false;
                    out.push(':');
                    self.bump();
                }
                '#' if pending_space || out.is_empty() => break,
                ' ' | '\t' | '\n' => {
                    // A tab that stands after the scalar's
                    // own text has begun stands inside the
                    // token, and refuses as PyYAML's scanner
                    // refuses it. A tab before any text is
                    // separation and folds as a blank does.
                    if ch == '\t' && !out.is_empty() {
                        return Err(
                            "a tab stands inside a plain scalar, which the subset does not carry"
                                .to_string(),
                        );
                    }
                    pending_space = true;
                    self.bump();
                }
                _ => {
                    if pending_space && !out.is_empty() {
                        out.push(' ');
                    }
                    pending_space = false;
                    out.push(ch);
                    self.bump();
                }
            }
        }
        if out.is_empty() {
            return Err("a flow entry is empty".to_string());
        }
        Ok(out)
    }

    /// Read a quoted scalar in flow context.
    fn flow_quoted(&mut self) -> Result<String, String> {
        let quote = self.bump().expect("a quote stands here");
        let mut segments: Vec<(String, String)> = Vec::new();
        let mut join_before = String::new();
        loop {
            let mut segment = String::new();
            let mut escaped_break = false;
            let mut first_break: Option<char> = None;
            let closed = loop {
                let ch = match self.peek() {
                    Some(ch) => ch,
                    None => return Err("a quoted scalar is not closed".to_string()),
                };
                if ch == '\n' || ch == '\u{2028}' || ch == '\u{2029}' {
                    // A break ends the segment. Every wide
                    // character in this stream is a line
                    // boundary, so a wide break ends the
                    // segment as a narrow one does, and the
                    // break run below takes it as the run's
                    // first break.
                    if ch != '\n' {
                        self.bump();
                        first_break = Some(ch);
                    }
                    break false;
                }
                if quote == '\'' && ch == '\'' {
                    self.bump();
                    if self.peek() == Some('\'') {
                        self.bump();
                        segment.push('\'');
                        continue;
                    }
                    break true;
                }
                if quote == '"' && ch == '"' {
                    self.bump();
                    break true;
                }
                if quote == '"' && ch == '\\' {
                    self.bump();
                    match self.peek() {
                        None => return Err("a quoted scalar is not closed".to_string()),
                        Some('\n') => {
                            escaped_break = true;
                            break false;
                        }
                        Some(_) => {
                            let (decoded, width) = decode_escape_chars(&self.chars, self.pos)?;
                            segment.push(decoded);
                            for _ in 0..width {
                                self.bump();
                            }
                            continue;
                        }
                    }
                }
                segment.push(ch);
                self.bump();
            };
            if closed {
                segments.push((join_before, segment));
                return Ok(fold_quoted_segments(&segments));
            }
            segments.push((join_before, segment));
            // Read the break run before the next segment,
            // skipping the blanks that stand between
            // breaks, as the scanner's break reading does.
            // The escaped break stands at the run's head,
            // unbumped, and joins directly.
            let mut run: Vec<char> = Vec::new();
            if let Some(ch) = first_break {
                run.push(ch);
            }
            loop {
                match self.peek() {
                    Some(' ') | Some('\t') => {
                        self.bump();
                    }
                    Some('\n') => {
                        run.push('\n');
                        self.bump();
                    }
                    Some(ch @ ('\u{2028}' | '\u{2029}')) => {
                        run.push(ch);
                        self.bump();
                    }
                    _ => break,
                }
            }
            join_before = if escaped_break {
                run.iter().skip(1).collect()
            } else {
                fold_break_run(&run)
            };
        }
    }
}

/// Decode one double quoted escape over a character slice.
///
/// The mirror of decode_escape for the flow stream, which
/// works in characters rather than bytes.
fn decode_escape_chars(chars: &[char], at: usize) -> Result<(char, usize), String> {
    let code = chars[at];
    let simple = match code {
        '0' => Some('\u{0000}'),
        'a' => Some('\u{0007}'),
        'b' => Some('\u{0008}'),
        't' | '\t' => Some('\t'),
        'n' => Some('\n'),
        'v' => Some('\u{000B}'),
        'f' => Some('\u{000C}'),
        'r' => Some('\r'),
        'e' => Some('\u{001B}'),
        ' ' => Some(' '),
        '"' => Some('"'),
        '/' => Some('/'),
        '\\' => Some('\\'),
        'N' => Some('\u{0085}'),
        '_' => Some('\u{00A0}'),
        'L' => Some('\u{2028}'),
        'P' => Some('\u{2029}'),
        _ => None,
    };
    if let Some(ch) = simple {
        return Ok((ch, 1));
    }
    let digits = match code {
        'x' => 2,
        'u' => 4,
        'U' => 8,
        _ => {
            return Err(
                "a double quoted scalar carries an escape the subset does not know".to_string(),
            )
        }
    };
    let mut value: u32 = 0;
    for offset in 0..digits {
        let digit_char = match chars.get(at + 1 + offset) {
            Some(ch) => *ch,
            None => return Err("a double quoted escape is not hexadecimal".to_string()),
        };
        let digit = match digit_char.to_digit(16) {
            Some(digit) => digit,
            None => return Err("a double quoted escape is not hexadecimal".to_string()),
        };
        value = value * 16 + digit;
    }
    if (0xD800..0xE000).contains(&value) {
        return Err("a double quoted escape names half of a surrogate pair".to_string());
    }
    match char::from_u32(value) {
        Some(ch) => Ok((ch, 1 + digits)),
        None => Err("a double quoted escape names no character".to_string()),
    }
}

impl Parser {
    fn parse_flow(&mut self, first: &Pend, lead: &str, depth: usize) -> Result<Value, String> {
        let mut source = FlowSource::build(self, first, lead);
        let value = source.node(depth)?;
        let (end_raw, remainder) = source.remainder();
        self.pos = end_raw + 1;
        if !strip_comment(&remainder).trim().is_empty() {
            return Err("content stands after a value on its line".to_string());
        }
        Ok(value)
    }
}

/// Resolve a plain scalar the way PyYAML resolves it.
///
/// The table is the resolver's, probed for the contract.
/// Null words and the empty scalar read as null, the YAML
/// 1.1 bool words read as bools, then the float spellings,
/// then the integer spellings, then timestamps, and
/// everything else stands as text.
fn resolve_plain(text: &str) -> Result<Value, String> {
    if text.is_empty() || matches!(text, "~" | "null" | "Null" | "NULL") {
        return Ok(Value::Null);
    }
    match text {
        "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON" => {
            return Ok(Value::Bool(true));
        }
        "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off" | "Off" | "OFF" => {
            return Ok(Value::Bool(false));
        }
        _ => {}
    }
    if let Some(value) = yaml_float(text) {
        return Ok(Value::Float(value));
    }
    if let Some(value) = yaml_int(text) {
        return Ok(Value::Int(value));
    }
    if let Some(stamp) = yaml_timestamp(text)? {
        return Ok(Value::Timestamp(stamp));
    }
    Ok(Value::Text(text.to_string()))
}

/// The float spellings of the resolver's float pattern.
///
/// The forms are a signed or unsigned decimal with a
/// point and an optional exponent whose sign is required,
/// an unsigned leading point decimal, a sexagesimal form
/// ending in a point part, and the inf and nan spellings.
fn yaml_float(text: &str) -> Option<f64> {
    // The nan spellings carry no sign in the pattern.
    if matches!(text, ".nan" | ".NaN" | ".NAN") {
        return Some(f64::NAN);
    }
    let (negative, rest) = strip_sign(text);
    let signed = rest.len() != text.len();
    if matches!(rest, ".inf" | ".Inf" | ".INF") {
        return Some(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    // Sexagesimal floats fold their parts at sixty.
    if rest.contains(':') {
        return sexagesimal_float(rest).map(|value| if negative { -value } else { value });
    }
    // A decimal with a point. The leading point form
    // stands only unsigned, so a signed text whose body
    // starts with a point is not a float at all.
    if signed {
        if rest.starts_with('.') {
            return None;
        }
        decimal_float(rest).map(|value| if negative { -value } else { value })
    } else {
        decimal_float(text)
    }
}

/// A decimal float body with no sign.
///
/// The shape is digits, a point, and digits, with an
/// optional exponent whose sign is required, or a point
/// first with at least one digit after it.
fn decimal_float(body: &str) -> Option<f64> {
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => {
            let exponent = &body[at + 1..];
            let digits = exponent.strip_prefix(['+', '-'].as_ref())?;
            if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            (&body[..at], Some(&body[at..]))
        }
        None => (body, None),
    };
    let (whole, fraction) = mantissa.split_once('.')?;
    if whole.is_empty() {
        // A leading point needs a digit after it.
        if fraction.is_empty() || !fraction.starts_with(|ch: char| ch.is_ascii_digit()) {
            return None;
        }
    } else if !whole.starts_with(|ch: char| ch.is_ascii_digit())
        || !whole.chars().all(|ch| ch.is_ascii_digit() || ch == '_')
    {
        return None;
    }
    if !fraction.chars().all(|ch| ch.is_ascii_digit() || ch == '_') {
        return None;
    }
    let cleaned: String = mantissa.chars().filter(|ch| *ch != '_').collect();
    let mut literal = cleaned;
    if let Some(exponent) = exponent {
        literal.push_str(exponent);
    }
    literal.parse::<f64>().ok()
}

/// A sexagesimal float body with no sign.
///
/// The parts split at colons, the leading part is whole
/// digits, the middle parts are one or two digits under
/// sixty, and the last part carries the point.
fn sexagesimal_float(body: &str) -> Option<f64> {
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let first = parts[0];
    if first.is_empty()
        || !first.starts_with(|ch: char| ch.is_ascii_digit())
        || !first.chars().all(|ch| ch.is_ascii_digit() || ch == '_')
    {
        return None;
    }
    for middle in &parts[1..parts.len() - 1] {
        if !is_sixtieth(middle) {
            return None;
        }
    }
    let last = parts[parts.len() - 1];
    let (head, fraction) = last.split_once('.')?;
    if !is_sixtieth(head) {
        return None;
    }
    if !fraction.chars().all(|ch| ch.is_ascii_digit() || ch == '_') {
        return None;
    }
    let mut value = 0.0f64;
    let mut base = 1.0f64;
    for part in parts.iter().rev() {
        let cleaned: String = part.chars().filter(|ch| *ch != '_').collect();
        let digit: f64 = cleaned.parse().ok()?;
        value += digit * base;
        base *= 60.0;
    }
    Some(value)
}

/// A one or two digit part under sixty, as the patterns state.
fn is_sixtieth(part: &str) -> bool {
    let bytes = part.as_bytes();
    match bytes.len() {
        1 => bytes[0].is_ascii_digit(),
        2 => (b'0'..=b'5').contains(&bytes[0]) && bytes[1].is_ascii_digit(),
        _ => false,
    }
}

/// The integer spellings of the resolver's int pattern.
fn yaml_int(text: &str) -> Option<IntVal> {
    let (negative, rest) = strip_sign(text);
    if let Some(tail) = rest.strip_prefix("0b") {
        if !tail.is_empty() && tail.chars().all(|ch| matches!(ch, '0' | '1' | '_')) {
            let digits: String = tail.chars().filter(|ch| *ch != '_').collect();
            return Some(IntVal::from_radix(&digits, 2).with_sign(negative));
        }
        return None;
    }
    if let Some(tail) = rest.strip_prefix("0x") {
        if !tail.is_empty() && tail.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '_') {
            let digits: String = tail.chars().filter(|ch| *ch != '_').collect();
            return Some(IntVal::from_radix(&digits, 16).with_sign(negative));
        }
        return None;
    }
    if rest.contains(':') {
        return sexagesimal_int(rest).map(|value| value.with_sign(negative));
    }
    if rest.len() > 1
        && rest.starts_with('0')
        && rest.chars().all(|ch| matches!(ch, '0'..='7' | '_'))
    {
        let digits: String = rest.chars().filter(|ch| *ch != '_').collect();
        return Some(IntVal::from_radix(&digits, 8).with_sign(negative));
    }
    if rest == "0"
        || (rest.starts_with(|ch: char| ('1'..='9').contains(&ch))
            && rest.chars().all(|ch| ch.is_ascii_digit() || ch == '_'))
    {
        let digits: String = rest.chars().filter(|ch| *ch != '_').collect();
        return Some(IntVal::from_decimal(&digits).with_sign(negative));
    }
    None
}

/// A sexagesimal integer body with no sign.
fn sexagesimal_int(body: &str) -> Option<IntVal> {
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let first = parts[0];
    if !first.starts_with(|ch: char| ('1'..='9').contains(&ch))
        || !first.chars().all(|ch| ch.is_ascii_digit() || ch == '_')
    {
        return None;
    }
    for part in &parts[1..] {
        if !is_sixtieth(part) {
            return None;
        }
    }
    let mut value = IntVal::zero();
    for part in &parts {
        let digits: String = part.chars().filter(|ch| *ch != '_').collect();
        value.mul_small(60);
        value.add_magnitude(&IntVal::from_decimal(&digits));
    }
    Some(value)
}

fn strip_sign(text: &str) -> (bool, &str) {
    if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix('+') {
        (false, rest)
    } else {
        (false, text)
    }
}

/// Read a timestamp spelling to its CPython str form.
///
/// The resolver's two shapes are a date alone, which must
/// stand at two digits for the month and the day, and a
/// datetime with one or two digit fields, a T or blank
/// separator, an optional fraction, and an optional zone.
/// A spelling that matches the shape but names a moment
/// that does not stand refuses, because CPython's
/// construction raises there and its readers let it escape.
fn yaml_timestamp(text: &str) -> Result<Option<String>, String> {
    let bytes = text.as_bytes();
    if bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        let year: u32 = text[0..4].parse().expect("four digits stand");
        let month: u32 = text[5..7].parse().expect("two digits stand");
        let day: u32 = text[8..10].parse().expect("two digits stand");
        if !valid_date(year, month, day) {
            return Err("a timestamp names a moment that does not stand".to_string());
        }
        return Ok(Some(format!("{year:04}-{month:02}-{day:02}")));
    }
    parse_datetime(text)
}

fn parse_datetime(text: &str) -> Result<Option<String>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    let invalid = || "a timestamp names a moment that does not stand".to_string();
    let take_digits = |chars: &Vec<char>, at: &mut usize, min: usize, max: usize| -> Option<u32> {
        let mut value: u32 = 0;
        let mut count = 0usize;
        while *at < chars.len() && chars[*at].is_ascii_digit() && count < max {
            value = value * 10 + chars[*at].to_digit(10)?;
            *at += 1;
            count += 1;
        }
        if count < min {
            return None;
        }
        Some(value)
    };
    let year = match take_digits(&chars, &mut at, 4, 4) {
        Some(year) => year,
        None => return Ok(None),
    };
    if chars.get(at) != Some(&'-') {
        return Ok(None);
    }
    at += 1;
    let month = match take_digits(&chars, &mut at, 1, 2) {
        Some(month) => month,
        None => return Ok(None),
    };
    if chars.get(at) != Some(&'-') {
        return Ok(None);
    }
    at += 1;
    let day = match take_digits(&chars, &mut at, 1, 2) {
        Some(day) => day,
        None => return Ok(None),
    };
    match chars.get(at) {
        Some('T') | Some('t') => at += 1,
        Some(' ') => {
            while matches!(chars.get(at), Some(' ')) {
                at += 1;
            }
            if chars.get(at) == Some(&'\t') {
                return Err(invalid());
            }
        }
        Some('\t') => return Err(invalid()),
        _ => return Ok(None),
    }
    let hour = match take_digits(&chars, &mut at, 1, 2) {
        Some(hour) => hour,
        None => return Ok(None),
    };
    if chars.get(at) != Some(&':') {
        return Ok(None);
    }
    at += 1;
    let minute = match take_digits(&chars, &mut at, 2, 2) {
        Some(minute) => minute,
        None => return Ok(None),
    };
    if chars.get(at) != Some(&':') {
        return Ok(None);
    }
    at += 1;
    let second = match take_digits(&chars, &mut at, 2, 2) {
        Some(second) => second,
        None => return Ok(None),
    };
    let mut micros: u32 = 0;
    if chars.get(at) == Some(&'.') {
        at += 1;
        let mut fraction = String::new();
        while matches!(chars.get(at), Some(ch) if ch.is_ascii_digit()) {
            fraction.push(chars[at]);
            at += 1;
        }
        if !fraction.is_empty() {
            let taken: String = fraction.chars().take(6).collect();
            let padded = format!("{:0<6}", taken);
            micros = padded.parse().expect("six digits stand");
        }
    }
    // The optional zone, after optional blanks.
    let mut zone_text = String::new();
    {
        let mut look = at;
        while matches!(chars.get(look), Some(' ') | Some('\t')) {
            look += 1;
        }
        match chars.get(look) {
            None => {}
            Some('Z') => {
                zone_text = "+00:00".to_string();
                at = look + 1;
            }
            Some(sign @ ('+' | '-')) => {
                let sign = *sign;
                let mut cursor = look + 1;
                let tz_hour = match take_digits(&chars, &mut cursor, 1, 2) {
                    Some(hour) => hour,
                    None => return Ok(None),
                };
                let mut tz_minute = 0u32;
                if chars.get(cursor) == Some(&':') {
                    cursor += 1;
                    match take_digits(&chars, &mut cursor, 2, 2) {
                        Some(minute) => tz_minute = minute,
                        None => return Ok(None),
                    }
                }
                if tz_hour * 60 + tz_minute >= 24 * 60 {
                    return Err(invalid());
                }
                zone_text = format!("{sign}{tz_hour:02}:{tz_minute:02}");
                at = cursor;
            }
            Some(_) => return Ok(None),
        }
    }
    if at != chars.len() {
        return Ok(None);
    }
    if !valid_date(year, month, day) || hour > 23 || minute > 59 || second > 59 {
        return Err(invalid());
    }
    let mut out = format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}");
    if micros != 0 {
        out.push_str(&format!(".{micros:06}"));
    }
    out.push_str(&zone_text);
    Ok(Some(out))
}

fn valid_date(year: u32, month: u32, day: u32) -> bool {
    // A year of zero names no moment. CPython's dates and
    // datetimes start at year one, and its construction
    // raises for a year of zero and lets the error escape.
    if year == 0 {
        return false;
    }
    if !(1..=12).contains(&month) {
        return false;
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    day >= 1 && day <= days
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar(text: &str) -> Value {
        parse_yaml(&format!("k: {text}"))
            .expect("the document reads")
            .get("k")
            .expect("the key stands")
            .clone()
    }

    #[test]
    fn the_resolution_table_holds_its_trap_rows() {
        // Single letters are text, the YAML 1.1 words are bools.
        assert_eq!(scalar("y"), Value::Text("y".to_string()));
        assert_eq!(scalar("n"), Value::Text("n".to_string()));
        assert_eq!(scalar("Yes"), Value::Bool(true));
        assert_eq!(scalar("NO"), Value::Bool(false));
        assert_eq!(scalar("on"), Value::Bool(true));
        assert_eq!(scalar("Off"), Value::Bool(false));
        // Null spellings.
        assert_eq!(scalar("~"), Value::Null);
        assert_eq!(scalar("Null"), Value::Null);
        // Integers in their bases, with the traps.
        assert_eq!(scalar("07"), Value::Int(IntVal::from_decimal("7")));
        assert_eq!(scalar("08"), Value::Text("08".to_string()));
        assert_eq!(scalar("0o17"), Value::Text("0o17".to_string()));
        assert_eq!(scalar("0x1F"), Value::Int(IntVal::from_decimal("31")));
        assert_eq!(scalar("0b101"), Value::Int(IntVal::from_decimal("5")));
        assert_eq!(scalar("1_000"), Value::Int(IntVal::from_decimal("1000")));
        assert_eq!(scalar("1:30"), Value::Int(IntVal::from_decimal("90")));
        assert_eq!(scalar("0:30"), Value::Text("0:30".to_string()));
        assert_eq!(scalar("-0"), Value::Int(IntVal::zero()));
        // Floats, with the leading point and exponent traps.
        assert_eq!(scalar("-.5"), Value::Text("-.5".to_string()));
        assert_eq!(scalar("+.5"), Value::Text("+.5".to_string()));
        assert_eq!(scalar(".5"), Value::Float(0.5));
        assert_eq!(scalar("1."), Value::Float(1.0));
        assert_eq!(scalar("1e3"), Value::Text("1e3".to_string()));
        assert_eq!(scalar("1.5e+3"), Value::Float(1500.0));
        assert_eq!(scalar("1:30.5"), Value::Float(90.5));
        assert_eq!(scalar(".inf"), Value::Float(f64::INFINITY));
        assert_eq!(scalar("-.Inf"), Value::Float(f64::NEG_INFINITY));
        match scalar(".NaN") {
            Value::Float(value) => assert!(value.is_nan()),
            other => panic!("expected a NaN float, found {other:?}"),
        }
        // Quoted spellings never resolve.
        let quoted = parse_yaml("k: 'on'\nj: \"1:30\"\n").expect("the document reads");
        assert_eq!(quoted.get("k"), Some(&Value::Text("on".to_string())));
        assert_eq!(quoted.get("j"), Some(&Value::Text("1:30".to_string())));
        // Timestamps carry the CPython str form.
        assert_eq!(
            scalar("2024-01-01"),
            Value::Timestamp("2024-01-01".to_string())
        );
        assert_eq!(
            scalar("2001-02-03 04:05:06.7"),
            Value::Timestamp("2001-02-03 04:05:06.700000".to_string())
        );
        assert_eq!(
            scalar("2001-02-03T04:05:06+5"),
            Value::Timestamp("2001-02-03 04:05:06+05:00".to_string())
        );
        assert_eq!(
            scalar("2001-02-03t04:05:06.78Z"),
            Value::Timestamp("2001-02-03 04:05:06.780000+00:00".to_string())
        );
    }

    #[test]
    fn block_and_flow_shapes_read() {
        let document = parse_yaml(
            "checks:\n- name: a\n  job: j\n- name: b\n  job: k\nother: [1, two]\nflow: {x: 1}\n",
        )
        .expect("the document reads");
        let checks = document.get("checks").expect("checks stand");
        assert_eq!(checks.as_list().map(|items| items.len()), Some(2));
        assert_eq!(
            document.get("other"),
            Some(&Value::List(vec![
                Value::Int(IntVal::from_decimal("1")),
                Value::Text("two".to_string())
            ]))
        );
        // Duplicate keys keep the last value.
        let dup = parse_yaml("a: 1\na: 2\n").expect("the document reads");
        assert_eq!(dup.get("a"), Some(&Value::Int(IntVal::from_decimal("2"))));
        // Plain scalars fold their continuation lines.
        let folded = parse_yaml("k: some long\n  text here\nnext: 1\n").expect("reads");
        assert_eq!(
            folded.get("k"),
            Some(&Value::Text("some long text here".to_string()))
        );
        let blank = parse_yaml("k: first\n\n  second\nnext: 1\n").expect("reads");
        assert_eq!(
            blank.get("k"),
            Some(&Value::Text("first\nsecond".to_string()))
        );
        // An empty document is a null.
        assert_eq!(parse_yaml(""), Ok(Value::Null));
        assert_eq!(parse_yaml("# only a comment\n"), Ok(Value::Null));
        // Markers stand around the document.
        let marked = parse_yaml("---\na: 1\n...\n").expect("reads");
        assert_eq!(
            marked.get("a"),
            Some(&Value::Int(IntVal::from_decimal("1")))
        );
        // A flow collection on a sequence entry line is a
        // flow value, not a block mapping whose key carries
        // the flow colons.
        let flowed = parse_yaml("- {a: 1, b: 2}\n- [x, y]\n").expect("reads");
        let items = flowed.as_list().expect("a sequence stands");
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].get("a"),
            Some(&Value::Int(IntVal::from_decimal("1")))
        );
        assert_eq!(
            items[1],
            Value::List(vec![
                Value::Text("x".to_string()),
                Value::Text("y".to_string())
            ])
        );
    }

    #[test]
    fn the_refused_family_refuses() {
        for text in [
            "k: &anchor v\n",
            "k: *alias\n",
            "base: {x: 1}\nk:\n  <<: base\n",
            "k: !!str v\n",
            "k: |\n  a block scalar\n",
            "k: >\n  a block scalar\n",
            "? explicit\n: value\n",
            "%YAML 1.1\n---\na: 1\n",
            "---\na: 1\n---\nb: 2\n",
            "k:\n\ttabbed: 1\n",
            "k: a: b\n",
        ] {
            assert!(parse_yaml(text).is_err(), "document {text:?}");
        }
    }

    #[test]
    fn year_zero_timestamps_refuse() {
        // CPython's construction raises for a year of zero
        // and lets the error escape, so the port refuses
        // the same spellings in its own words. The first
        // standing year still resolves.
        assert!(parse_yaml("k: 0000-01-01\n").is_err());
        assert!(parse_yaml("k: 0000-06-15 04:05:06\n").is_err());
        assert_eq!(
            scalar("0001-01-01"),
            Value::Timestamp("0001-01-01".to_string())
        );
    }

    #[test]
    fn the_refused_family_refuses_in_key_position() {
        // PORT4-2 states the family refuses, and a key led
        // by one of its markers is the family standing in
        // key position, in block and flow mappings both.
        for text in [
            "&a k: v\n",
            "!!str k: v\n",
            "*a: v\n",
            "{&a k: 1}\n",
            "{!!str k: 1}\n",
            "{*a: 1}\n",
            "{<<: base}\n",
            "[<<: base]\n",
        ] {
            assert!(parse_yaml(text).is_err(), "document {text:?}");
        }
        // A marker character away from the lead is plain
        // text in a key, as the Python reads it.
        let control = parse_yaml("a&b: v\n").expect("the document reads");
        assert_eq!(control.get("a&b"), Some(&Value::Text("v".to_string())));
    }

    #[test]
    fn a_byte_order_mark_opens_the_document() {
        // PyYAML's stream reading strips one leading mark,
        // and a second mark is data in the first key.
        let marked = parse_yaml("\u{FEFF}a: 1\n").expect("the document reads");
        assert_eq!(
            marked.get("a"),
            Some(&Value::Int(IntVal::from_decimal("1")))
        );
        let commented = parse_yaml("\u{FEFF}# c\na: 1\n").expect("the document reads");
        assert_eq!(
            commented.get("a"),
            Some(&Value::Int(IntVal::from_decimal("1")))
        );
        assert_eq!(parse_yaml("\u{FEFF}"), Ok(Value::Null));
        let doubled = parse_yaml("\u{FEFF}\u{FEFF}a: 1\n").expect("the document reads");
        assert_eq!(
            doubled.get("\u{FEFF}a"),
            Some(&Value::Int(IntVal::from_decimal("1")))
        );
    }

    #[test]
    fn the_break_set_splits_block_lines() {
        // PyYAML's break set beside the newline and the
        // CRLF pair. Each document reads as the same map.
        for text in [
            "a: 1\rb: 2\n",
            "a: 1\u{0085}b: 2\n",
            "a: 1\u{2028}b: 2\n",
            "a: 1\u{2029}b: 2\n",
        ] {
            let doc = parse_yaml(text).expect("the document reads");
            assert_eq!(
                doc.get("a"),
                Some(&Value::Int(IntVal::from_decimal("1"))),
                "document {text:?}"
            );
            assert_eq!(
                doc.get("b"),
                Some(&Value::Int(IntVal::from_decimal("2"))),
                "document {text:?}"
            );
        }
    }

    #[test]
    fn breaks_inside_quoted_scalars_follow_the_probe() {
        // A carriage return and a NEL fold inside a quoted
        // scalar, as a newline does. The wide breaks are
        // data there, and the escaped NEL is data, because
        // the break handling is a scanner rule and not a
        // text substitution.
        assert_eq!(scalar("'a\u{0085}b'"), Value::Text("a b".to_string()));
        assert_eq!(scalar("'a\rb'"), Value::Text("a b".to_string()));
        assert_eq!(
            scalar("'a\u{2028}b'"),
            Value::Text("a\u{2028}b".to_string())
        );
        assert_eq!(
            scalar("\"a\u{2029}b\""),
            Value::Text("a\u{2029}b".to_string())
        );
        assert_eq!(scalar("\"a\\x85b\""), Value::Text("a\u{0085}b".to_string()));
        let flowed = parse_yaml("k: ['a\u{2028}b']\n").expect("the document reads");
        assert_eq!(
            flowed.get("k"),
            Some(&Value::List(vec![Value::Text("a\u{2028}b".to_string())]))
        );
    }

    #[test]
    fn scanner_refused_plain_shapes_refuse() {
        // Shapes PyYAML's scanner refuses, each producing
        // a value in the port before this fix.
        for text in [
            "@x\n",
            "`x\n",
            "k: @x\n",
            "k: `x\n",
            "@x: v\n",
            "- @x\n",
            "{k: @x}\n",
            "k: %x\n",
            "- %x\n",
            "{k: %x}\n",
            "k: -\n",
            "k: - x\n",
            "k:\t1\n",
            "\"k\":\t1\n",
            "k\t: 1\n",
            "k: \t1\n",
            "{k:\t1}\n",
            ": v\n",
            ":\n",
            "--- a: 1\n",
            "--- 'k': v\n",
        ] {
            assert!(parse_yaml(text).is_err(), "document {text:?}");
        }
        // The same characters away from a token lead read.
        assert_eq!(scalar("a@b"), Value::Text("a@b".to_string()));
        assert_eq!(scalar("a`b"), Value::Text("a`b".to_string()));
        assert_eq!(scalar("a%b"), Value::Text("a%b".to_string()));
        assert_eq!(scalar("--"), Value::Text("--".to_string()));
        assert_eq!(scalar("-5"), Value::Int(IntVal::from_decimal("-5")));
        let keyed = parse_yaml("a@b: v\n").expect("the document reads");
        assert_eq!(keyed.get("a@b"), Some(&Value::Text("v".to_string())));
    }

    #[test]
    fn scanner_refused_plain_shapes_refuse_second_family() {
        // The red team's final F3 probes. Each shape
        // produced a value in the port before this fix.
        for text in [
            "k: ?\n",
            "k: ? x\n",
            "k: :\n",
            "k: : x\n",
            "k: 2001-02-03\t04:05:06\n",
            "{\"a\nb\": 1}\n",
            "{'a\nb': 1}\n",
            "[\"a\nb\": 1]\n",
        ] {
            assert!(parse_yaml(text).is_err(), "document {text:?}");
        }
        // The same characters away from the token lead
        // read, and a quoted value spanning lines in flow
        // still folds, because only the key refuses.
        assert_eq!(scalar("?x"), Value::Text("?x".to_string()));
        assert_eq!(scalar(":x"), Value::Text(":x".to_string()));
        let folded = parse_yaml("{k: \"a\nb\"}\n").expect("the document reads");
        assert_eq!(folded.get("k"), Some(&Value::Text("a b".to_string())));
    }

    #[test]
    fn nan_keys_share_one_entry_in_a_document() {
        // Both readers hand back one shared NaN object for
        // every .nan spelling in a document, and a dict
        // keeps the last value for the identical key
        // object, so the reader collapses the two keys
        // here while the value model's rule for distinct
        // NaN objects stands untouched.
        let doc = parse_yaml(".nan: a\n.nan: b\n").expect("the document reads");
        let entries = doc.as_map().expect("a map stands");
        assert_eq!(entries.len(), 1);
        match &entries[0].0 {
            Value::Float(value) => assert!(value.is_nan()),
            other => panic!("expected a NaN key, found {other:?}"),
        }
        assert_eq!(entries[0].1, Value::Text("b".to_string()));
        let mixed = parse_yaml(".NaN: a\n.NAN: b\n").expect("the document reads");
        assert_eq!(mixed.as_map().expect("a map stands").len(), 1);
    }

    #[test]
    fn a_tab_inside_a_plain_scalar_refuses() {
        // Fix pass 2, Fix A. PyYAML's scanner refuses a
        // tab that stands inside a plain scalar token in
        // every position the probes tried, and the port
        // read a value for these shapes before this fix.
        for text in [
            "k: a\tb\n",
            "k: a\tb\tc\n",
            "k: a \tb\n",
            "k: ab\t\n",
            "k: ab\t# a comment\n",
            "a\tb: v\n",
            "ab\t: v\n",
            "a\tb\n",
            "- a\tb\n",
            "k:\n- a\tb\n",
            "k: a\n  b\tc\n",
            "{k: a\tb}\n",
            "k: [a\tb]\n",
            "{a\tb: v}\n",
        ] {
            assert!(parse_yaml(text).is_err(), "document {text:?}");
        }
        // A tab inside a quoted scalar is data, a tab
        // inside a comment is comment text, and the
        // double quoted tab escape reads as a tab.
        assert_eq!(scalar("'a\tb'"), Value::Text("a\tb".to_string()));
        assert_eq!(scalar("\"a\tb\""), Value::Text("a\tb".to_string()));
        let commented = parse_yaml("k: a # comment\twords\n").expect("the document reads");
        assert_eq!(commented.get("k"), Some(&Value::Text("a".to_string())));
        assert_eq!(scalar("\"a\\tb\""), Value::Text("a\tb".to_string()));
    }

    #[test]
    fn a_wide_break_at_a_quoted_lines_end_folds_as_the_python_folds() {
        // Fix pass 2, Fix B. A wide break standing at the
        // very end of a quoted scalar's line, just before
        // a narrow break, folds the way PyYAML folds it.
        // The line separator and the paragraph separator
        // stand in the value as data, and one newline
        // follows each. The port joined the segments with
        // a space before this fix. The expected values are
        // the probe's own, not a paraphrase.
        assert_eq!(
            scalar("'a\u{2028}\nb'"),
            Value::Text("a\u{2028}\nb".to_string())
        );
        assert_eq!(
            scalar("\"a\u{2028}\nb\""),
            Value::Text("a\u{2028}\nb".to_string())
        );
        assert_eq!(
            scalar("'a\u{2029}\nb'"),
            Value::Text("a\u{2029}\nb".to_string())
        );
        assert_eq!(
            scalar("'a\u{2028}\r\nb'"),
            Value::Text("a\u{2028}\nb".to_string())
        );
        assert_eq!(
            scalar("'a\u{2028}\n\nb'"),
            Value::Text("a\u{2028}\n\nb".to_string())
        );
        // The same fold in flow context.
        let flowed = parse_yaml("k: ['a\u{2028}\nb']\n").expect("the document reads");
        assert_eq!(
            flowed.get("k"),
            Some(&Value::List(vec![Value::Text("a\u{2028}\nb".to_string())]))
        );
        // A NEL at the same place is itself a break, so
        // the two breaks fold to one newline and no NEL
        // stands in the value. This row read correctly
        // before the fix and stands as a control.
        assert_eq!(scalar("'a\u{0085}\nb'"), Value::Text("a\nb".to_string()));
        // A wide break inside one line of a quoted scalar
        // is data and stands as it stood.
        assert_eq!(
            scalar("'a\u{2028}b'"),
            Value::Text("a\u{2028}b".to_string())
        );
    }
}
