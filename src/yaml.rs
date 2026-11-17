//! The YAML subset reader PORT4-2 states.
//!
//! The subset carries block mappings and sequences at any
//! nesting the bound allows, flow mappings and sequences,
//! plain scalars, single quoted scalars, and double quoted
//! scalars with the YAML escape set, block scalars under
//! the slice 19 readings, explicit keys under the slice 22
//! readings, with comments, an optional
//! document start marker, and a document end marker.
//! Plain scalar resolution follows PyYAML's YAML
//! 1.1 table as the round's probes recorded it. The refused
//! family is the contract's stated port limit. Anchors and
//! aliases, merge keys, the carried tags, and directives
//! read under their slices, and multiple documents and
//! tabs in indentation refuse. Nesting is bounded at four
//! thousand and the
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

/// The anchors a document has registered, in the order
/// they registered. A name standing with no value yet is
/// an anchor whose value is still composing, and an alias
/// that reaches it stands in the recursive spelling S20-3
/// states as this port's limit.
#[derive(Default)]
struct AnchorTable {
    entries: Vec<(String, Option<Value>)>,
}

impl AnchorTable {
    fn begin(&mut self, name: &str) -> Result<(), String> {
        if self.entries.iter().any(|(held, _)| held == name) {
            return Err("the same anchor stands twice in the document".to_string());
        }
        self.entries.push((name.to_string(), None));
        Ok(())
    }

    fn complete(&mut self, name: &str, value: &Value) {
        for (held, slot) in self.entries.iter_mut() {
            if held == name {
                *slot = Some(value.clone());
                return;
            }
        }
    }

    fn resolve(&self, name: &str) -> Result<Value, String> {
        for (held, slot) in &self.entries {
            if held == name {
                return match slot {
                    Some(value) => Ok(value.clone()),
                    None => Err(
                        "an alias names the anchor that holds it, which the subset does not carry"
                            .to_string(),
                    ),
                };
            }
        }
        Err("an alias names no anchor in the document".to_string())
    }
}

/// Read the name of an anchor or alias spelling whose
/// first character is its mark. The name is the run of
/// letters, digits, hyphens, and underscores after the
/// mark, the character that ends the run standing as a
/// blank, a flow mark, a colon, or one of the indicator
/// characters. A spelling with no name, and a name ended
/// by a character that cannot end one, refuse under
/// S20-5. Returns the name and the text after it.
fn read_anchor_name(text: &str) -> Result<(&str, &str), String> {
    let body = &text[1..];
    let mut end = 0;
    for ch in body.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return Err(
            "an anchor or alias stands with no name, which the subset does not carry".to_string(),
        );
    }
    let rest = &body[end..];
    if let Some(ch) = rest.chars().next() {
        let can_end = matches!(
            ch,
            ' ' | '\t'
                | '\n'
                | ','
                | '['
                | ']'
                | '{'
                | '}'
                | ':'
                | '#'
                | '&'
                | '*'
                | '!'
                | '|'
                | '>'
                | '\''
                | '"'
                | '%'
                | '@'
                | '`'
                | '?'
        );
        if !can_end {
            return Err(
                "an anchor or alias name carries a character the subset does not read".to_string(),
            );
        }
    }
    Ok((&body[..end], rest))
}

/// The type a tag names, one of the carried types of
/// S21-3. A tag resolves the value it stands on as the
/// type it names, a scalar by its text and a collection
/// by its kind.
#[derive(Clone, Copy, PartialEq)]
enum Tag {
    Str,
    Int,
    Float,
    Bool,
    Null,
    Timestamp,
    Seq,
    Map,
}

/// The carried type a shorthand tag's name names, or
/// nothing where the name stands outside the carried set.
fn tag_for_name(name: &str) -> Option<Tag> {
    match name {
        "str" => Some(Tag::Str),
        "int" => Some(Tag::Int),
        "float" => Some(Tag::Float),
        "bool" => Some(Tag::Bool),
        "null" => Some(Tag::Null),
        "timestamp" => Some(Tag::Timestamp),
        "seq" => Some(Tag::Seq),
        "map" => Some(Tag::Map),
        _ => None,
    }
}

/// The tag handles a document's tag spellings resolve
/// through, under S23-3. The primary handle spells the
/// local tags, the secondary handle the standard
/// shorthand, and a named handle the tags of its own
/// family. With no directive standing, the handles stand
/// at the defaults the earlier slices read under. A tag
/// directive redeclares one handle for the one document
/// that follows its block, and a spelling through a
/// handle expands as the handle's declared prefix
/// standing before the spelling's suffix.
#[derive(Debug, Clone)]
struct TagHandles {
    primary: String,
    secondary: String,
    named: Vec<(String, String)>,
    declared: Vec<String>,
}

impl Default for TagHandles {
    fn default() -> TagHandles {
        TagHandles {
            primary: "!".to_string(),
            secondary: "tag:yaml.org,2002:".to_string(),
            named: Vec::new(),
            declared: Vec::new(),
        }
    }
}

impl TagHandles {
    /// The carried type a handle and suffix expand to,
    /// where the expansion is the full name of a carried
    /// type, and nothing where the handle stands
    /// undeclared or the expansion names no carried type.
    fn resolve(&self, handle: &str, suffix: &str) -> Option<Tag> {
        let prefix: &str = match handle {
            "!" => &self.primary,
            "!!" => &self.secondary,
            _ => &self.named.iter().find(|(held, _)| held == handle)?.1,
        };
        let full = format!("{prefix}{suffix}");
        full.strip_prefix("tag:yaml.org,2002:")
            .and_then(tag_for_name)
    }

    /// Declare a handle for the document under its
    /// directive, refusing where the handle stands
    /// declared already, under S23-5.
    fn declare(&mut self, handle: &str, prefix: String) -> Result<(), String> {
        if self.declared.iter().any(|held| held == handle) {
            return Err(
                "a tag handle stands twice in the document, which the subset does not carry"
                    .to_string(),
            );
        }
        self.declared.push(handle.to_string());
        match handle {
            "!" => self.primary = prefix,
            "!!" => self.secondary = prefix,
            _ => self.named.push((handle.to_string(), prefix)),
        }
        Ok(())
    }
}

/// The carried type a whole tag spelling names under the
/// handles in standing, the spelling standing as nothing
/// for the bare mark, under S21-3 and S23-3. A spelling
/// whose handle is undeclared, whose suffix stands
/// empty, or whose expansion names no carried type
/// refuses under S21-5.
fn tag_spelling_kind(spelling: &str, handles: &TagHandles) -> Result<Option<Tag>, String> {
    let unread = || "a tag the subset does not read stands in the document".to_string();
    if spelling == "!" {
        return Ok(None);
    }
    let rest = &spelling[1..];
    let (handle, suffix): (&str, &str) = if let Some(after) = rest.strip_prefix('!') {
        ("!!", after)
    } else if let Some(close) = rest.find('!') {
        (&spelling[..close + 2], &rest[close + 1..])
    } else {
        ("!", rest)
    };
    if suffix.is_empty() {
        return Err(unread());
    }
    let decoded = decode_tag_name(suffix).ok_or_else(unread)?;
    handles
        .resolve(handle, &decoded)
        .map(Some)
        .ok_or_else(unread)
}

/// Whether a tag directive's handle stands in one of
/// the forms S23-2 states, the primary mark, the mark
/// doubled, or a mark, a word of letters and digits and
/// hyphens and underscores, and a closing mark.
fn handle_is_well_formed(handle: &str) -> bool {
    if handle == "!" || handle == "!!" {
        return true;
    }
    match handle
        .strip_prefix('!')
        .and_then(|rest| rest.strip_suffix('!'))
    {
        Some(word) => {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        }
        None => false,
    }
}

/// Read a tag directive's prefix in the form S23-2
/// states, its percent escapes decoded, under S23-3.
/// A character standing outside the form, or an escape
/// standing malformed, leaves the prefix unread.
fn decode_tag_prefix(prefix: &str) -> Option<String> {
    let mut decoded = String::new();
    let mut chars = prefix.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hi = chars.next()?;
            let lo = chars.next()?;
            let value = u8::from_str_radix(&format!("{hi}{lo}"), 16).ok()?;
            decoded.push(value as char);
        } else if ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                '-' | ';'
                    | '/'
                    | '?'
                    | ':'
                    | '@'
                    | '&'
                    | '='
                    | '+'
                    | '$'
                    | ','
                    | '.'
                    | '_'
                    | '!'
                    | '~'
                    | '*'
                    | '\''
                    | '('
                    | ')'
                    | '['
                    | ']'
            )
        {
            decoded.push(ch);
        } else {
            return None;
        }
    }
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

/// Decode the percent escapes standing in a tag's
/// suffix or in a verbatim tag's name, under S25-2.
/// An escape stands as a percent sign and two hex
/// digits and decodes to the byte the pair names, and
/// the name's bytes, the escape bytes and the plainly
/// standing characters' bytes together, read as UTF-8
/// text. An escape standing cut short or carrying a
/// digit outside the hex digits, and bytes reading as
/// no UTF-8 text, leave the name unread.
fn decode_tag_name(name: &str) -> Option<String> {
    let mut bytes: Vec<u8> = Vec::new();
    let mut chars = name.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hi = chars.next()?;
            let lo = chars.next()?;
            let value = u8::from_str_radix(&format!("{hi}{lo}"), 16).ok()?;
            bytes.push(value);
        } else {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
    }
    String::from_utf8(bytes).ok()
}

/// Read a tag spelling whose first character is its mark.
/// Returns the type it names and the text after it, the
/// type standing as nothing for the bare mark, the
/// non-specific tag the arbiter reads as no tag. A
/// spelling outside the carried set refuses under S21-5,
/// an unknown shorthand name, a local tag, a verbatim tag
/// naming an unread type, and the unread registry names
/// of S21-4 among them.
fn read_tag<'a>(text: &'a str, handles: &TagHandles) -> Result<(Option<Tag>, &'a str), String> {
    let unread = || "a tag the subset does not read stands in the document".to_string();
    if let Some(inner) = text.strip_prefix("!<") {
        let close = inner.find('>').ok_or_else(unread)?;
        let name = &inner[..close];
        let decoded = decode_tag_name(name).ok_or_else(unread)?;
        let kind = decoded
            .strip_prefix("tag:yaml.org,2002:")
            .and_then(tag_for_name)
            .ok_or_else(unread)?;
        return Ok((Some(kind), &inner[close + 1..]));
    }
    // The spelling is the mark and the run of handle and
    // name characters standing after it, a second mark
    // opening the secondary handle or closing a named
    // one, under S23-2.
    let mut end = 1;
    for ch in text[1..].chars() {
        if ch == '!' || ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '%' {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    let spelling = &text[..end];
    if spelling == "!" {
        let body = &text[1..];
        return match body.chars().next() {
            None => Ok((None, body)),
            Some(ch) if ch == ' ' || ch == '\t' || ch == '#' => Ok((None, body)),
            Some(_) => Err(unread()),
        };
    }
    let kind = tag_spelling_kind(spelling, handles)?;
    Ok((kind, &text[end..]))
}

/// Read the properties standing at the head of a value's
/// text, an anchor and a tag in either order, each at most
/// once. Returns the anchor's name where one stands, the
/// tag's type where one stands, and the text after the
/// properties. The refusals are S21-5's and S20-5's own.
fn parse_properties<'a>(
    text: &'a str,
    handles: &TagHandles,
) -> Result<(Option<&'a str>, Option<Tag>, &'a str), String> {
    let mut anchor = None;
    let mut tag = None;
    let mut rest = text.trim_start();
    loop {
        if rest.starts_with('&') {
            if anchor.is_some() {
                return Err(
                    "two anchors stand on one value, which the subset does not carry".to_string(),
                );
            }
            let (name, after) = read_anchor_name(rest)?;
            anchor = Some(name);
            rest = after.trim_start();
        } else if rest.starts_with('!') {
            let (found, after) = read_tag(rest, handles)?;
            if let Some(found) = found {
                if tag.is_some() {
                    return Err(
                        "two tags stand on one value, which the subset does not carry".to_string(),
                    );
                }
                tag = Some(found);
            }
            rest = after.trim_start();
        } else {
            break;
        }
    }
    Ok((anchor, tag, rest))
}

/// Resolve a scalar's text under the type its tag names,
/// under S21-3. The int grammar is PORT4-2's whole with
/// the 0o octal form standing in it, the form the tag's
/// own grammar carries and the plain grammar does not.
/// The float grammar is PORT4-2's with the bare exponent
/// form standing as a float under this tag alone. A text
/// that does not resolve under its tag refuses under
/// S21-5, and a collection tag standing on a scalar
/// refuses there as a value of another kind.
fn resolve_tagged_scalar(tag: Tag, text: &str) -> Result<Value, String> {
    let unresolved = || {
        "a tagged scalar does not resolve as its tag names, which the subset does not carry"
            .to_string()
    };
    match tag {
        Tag::Str => Ok(Value::Text(text.to_string())),
        Tag::Null => Ok(Value::Null),
        Tag::Bool => match text {
            "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON" => {
                Ok(Value::Bool(true))
            }
            "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off" | "Off" | "OFF" => {
                Ok(Value::Bool(false))
            }
            _ => Err(unresolved()),
        },
        Tag::Int => {
            if let Some(value) = yaml_int(text) {
                return Ok(Value::Int(value));
            }
            let (negative, rest) = strip_sign(text);
            if let Some(tail) = rest.strip_prefix("0o") {
                if !tail.is_empty() && tail.chars().all(|ch| matches!(ch, '0'..='7' | '_')) {
                    let digits: String = tail.chars().filter(|ch| *ch != '_').collect();
                    return Ok(Value::Int(
                        IntVal::from_radix(&digits, 8).with_sign(negative),
                    ));
                }
            }
            Err(unresolved())
        }
        Tag::Float => tagged_float(text).map(Value::Float).ok_or_else(unresolved),
        Tag::Timestamp => match yaml_timestamp(text)? {
            Some(stamp) => Ok(Value::Timestamp(stamp)),
            None => Err(unresolved()),
        },
        Tag::Seq | Tag::Map => Err(
            "a tag stands on a value of another kind, which the subset does not carry".to_string(),
        ),
    }
}

/// The float readings of a tagged scalar, PORT4-2's
/// grammar with the bare exponent form standing in it.
fn tagged_float(text: &str) -> Option<f64> {
    if let Some(value) = yaml_float(text) {
        return Some(value);
    }
    let (_, rest) = strip_sign(text);
    let (mantissa, exponent) = rest.split_once(['e', 'E'])?;
    let exponent_digits = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
    if exponent_digits.is_empty() || !exponent_digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    if mantissa.is_empty()
        || !mantissa
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == '_')
        || mantissa.chars().filter(|ch| *ch == '.').count() > 1
        || !mantissa.chars().any(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    text.replace('_', "").parse::<f64>().ok()
}

/// Hold a composed value against the kind its tag names,
/// under S21-3. A collection tag on its own kind reads
/// the value whole, and any other standing refuses under
/// S21-5 as a value of another kind.
fn tag_kind_check(tag: Tag, value: Value) -> Result<Value, String> {
    match (tag, &value) {
        (Tag::Seq, Value::List(_)) | (Tag::Map, Value::Map(_)) => Ok(value),
        _ => Err(
            "a tag stands on a value of another kind, which the subset does not carry".to_string(),
        ),
    }
}

/// The text a routing judgement reads. A line or a
/// value led by an anchor spelling is judged by the text
/// the anchor stands before, because the anchor is never
/// part of the construct it opens: a flow collection
/// after an anchor is a flow collection, and a key
/// spelling after an anchor is a key spelling. A tag
/// standing in the same place is stripped the same way,
/// the properties never opening the construct they stand
/// before.
fn route_text<'a>(text: &'a str, handles: &TagHandles) -> &'a str {
    let trimmed = text.trim_start();
    if trimmed.starts_with('&') || trimmed.starts_with('!') {
        if let Ok((_, _, rest)) = parse_properties(trimmed, handles) {
            if !rest.is_empty() && !rest.starts_with('#') {
                return rest;
            }
        }
    }
    text
}

/// The merge groups a merge key's value contributes, in
/// the order they are laid. A mapping contributes its own
/// pairs. A sequence contributes its sources in the
/// reverse of its order, each source a mapping, as S20-4
/// states. Any other value refuses.
fn merge_groups_of(value: Value) -> Result<Vec<Vec<(Value, Value)>>, String> {
    match value {
        Value::Map(pairs) => Ok(vec![pairs]),
        Value::List(items) => {
            let mut groups = Vec::new();
            for item in items.into_iter().rev() {
                match item {
                    Value::Map(pairs) => groups.push(pairs),
                    _ => {
                        return Err(
                            "a merge key's value is not a mapping, which the subset does not carry"
                                .to_string(),
                        )
                    }
                }
            }
            Ok(groups)
        }
        _ => {
            Err("a merge key's value is not a mapping, which the subset does not carry".to_string())
        }
    }
}

/// Assemble a mapping from its merge groups and its
/// explicit pairs under S20-4. The merged pairs are laid
/// first, group by group, and the explicit pairs after
/// them, every laying under the model's standing insert
/// rule.
fn assemble_mapping(
    merge_groups: Vec<Vec<(Value, Value)>>,
    explicit: Vec<(Value, Value)>,
) -> Value {
    let mut entries: Vec<(Value, Value)> = Vec::new();
    for group in merge_groups {
        for (key, value) in group {
            yaml_map_insert(&mut entries, key, value);
        }
    }
    for (key, value) in explicit {
        yaml_map_insert(&mut entries, key, value);
    }
    Value::Map(entries)
}

struct Parser {
    lines: Vec<RawLine>,
    pos: usize,
    pending: Option<Pend>,
    anchors: AnchorTable,
    pending_tag: Option<Tag>,
    handles: TagHandles,
    version_directive_stood: bool,
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
            anchors: AnchorTable::default(),
            pending_tag: None,
            handles: TagHandles::default(),
            version_directive_stood: false,
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

    /// Read one directive line into the standings it
    /// declares, under S23-2 and S23-3. A reserved
    /// directive is skipped whole, with whatever stands
    /// after its name on its line. The wordings are
    /// S23-5's.
    fn read_directive(&mut self, text: &str) -> Result<(), String> {
        let malformed = || {
            "a malformed directive stands in the document, which the subset does not carry"
                .to_string()
        };
        // A comment stands where a space stands before
        // its mark. A tab standing outside a comment
        // makes the line malformed, as the arbiter's
        // scanner refuses a tab standing in a directive.
        let head = match text.find(" #") {
            Some(at) => &text[..at],
            None => text,
        };
        if head.contains('\t') {
            return Err(malformed());
        }
        let head = head.trim_end_matches(' ');
        let body = &head[1..];
        let name_end = body
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
            .unwrap_or(body.len());
        let name = &body[..name_end];
        if name.is_empty() {
            return Err(malformed());
        }
        let rest = &body[name_end..];
        if !rest.is_empty() && !rest.starts_with(' ') {
            return Err(malformed());
        }
        let args: Vec<&str> = rest.split(' ').filter(|part| !part.is_empty()).collect();
        match name {
            "YAML" => {
                if self.version_directive_stood {
                    return Err(
                        "the version directive stands twice in the document, which the subset does not carry"
                            .to_string(),
                    );
                }
                if args.len() != 1 {
                    return Err(malformed());
                }
                let mut parts = args[0].split('.');
                let major = parts.next().unwrap_or("");
                let minor = parts.next().unwrap_or("");
                if parts.next().is_some()
                    || major.is_empty()
                    || minor.is_empty()
                    || !major.bytes().all(|b| b.is_ascii_digit())
                    || !minor.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(malformed());
                }
                if major.trim_start_matches('0') != "1" {
                    return Err("a directive names a version the subset does not carry".to_string());
                }
                self.version_directive_stood = true;
                Ok(())
            }
            "TAG" => {
                if args.len() != 2 {
                    return Err(malformed());
                }
                if !handle_is_well_formed(args[0]) {
                    return Err(malformed());
                }
                let prefix = decode_tag_prefix(args[1]).ok_or_else(malformed)?;
                self.handles.declare(args[0], prefix)
            }
            _ => Ok(()),
        }
    }

    fn parse_document(&mut self) -> Result<Value, String> {
        // The leading region. A directive block may
        // stand first, each directive read as it stands,
        // and a block that stood must be followed by the
        // start marker. A start marker opens the
        // document, and anything else is the document
        // itself.
        let mut marker_rest: Option<Pend> = None;
        let mut directives_stood = false;
        while let Some(cur) = self.peek() {
            if cur.indent != 0 || !cur.text.starts_with('%') {
                break;
            }
            self.read_directive(&cur.text)?;
            self.consume(&cur);
            directives_stood = true;
        }
        if directives_stood {
            match self.peek() {
                Some(cur) if is_doc_start(cur.text.trim_end()) => {}
                _ => {
                    return Err(
                        "a directive stands with no document start after it, which the subset does not carry"
                            .to_string(),
                    );
                }
            }
        }
        if let Some(cur) = self.peek() {
            let text = cur.text.trim_end();
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
                self.parse_block(0, 0)?
            }
            None => {
                if self.peek().is_none() {
                    Value::Null
                } else {
                    self.parse_block(0, 0)?
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

    fn parse_block(&mut self, parent_indent: usize, depth: usize) -> Result<Value, String> {
        if depth > YAML_DEPTH_BOUND {
            return Err("the document nests deeper than the subset carries".to_string());
        }
        let cur = match self.peek() {
            Some(cur) => cur,
            None => return Ok(Value::Null),
        };
        let text = cur.text.trim_end().to_string();
        let indent = cur.indent;
        let trimmed = text.trim_start();
        if trimmed.starts_with('&') || trimmed.starts_with('!') {
            if let Ok((anchor, tag, rest)) = parse_properties(trimmed, &self.handles) {
                if rest.is_empty() || rest.starts_with('#') {
                    // The properties stand alone on their
                    // line. The block that follows is their
                    // value, and a line standing behind the
                    // line's own indentation ends the block
                    // before it, leaving the value null.
                    if let Some(name) = anchor {
                        self.anchors.begin(name)?;
                    }
                    self.consume(&cur);
                    let followed = match self.peek() {
                        Some(next) if next.indent >= indent => Some(next),
                        _ => None,
                    };
                    let value = match (tag, followed) {
                        (Some(tag), None) => resolve_tagged_scalar(tag, "")?,
                        (Some(tag), Some(next)) => {
                            let next_text = next.text.trim_end().to_string();
                            if is_seq_start(route_text(&next_text, &self.handles))
                                || starts_mapping(route_text(&next_text, &self.handles))
                            {
                                let value = self.parse_block(parent_indent, depth + 1)?;
                                tag_kind_check(tag, value)?
                            } else {
                                self.pending_tag = Some(tag);
                                let value = self.parse_block(parent_indent, depth + 1)?;
                                match self.pending_tag.take() {
                                    Some(tag) => tag_kind_check(tag, value)?,
                                    None => value,
                                }
                            }
                        }
                        (None, Some(_)) => self.parse_block(parent_indent, depth + 1)?,
                        (None, None) => Value::Null,
                    };
                    if let Some(name) = anchor {
                        self.anchors.complete(name, &value);
                    }
                    return Ok(value);
                }
            }
        }
        if is_seq_start(route_text(&text, &self.handles)) {
            return self.parse_sequence(cur.indent, depth);
        }
        if starts_mapping(route_text(&text, &self.handles)) {
            return self.parse_mapping(cur.indent, depth);
        }
        self.parse_value_from(cur, indent, parent_indent, depth)
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
            return self.parse_block(indent, depth + 1);
        }
        if next.indent == indent && is_seq_start(next.text.trim_end()) {
            return self.parse_sequence(indent, depth + 1);
        }
        Ok(Value::Null)
    }

    fn parse_mapping(&mut self, indent: usize, depth: usize) -> Result<Value, String> {
        let mut entries: Vec<(Value, Value)> = Vec::new();
        let mut merge_groups: Vec<Vec<(Value, Value)>> = Vec::new();
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
            if is_value_line(text) {
                // A value indicator standing alone closes
                // the mapping. The structure that opened it
                // reads the line, an explicit entry's value
                // under slice 22 or a refusal above.
                break;
            }
            if is_explicit_entry(text) {
                let (key, value, merge_key) = self.parse_explicit_entry(&cur, indent, depth)?;
                if merge_key {
                    merge_groups.extend(merge_groups_of(value)?);
                } else {
                    yaml_map_insert(&mut entries, key, value);
                }
                continue;
            }
            let split = match self.split_key(&cur)? {
                Some(split) => split,
                None => {
                    return Err("a mapping entry carries no colon".to_string());
                }
            };
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
                self.parse_value_from(rest, indent, indent, depth)?
            };
            if split.merge_key {
                merge_groups.extend(merge_groups_of(value)?);
            } else {
                yaml_map_insert(&mut entries, split.key, value);
            }
        }
        if merge_groups.is_empty() {
            Ok(Value::Map(entries))
        } else {
            Ok(assemble_mapping(merge_groups, entries))
        }
    }

    /// Parse one explicit entry of a block mapping, the
    /// entry whose key stands behind the key indicator of
    /// slice 22. The entry's own line stands consumed on
    /// return, and its value line where one stands.
    ///
    /// The key node stands after the indicator on the same
    /// line or on the lines following at a deeper
    /// indentation, read by the one block reading, and a
    /// collection composing as the key refuses under
    /// S22-5, the arbiter's constructor refusing it as a
    /// key its mappings cannot hold. No key node standing
    /// after the indicator reads as the null key. The value
    /// stands behind a value line at the entry's own
    /// indentation, and no value line leaves the value
    /// null.
    fn parse_explicit_entry(
        &mut self,
        cur: &Pend,
        indent: usize,
        depth: usize,
    ) -> Result<(Value, Value, bool), String> {
        let text = cur.text.trim_end().to_string();
        let rest = text[1..].trim_start_matches(' ').to_string();
        self.consume(cur);
        let key = if strip_comment(&rest).trim().is_empty() {
            match self.peek() {
                Some(next) if next.indent > indent => self.parse_block(indent, depth + 1)?,
                _ => Value::Null,
            }
        } else {
            let key_pend = Pend {
                indent,
                text: rest,
                raw: cur.raw,
            };
            self.pending = Some(key_pend.clone());
            self.parse_block(indent, depth + 1)?
        };
        if matches!(key, Value::List(_) | Value::Map(_)) {
            return Err(
                "a collection stands as an explicit key, which the subset does not carry"
                    .to_string(),
            );
        }
        let merge_key = key == Value::Text("<<".to_string());
        let value = match self.peek() {
            Some(next) if next.indent == indent && is_value_line(next.text.trim_end()) => {
                let next_text = next.text.trim_end().to_string();
                let after = &next_text[1..];
                if after.starts_with('\t') {
                    return Err(
                        "a tab stands in separation position, which the subset does not carry"
                            .to_string(),
                    );
                }
                let spaces = after.len() - after.trim_start_matches(' ').len();
                let after = after.trim_start_matches(' ').to_string();
                self.consume(&next);
                if strip_comment(&after).trim().is_empty() {
                    self.nested_value(indent, depth)?
                } else if let Some(value) = self.compact_value(&next, &after, spaces, depth)? {
                    value
                } else {
                    let rest_pend = Pend {
                        indent: next.indent,
                        text: after,
                        raw: next.raw,
                    };
                    self.pending = Some(rest_pend.clone());
                    self.parse_value_from(rest_pend, indent, indent, depth)?
                }
            }
            _ => Value::Null,
        };
        Ok((key, value, merge_key))
    }

    /// A value opening in compact form on an explicit
    /// entry's value line, under slice 24. The line's
    /// content, the text standing behind the value
    /// indicator, opens a block sequence, a block mapping,
    /// or a further explicit entry at the column the
    /// opening's first mark establishes, and the block
    /// reading that owns the form reads it there. A
    /// property standing before a block sequence opening
    /// or a further explicit entry is not admitted here,
    /// under S24-4, and the standing value reading refuses
    /// it in its own words. Properties before a block
    /// mapping opening stand on its first key, as they
    /// stand on the first key of any mapping line. Returns
    /// None where the content opens none of the three
    /// forms, leaving the standing reading to judge it.
    fn compact_value(
        &mut self,
        line: &Pend,
        content: &str,
        spaces: usize,
        depth: usize,
    ) -> Result<Option<Value>, String> {
        let routed = route_text(content, &self.handles);
        let propertied = content.starts_with('&') || content.starts_with('!');
        let opens_sequence = is_seq_start(routed);
        let opens_entry = is_explicit_entry(routed);
        if !opens_sequence && !opens_entry && !starts_mapping(routed) {
            return Ok(None);
        }
        if propertied && (opens_sequence || opens_entry) {
            return Ok(None);
        }
        if depth + 1 > YAML_DEPTH_BOUND {
            return Err("the document nests deeper than the subset carries".to_string());
        }
        let column = line.indent + 1 + spaces;
        let pend = Pend {
            indent: column,
            text: content.to_string(),
            raw: line.raw,
        };
        self.pending = Some(pend);
        if opens_sequence {
            Ok(Some(self.parse_sequence(column, depth + 1)?))
        } else {
            Ok(Some(self.parse_mapping(column, depth + 1)?))
        }
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
                    Some(next) if next.indent > indent => self.parse_block(indent, depth + 1)?,
                    _ => Value::Null,
                };
                items.push(item);
                continue;
            }
            if rest.starts_with('&') {
                if let Ok((name, after)) = read_anchor_name(&rest) {
                    let after = after.trim_start();
                    if after.is_empty() || after.starts_with('#') {
                        // The anchor stands alone on the
                        // entry's line. A deeper block is
                        // its value, and a sibling entry
                        // standing at this indentation is
                        // not, so the value is null there.
                        self.anchors.begin(name)?;
                        self.consume(&cur);
                        let item = match self.peek() {
                            Some(next) if next.indent > indent => {
                                self.parse_block(indent, depth + 1)?
                            }
                            _ => Value::Null,
                        };
                        self.anchors.complete(name, &item);
                        items.push(item);
                        continue;
                    }
                }
            }
            let item_column = cur.indent + (text.len() - rest.len());
            let rest_pend = Pend {
                indent: item_column,
                text: rest.clone(),
                raw: cur.raw,
            };
            self.consume(&cur);
            self.pending = Some(rest_pend.clone());
            let item = if is_seq_start(route_text(&rest, &self.handles)) {
                self.parse_sequence(item_column, depth + 1)?
            } else if starts_mapping(route_text(&rest, &self.handles)) {
                self.parse_mapping(item_column, depth + 1)?
            } else {
                self.parse_value_from(rest_pend, indent, indent, depth)?
            };
            items.push(item);
        }
        Ok(Value::List(items))
    }
}

/// Whether a line is an explicit entry's value line, a
/// colon standing alone or opening the value's text.
fn is_value_line(text: &str) -> bool {
    text == ":" || text.starts_with(": ") || text.starts_with(":\t")
}

/// Whether a line opens an explicit entry under slice
/// 22, the key indicator followed by a space or standing
/// alone. A question mark against other content is plain
/// scalar content and opens nothing.
fn is_explicit_entry(text: &str) -> bool {
    text == "?" || text.starts_with("? ")
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
    if trimmed == "?" || trimmed.starts_with("? ") {
        // An explicit key opens a block mapping, under
        // slice 22.
        return true;
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
/// so a plain key whose first character is a tag marker
/// refuses in the same words the value reading uses.
/// Anchors and aliases in key position are read under
/// slice 20 and never reach this refusal. A reserved character or a percent
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
        Some(b'@') | Some(b'`') | Some(b'%') => {
            Some("a scalar starts with a reserved character, which the subset does not carry")
        }
        _ => None,
    }
}

impl Parser {
    fn split_key(&mut self, cur: &Pend) -> Result<Option<SplitKey>, String> {
        // Only trailing spaces trim here. A trailing tab
        // stands inside the value's own text, and trimming
        // it away would hide it from the plain scalar
        // reading that refuses it.
        let text = cur.text.trim_end_matches(' ');
        let first = text.as_bytes().first().copied();
        if first == Some(b'&') || first == Some(b'!') {
            // A key standing behind properties. The anchor
            // and the tag stand before the key's own
            // spelling in either order, the anchor
            // registering the key it resolves and the tag
            // resolving the key under the type it names.
            let (anchor, tag, after) = parse_properties(text, &self.handles)?;
            if after.starts_with('*') {
                return Err(if anchor.is_some() {
                    "an anchor stands on an alias, which the subset does not carry".to_string()
                } else {
                    "a tag stands on an alias, which the subset does not carry".to_string()
                });
            }
            let sub = Pend {
                indent: cur.indent,
                text: after.to_string(),
                raw: cur.raw,
            };
            self.pending_tag = tag;
            let split = self.split_key(&sub)?;
            let mut split = split;
            if self.pending_tag.take().is_some() {
                return Err(
                    "a tag stands on a value of another kind, which the subset does not carry"
                        .to_string(),
                );
            }
            if let Some(found) = &mut split {
                if tag.is_some() {
                    found.merge_key = false;
                }
                if let Some(name) = anchor {
                    self.anchors.begin(name)?;
                    self.anchors.complete(name, &found.key);
                }
            }
            return Ok(split);
        }
        match first {
            Some(b'?') => {
                // An explicit entry never reaches this
                // split, the mapping reading taking it
                // under slice 22. A tab after the indicator
                // refuses in the tab family, as the
                // arbiter's scanner refuses it.
                let second = text.as_bytes().get(1).copied();
                if second == Some(b'\t') {
                    return Err(
                        "a tab stands in separation position, which the subset does not carry"
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
                    Some((key, rest)) => {
                        let key = match self.pending_tag.take() {
                            Some(tag) => resolve_tagged_scalar(tag, &key)?,
                            None => Value::Text(key),
                        };
                        Ok(Some(SplitKey {
                            key,
                            rest,
                            merge_key: false,
                        }))
                    }
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
                let key = if key_text.starts_with('*') {
                    if self.pending_tag.is_some() {
                        return Err(
                            "a tag stands on an alias, which the subset does not carry".to_string()
                        );
                    }
                    let (name, _) = read_anchor_name(&key_text)?;
                    let value = self.anchors.resolve(name)?;
                    if matches!(value, Value::List(_) | Value::Map(_)) {
                        return Err(
                            "an alias names a collection standing as a key, which the subset does not carry"
                                .to_string(),
                        );
                    }
                    value
                } else {
                    match self.pending_tag.take() {
                        Some(tag) => resolve_tagged_scalar(tag, &key_text)?,
                        None => resolve_plain(&key_text)?,
                    }
                };
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
        parent_indent: usize,
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
                match self.pending_tag.take() {
                    Some(tag) => resolve_tagged_scalar(tag, &value),
                    None => Ok(Value::Text(value)),
                }
            }
            Some(b'&') | Some(b'!') => {
                let (anchor, tag, rest) = parse_properties(&lead, &self.handles)?;
                if rest.starts_with('*') {
                    return Err(if anchor.is_some() {
                        "an anchor stands on an alias, which the subset does not carry".to_string()
                    } else {
                        "a tag stands on an alias, which the subset does not carry".to_string()
                    });
                }
                if let Some(name) = anchor {
                    self.anchors.begin(name)?;
                }
                let value = if rest.is_empty() || rest.starts_with('#') {
                    // The value stands on the lines after.
                    // A following collection composes whole
                    // and the tag judges its kind, a
                    // following scalar composes under the
                    // tag through the pending reading, and
                    // no following block leaves the empty
                    // plain scalar standing under the tag.
                    let following = match self.peek() {
                        Some(next) if next.indent > block_indent => {
                            let next_text = next.text.trim_end().to_string();
                            if is_seq_start(route_text(&next_text, &self.handles))
                                || starts_mapping(route_text(&next_text, &self.handles))
                            {
                                Some(true)
                            } else {
                                Some(false)
                            }
                        }
                        Some(next)
                            if next.indent == block_indent
                                && is_seq_start(next.text.trim_end()) =>
                        {
                            Some(true)
                        }
                        _ => None,
                    };
                    match (tag, following) {
                        (Some(tag), Some(true)) => {
                            let value = self.nested_value(block_indent, depth)?;
                            tag_kind_check(tag, value)?
                        }
                        (Some(tag), Some(false)) => {
                            self.pending_tag = Some(tag);
                            let value = self.nested_value(block_indent, depth)?;
                            match self.pending_tag.take() {
                                Some(tag) => tag_kind_check(tag, value)?,
                                None => value,
                            }
                        }
                        (Some(tag), None) => match tag {
                            Tag::Seq | Tag::Map => {
                                return Err("a tag stands on a value of another kind, which the subset does not carry".to_string());
                            }
                            _ => resolve_tagged_scalar(tag, "")?,
                        },
                        (None, _) => self.nested_value(block_indent, depth)?,
                    }
                } else {
                    self.pending_tag = tag;
                    let pend = Pend {
                        indent: first.indent,
                        text: rest.to_string(),
                        raw: first.raw,
                    };
                    let value = self.parse_value_from(pend, block_indent, parent_indent, depth)?;
                    match self.pending_tag.take() {
                        Some(tag) => tag_kind_check(tag, value)?,
                        None => value,
                    }
                };
                if let Some(name) = anchor {
                    self.anchors.complete(name, &value);
                }
                Ok(value)
            }
            Some(b'*') => {
                let (name, rest) = read_anchor_name(&lead)?;
                if !strip_comment(rest).trim().is_empty() {
                    return Err("content stands after a value on its line".to_string());
                }
                if self.pending_tag.is_some() {
                    return Err(
                        "a tag stands on an alias, which the subset does not carry".to_string()
                    );
                }
                self.anchors.resolve(name)
            }
            Some(b'|') | Some(b'>') => {
                let value = self.parse_block_scalar(&lead, parent_indent, block_indent)?;
                match (self.pending_tag.take(), value) {
                    (Some(tag), Value::Text(text)) => resolve_tagged_scalar(tag, &text),
                    (_, value) => Ok(value),
                }
            }
            Some(b'@') | Some(b'`') | Some(b'%') => Err(
                "a scalar starts with a reserved character, which the subset does not carry"
                    .to_string(),
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
                        "a mapping entry stands in place of a value, which the subset does not carry"
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
                match self.pending_tag.take() {
                    Some(tag) => resolve_tagged_scalar(tag, &folded),
                    None => resolve_plain(&folded),
                }
            }
        }
    }

    /// Read a block scalar under the slice 19 obligations.
    ///
    /// The header stands in the lead as the literal mark or
    /// the folded mark, carrying at most one chomping mark
    /// and at most one indentation digit, in either order,
    /// with nothing after them but a comment. The content
    /// lines are read raw from the line list, because the
    /// peek reading skips the blank lines this reading
    /// counts. The block indentation is the digit's count
    /// past the parent indentation where a digit stands,
    /// and the first content line's own indentation where
    /// none stands, and a first content line standing at or
    /// behind the header line's indentation leaves the
    /// block empty.
    fn parse_block_scalar(
        &mut self,
        lead: &str,
        parent_indent: usize,
        header_indent: usize,
    ) -> Result<Value, String> {
        let junk =
            || "a block scalar header carries a character the subset does not read".to_string();
        let bytes = lead.as_bytes();
        let folded = bytes[0] == b'>';
        let mut chomping: Option<u8> = None;
        let mut digit: Option<usize> = None;
        let mut index = 1usize;
        while index < bytes.len() {
            match bytes[index] {
                b'+' | b'-' => {
                    if chomping.is_some() {
                        return Err(junk());
                    }
                    chomping = Some(bytes[index]);
                    index += 1;
                }
                b'1'..=b'9' => {
                    if digit.is_some() {
                        return Err(junk());
                    }
                    digit = Some((bytes[index] - b'0') as usize);
                    index += 1;
                }
                b' ' => {
                    if !strip_comment(&lead[index..]).trim().is_empty() {
                        return Err(junk());
                    }
                    index = bytes.len();
                }
                _ => return Err(junk()),
            }
        }
        // The final piece of a text that ends on a break is
        // a phantom line holding nothing, the break that
        // made it being the last real line's closing break.
        let total = self.lines.len();
        let phantom = if total > 0
            && self.lines[total - 1].text.is_empty()
            && self.lines[total - 1].indent == 0
        {
            total - 1
        } else {
            total
        };
        let closing = |at: usize| -> Option<char> {
            if at + 1 < total {
                Some(self.lines[at + 1].wide_before.unwrap_or('\n'))
            } else {
                None
            }
        };
        struct BlockLine {
            text: String,
            deep: bool,
            blank: bool,
            closing: Option<char>,
        }
        let mut block_indent: Option<usize> = digit.map(|d| parent_indent + d);
        let mut items: Vec<BlockLine> = Vec::new();
        let mut at = self.pos;
        while at < phantom {
            let line = &self.lines[at];
            let blank = line.text.is_empty();
            if block_indent.is_none() {
                if blank {
                    items.push(BlockLine {
                        text: String::new(),
                        deep: false,
                        blank: true,
                        closing: closing(at),
                    });
                    at += 1;
                    continue;
                }
                if line.indent <= header_indent {
                    items.clear();
                    break;
                }
                block_indent = Some(line.indent);
            }
            let bound = block_indent.unwrap_or(0);
            if blank {
                if line.indent > bound {
                    items.push(BlockLine {
                        text: " ".repeat(line.indent - bound),
                        deep: true,
                        blank: false,
                        closing: closing(at),
                    });
                } else {
                    items.push(BlockLine {
                        text: String::new(),
                        deep: false,
                        blank: true,
                        closing: closing(at),
                    });
                }
                at += 1;
                continue;
            }
            if line.indent < bound {
                break;
            }
            items.push(BlockLine {
                text: format!("{}{}", " ".repeat(line.indent - bound), line.text),
                deep: line.indent > bound,
                blank: false,
                closing: closing(at),
            });
            at += 1;
        }
        self.pos = at;
        let last_content = match items.iter().rposition(|item| !item.blank) {
            Some(at) => at,
            None => return Ok(Value::Text(String::new())),
        };
        let mut out = String::new();
        if !folded {
            for item in &items[..last_content] {
                out.push_str(&item.text);
                if let Some(c) = item.closing {
                    out.push(c);
                }
            }
            out.push_str(&items[last_content].text);
        } else {
            let first_content = items.iter().position(|item| !item.blank).unwrap_or(0);
            for item in &items[..first_content] {
                if let Some(c) = item.closing {
                    out.push(c);
                }
            }
            out.push_str(&items[first_content].text);
            let mut prev = first_content;
            for at in (first_content + 1)..=last_content {
                if items[at].blank {
                    continue;
                }
                let gap: Vec<Option<char>> =
                    items[prev..at].iter().map(|item| item.closing).collect();
                let both_at = !items[prev].deep && !items[at].deep;
                let all_plain = gap.iter().all(|c| *c == Some('\n'));
                if both_at && all_plain && gap.len() == 1 {
                    out.push(' ');
                } else if both_at && all_plain {
                    for _ in 0..gap.len() - 1 {
                        out.push('\n');
                    }
                } else {
                    for c in gap.into_iter().flatten() {
                        out.push(c);
                    }
                }
                out.push_str(&items[at].text);
                prev = at;
            }
        }
        match chomping {
            Some(b'-') => {}
            Some(b'+') => {
                if let Some(c) = items[last_content].closing {
                    out.push(c);
                }
                for item in &items[last_content + 1..] {
                    if let Some(c) = item.closing {
                        out.push(c);
                    }
                }
            }
            _ => {
                if let Some(c) = items[last_content].closing {
                    out.push(c);
                }
            }
        }
        Ok(Value::Text(out))
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
        // The head stands trimmed of trailing blanks, so a
        // colon at its end is a mapping value standing
        // inside the scalar, the same rule the continuation
        // segments below carry.
        if head.contains(": ") || head.ends_with(':') {
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
struct FlowSource<'a> {
    chars: Vec<char>,
    line_of: Vec<usize>,
    col_of: Vec<usize>,
    segs: Vec<(usize, String)>,
    pos: usize,
    prev: Option<char>,
    anchors: &'a mut AnchorTable,
    pending_tag: Option<Tag>,
    handles: TagHandles,
}

impl<'a> FlowSource<'a> {
    fn build(
        lines: &[RawLine],
        first: &Pend,
        lead: &str,
        anchors: &'a mut AnchorTable,
        handles: TagHandles,
    ) -> FlowSource<'a> {
        let mut segs: Vec<(usize, String)> = vec![(first.raw, lead.to_string())];
        for (offset, line) in lines[(first.raw + 1)..].iter().enumerate() {
            let raw = first.raw + 1 + offset;
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
                chars.push(lines[*raw].wide_before.unwrap_or('\n'));
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
            anchors,
            pending_tag: None,
            handles,
        }
    }

    /// Read a tag spelling the reading stands on,
    /// consuming its mark and its name, under the same
    /// standings read_tag states for a line. Returns the
    /// type it names, standing as nothing for the bare
    /// mark.
    fn flow_tag(&mut self) -> Result<Option<Tag>, String> {
        let unread = || "a tag the subset does not read stands in the document".to_string();
        self.bump();
        if self.peek() == Some('<') {
            self.bump();
            let mut name = String::new();
            loop {
                match self.bump() {
                    Some('>') => break,
                    Some(ch) => name.push(ch),
                    None => return Err(unread()),
                }
            }
            let decoded = decode_tag_name(&name).ok_or_else(unread)?;
            let kind = decoded
                .strip_prefix("tag:yaml.org,2002:")
                .and_then(tag_for_name)
                .ok_or_else(unread)?;
            return Ok(Some(kind));
        }
        // The spelling is the mark and the run of handle
        // and name characters standing after it, read as
        // a line's spelling reads, under S23-3.
        let mut spelling = String::from("!");
        while let Some(ch) = self.peek() {
            if ch == '!' || ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '%' {
                spelling.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        if spelling == "!" {
            return match self.peek() {
                None | Some(' ') | Some('\t') | Some('\n') | Some(',') | Some(']') | Some('}')
                | Some(':') | Some('#') => Ok(None),
                Some(_) => Err(unread()),
            };
        }
        tag_spelling_kind(&spelling, &self.handles)
    }

    /// Read the name of the anchor or alias spelling the
    /// reading stands on, consuming its mark and its name.
    /// The name is the run of letters, digits, hyphens,
    /// and underscores after the mark, under the same
    /// standings read_anchor_name states for a line.
    fn flow_anchor_name(&mut self) -> Result<String, String> {
        self.bump();
        let mut name = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                name.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        if name.is_empty() {
            return Err(
                "an anchor or alias stands with no name, which the subset does not carry"
                    .to_string(),
            );
        }
        if let Some(ch) = self.peek() {
            let can_end = matches!(
                ch,
                ' ' | '\t'
                    | '\n'
                    | '\u{0085}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | ','
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | ':'
                    | '#'
                    | '&'
                    | '*'
                    | '!'
                    | '|'
                    | '>'
                    | '\''
                    | '"'
                    | '%'
                    | '@'
                    | '`'
                    | '?'
            );
            if !can_end {
                return Err(
                    "an anchor or alias name carries a character the subset does not read"
                        .to_string(),
                );
            }
        }
        Ok(name)
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
            Some('\'') | Some('"') => {
                let text = self.flow_quoted()?;
                match self.pending_tag.take() {
                    Some(tag) => resolve_tagged_scalar(tag, &text),
                    None => Ok(Value::Text(text)),
                }
            }
            Some('&') | Some('!') => {
                let mut anchor: Option<String> = None;
                let mut tag: Option<Tag> = None;
                loop {
                    match self.peek() {
                        Some('&') => {
                            if anchor.is_some() {
                                return Err("two anchors stand on one value, which the subset does not carry".to_string());
                            }
                            anchor = Some(self.flow_anchor_name()?);
                        }
                        Some('!') => {
                            if let Some(found) = self.flow_tag()? {
                                if tag.is_some() {
                                    return Err("two tags stand on one value, which the subset does not carry".to_string());
                                }
                                tag = Some(found);
                            }
                        }
                        _ => break,
                    }
                    self.skip_space();
                }
                match self.peek() {
                    Some('*') => Err(if anchor.is_some() {
                        "an anchor stands on an alias, which the subset does not carry".to_string()
                    } else {
                        "a tag stands on an alias, which the subset does not carry".to_string()
                    }),
                    Some(',') | Some('}') | Some(']') | Some(':') | None => {
                        let value = match tag {
                            Some(tag) => resolve_tagged_scalar(tag, "")?,
                            None => Value::Null,
                        };
                        if let Some(name) = &anchor {
                            self.anchors.begin(name)?;
                            self.anchors.complete(name, &value);
                        }
                        Ok(value)
                    }
                    Some('[') | Some('{') => {
                        if let Some(name) = &anchor {
                            self.anchors.begin(name)?;
                        }
                        let value = self.node(depth)?;
                        let value = match tag {
                            Some(tag) => tag_kind_check(tag, value)?,
                            None => value,
                        };
                        if let Some(name) = &anchor {
                            self.anchors.complete(name, &value);
                        }
                        Ok(value)
                    }
                    _ => {
                        if let Some(name) = &anchor {
                            self.anchors.begin(name)?;
                        }
                        self.pending_tag = tag;
                        let value = self.node(depth)?;
                        let value = match self.pending_tag.take() {
                            Some(tag) => tag_kind_check(tag, value)?,
                            None => value,
                        };
                        if let Some(name) = &anchor {
                            self.anchors.complete(name, &value);
                        }
                        Ok(value)
                    }
                }
            }
            Some('*') => {
                let name = self.flow_anchor_name()?;
                if self.pending_tag.is_some() {
                    return Err(
                        "a tag stands on an alias, which the subset does not carry".to_string()
                    );
                }
                self.anchors.resolve(&name)
            }
            Some('|') | Some('>') => {
                Err("a block scalar stands in flow, which the subset does not carry".to_string())
            }
            Some('@') | Some('`') | Some('%') => Err(
                "a scalar starts with a reserved character, which the subset does not carry"
                    .to_string(),
            ),
            Some(_) => {
                let text = self.flow_plain()?;
                if text == "?" {
                    return Err(
                        "an explicit key indicator stands without a key, which the subset does not carry"
                            .to_string(),
                    );
                }
                if text == ":" {
                    return Err(
                        "a mapping value stands without a key, which the subset does not carry"
                            .to_string(),
                    );
                }
                match self.pending_tag.take() {
                    Some(tag) => resolve_tagged_scalar(tag, &text),
                    None => resolve_plain(&text),
                }
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
            // The explicit key indicator of slice 22 opens
            // a pair entry here as it opens one in a flow
            // mapping, standing against the key with or
            // without a space after it.
            let lead_offset = self
                .chars
                .iter()
                .skip(self.pos)
                .position(|ch| !matches!(ch, ' ' | '\t' | '\n'));
            let explicit = match lead_offset {
                Some(offset) if self.chars.get(self.pos + offset) == Some(&'?') => {
                    self.pos += offset;
                    self.bump();
                    self.skip_space();
                    true
                }
                _ => false,
            };
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
            if (self.peek() == Some(':') && !matches!(item, Value::List(_) | Value::Map(_)))
                || explicit
            {
                // A single pair mapping standing in a sequence.
                let value = if self.peek() == Some(':') {
                    self.bump();
                    self.skip_space();
                    match self.peek() {
                        Some(',') | Some(']') => Value::Null,
                        _ => self.node(depth + 1)?,
                    }
                } else {
                    Value::Null
                };
                if item == Value::Text("<<".to_string())
                    && !matches!(item_lead, Some('\'') | Some('"'))
                {
                    let assembled = assemble_mapping(merge_groups_of(value)?, Vec::new());
                    items.push(assembled);
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
                    continue;
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

    /// Read one flow mapping key. Returns the key and
    /// whether it stood as a plain spelling, which the
    /// merge key reading turns on. Anchors and aliases in
    /// key position read under slice 20.
    fn flow_key(&mut self) -> Result<(Value, bool), String> {
        match self.peek() {
            Some('\'') | Some('"') => {
                let start = self.pos;
                let key = self.flow_quoted()?;
                if self.chars[start..self.pos].contains(&'\n') {
                    return Err(
                        "a quoted key runs over more than one line, which the subset does not carry"
                            .to_string(),
                    );
                }
                let key = match self.pending_tag.take() {
                    Some(tag) => resolve_tagged_scalar(tag, &key)?,
                    None => Value::Text(key),
                };
                Ok((key, false))
            }
            Some('[') | Some('{') => Err(
                "a flow collection stands as a key, which the subset does not carry".to_string(),
            ),
            Some('&') | Some('!') => {
                let mut anchor: Option<String> = None;
                let mut tag: Option<Tag> = None;
                loop {
                    match self.peek() {
                        Some('&') => {
                            if anchor.is_some() {
                                return Err("two anchors stand on one value, which the subset does not carry".to_string());
                            }
                            anchor = Some(self.flow_anchor_name()?);
                        }
                        Some('!') => {
                            if let Some(found) = self.flow_tag()? {
                                if tag.is_some() {
                                    return Err("two tags stand on one value, which the subset does not carry".to_string());
                                }
                                tag = Some(found);
                            }
                        }
                        _ => break,
                    }
                    self.skip_space();
                }
                if self.peek() == Some('*') {
                    return Err(if anchor.is_some() {
                        "an anchor stands on an alias, which the subset does not carry".to_string()
                    } else {
                        "a tag stands on an alias, which the subset does not carry".to_string()
                    });
                }
                self.pending_tag = tag;
                let (key, plain) = self.flow_key()?;
                if self.pending_tag.take().is_some() {
                    return Err(
                        "a tag stands on a value of another kind, which the subset does not carry"
                            .to_string(),
                    );
                }
                if let Some(name) = &anchor {
                    self.anchors.begin(name)?;
                    self.anchors.complete(name, &key);
                }
                Ok((key, plain))
            }
            Some('*') => {
                let name = self.flow_anchor_name()?;
                if self.pending_tag.is_some() {
                    return Err(
                        "a tag stands on an alias, which the subset does not carry".to_string()
                    );
                }
                let value = self.anchors.resolve(&name)?;
                if matches!(value, Value::List(_) | Value::Map(_)) {
                    return Err(
                        "an alias names a collection standing as a key, which the subset does not carry"
                            .to_string(),
                    );
                }
                Ok((value, false))
            }
            Some('@') | Some('`') | Some('%') => Err(
                "a scalar starts with a reserved character, which the subset does not carry"
                    .to_string(),
            ),
            Some(_) => {
                let text = self.flow_plain()?;
                let key = match self.pending_tag.take() {
                    Some(tag) => resolve_tagged_scalar(tag, &text)?,
                    None => resolve_plain(&text)?,
                };
                Ok((key, true))
            }
            None => Err("a flow collection is not closed".to_string()),
        }
    }

    fn flow_mapping(&mut self, depth: usize) -> Result<Value, String> {
        self.bump();
        let mut entries: Vec<(Value, Value)> = Vec::new();
        let mut merge_groups: Vec<Vec<(Value, Value)>> = Vec::new();
        self.skip_space();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(Value::Map(entries));
        }
        loop {
            self.skip_space();
            if self.peek() == Some('?') {
                // The explicit key indicator of slice 22.
                // In flow it stands against the key it
                // opens, with or without a space after it.
                self.bump();
                self.skip_space();
            }
            let (key, plain_spelling) = self.flow_key()?;
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
            if plain_spelling && key == Value::Text("<<".to_string()) {
                merge_groups.extend(merge_groups_of(value)?);
            } else {
                yaml_map_insert(&mut entries, key, value);
            }
            self.skip_space();
            match self.peek() {
                Some(',') => {
                    self.bump();
                    self.skip_space();
                    if self.peek() == Some('}') {
                        self.bump();
                        return Ok(assemble_mapping(merge_groups, entries));
                    }
                }
                Some('}') => {
                    self.bump();
                    return Ok(assemble_mapping(merge_groups, entries));
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
        // A dash followed by a blank, a tab, a line break,
        // or the end of the text is a block sequence entry
        // to PyYAML's scanner wherever it stands, and its
        // flow parser refuses the document. The check
        // stands at the head, where every caller opens a
        // fresh plain scalar, so the item, value, and key
        // positions refuse alike. A dash followed by any
        // other character opens a plain scalar.
        if self.peek() == Some('-') {
            let next = self.chars.get(self.pos + 1).copied();
            if matches!(
                next,
                None | Some(' ')
                    | Some('\t')
                    | Some('\n')
                    | Some('\u{0085}')
                    | Some('\u{2028}')
                    | Some('\u{2029}')
            ) {
                return Err(
                    "a block sequence entry stands in a flow collection, which the subset does not carry"
                        .to_string(),
                );
            }
        }
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
        let mut source = FlowSource::build(
            &self.lines,
            first,
            lead,
            &mut self.anchors,
            self.handles.clone(),
        );
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
            "k: *alias\n",
            "base: {x: 1}\nk:\n  <<: base\n",
            "k: {j: |}\n",
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
        for text in ["*a: v\n", "{*a: 1}\n", "{<<: base}\n", "[<<: base]\n"] {
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

    #[test]
    fn a_plain_value_ending_in_a_colon_refuses() {
        // Fix pass 3, the first CodeRabbit finding on
        // PR 22. strip_comment trims the head's trailing
        // blanks before fold_plain's first segment check,
        // so the check's colon and blank arm could never
        // fire, and a plain value ending in a colon read
        // as text where PyYAML raises for a mapping value
        // standing inside the scalar. The continuation
        // segments already carried the rule in its bare
        // form, a segment ending in a colon refuses.
        for text in [
            "k: a:\n",
            "k: a: \n",
            "k: a: # a comment\n",
            "k: a::\n",
            "k: a:b:\n",
            "k: a:\n  cont\n",
            "outer:\n  k: a:\n",
            "- k: a:\n",
        ] {
            assert!(parse_yaml(text).is_err(), "document {text:?}");
        }
        // A colon away from the value's end reads, a
        // quoted spelling of the same text reads, and a
        // colon closing a pair on a sequence line or in a
        // flow sequence still reads as the pair it is,
        // because those colons never reach fold_plain.
        assert_eq!(scalar("a:b"), Value::Text("a:b".to_string()));
        assert_eq!(scalar("'a:'"), Value::Text("a:".to_string()));
        let block_pair = parse_yaml("- a:\n").expect("the document reads");
        let items = block_pair.as_list().expect("a sequence stands");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].get("a"), Some(&Value::Null), "document - a:");
        let flow_pair = parse_yaml("[a:]\n").expect("the document reads");
        let items = flow_pair.as_list().expect("a sequence stands");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].get("a"), Some(&Value::Null), "document [a:]");
    }

    #[test]
    fn a_block_entry_indicator_inside_flow_refuses() {
        // Fix pass 3, the second CodeRabbit finding on
        // PR 22. PyYAML's scanner hands back a block entry
        // token for a dash followed by a blank or a break
        // wherever it stands, and its flow parser refuses
        // the document, while flow_plain read the dash and
        // what followed it as one plain scalar. The check
        // stands at flow_plain's head, so the item, value,
        // and key positions refuse alike, as the probes
        // showed the key position diverging the same way.
        for text in [
            "[- a]\n",
            "{k: - a}\n",
            "[- a, b]\n",
            "k: [- a]\n",
            "[-\ta]\n",
            "[- ]\n",
            "{k: - }\n",
            "{k: -\n  a}\n",
            "[-\n  a]\n",
            "[-\u{2028}a]\n",
            "[-\u{0085}a]\n",
            "{- a: 1}\n",
            "[[ - a]]\n",
            "{k: [- a]}\n",
        ] {
            assert!(parse_yaml(text).is_err(), "document {text:?}");
        }
        // A dash followed by any other character opens a
        // plain scalar, as it did before this fix.
        let lone = parse_yaml("[-]\n").expect("the document reads");
        assert_eq!(
            lone.as_list().expect("a sequence stands"),
            &vec![Value::Text("-".to_string())]
        );
        let number = parse_yaml("[-5]\n").expect("the document reads");
        assert_eq!(
            number.as_list().expect("a sequence stands"),
            &vec![Value::Int(IntVal::from_decimal("-5"))]
        );
        let word = parse_yaml("[-a]\n").expect("the document reads");
        assert_eq!(
            word.as_list().expect("a sequence stands"),
            &vec![Value::Text("-a".to_string())]
        );
        let keyed = parse_yaml("{-: 1}\n").expect("the document reads");
        assert_eq!(keyed.get("-"), Some(&Value::Int(IntVal::from_decimal("1"))));
    }
}
