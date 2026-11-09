//! The value model the judgement stack reads documents into.
//!
//! PORT4-1 states the model. A value is a null, a bool, an
//! exact integer of any size, a float, a text, a timestamp
//! carrying the CPython str form of the date or datetime it
//! came from, a list, or a map. A map keeps insertion order,
//! its keys are scalars, key equality follows the Python's
//! dict, where a bool, an integer, and a float form one
//! equality family by numeric value, and on a collision the
//! first key object is retained and the last value wins.
//!
//! This module also carries the JSON reading both config
//! forms pass through, CPython's float spelling, which the
//! report rendering and the workflow key coercions both
//! need, and the thread the deep readings run on.

use std::cmp::Ordering;

/// An exact integer, held as a sign and canonical decimal digits.
///
/// The digits carry no leading zeros and zero carries no
/// sign, so two spellings of one number hold the same value.
/// The shape follows the digit discipline of the trace
/// parse's number type, widened with the small multiply and
/// add the radix and sexagesimal readings need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntVal {
    negative: bool,
    digits: String,
}

impl IntVal {
    pub fn zero() -> IntVal {
        IntVal {
            negative: false,
            digits: "0".to_string(),
        }
    }

    pub fn is_zero(&self) -> bool {
        self.digits == "0"
    }

    pub fn is_negative(&self) -> bool {
        self.negative
    }

    pub fn digits(&self) -> &str {
        &self.digits
    }

    /// Read a decimal spelling with an optional sign.
    ///
    /// The caller has already judged the spelling a decimal
    /// integer, so what stands here is a sign and digits.
    pub fn from_decimal(text: &str) -> IntVal {
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        IntVal::from_parts(negative, digits)
    }

    /// Read unsigned digits in a radix by multiply and add.
    pub fn from_radix(text: &str, radix: u32) -> IntVal {
        let mut value = IntVal::zero();
        for ch in text.chars() {
            let digit = ch.to_digit(radix).unwrap_or(0);
            value.mul_small(radix);
            value.add_small(digit);
        }
        value
    }

    /// Apply a sign, clearing it when the value is zero.
    pub fn with_sign(mut self, negative: bool) -> IntVal {
        self.negative = negative && !self.is_zero();
        self
    }

    fn from_parts(negative: bool, digits: &str) -> IntVal {
        let trimmed = digits.trim_start_matches('0');
        if trimmed.is_empty() {
            return IntVal::zero();
        }
        IntVal {
            negative,
            digits: trimmed.to_string(),
        }
    }

    /// Multiply the magnitude by a small factor in place.
    pub fn mul_small(&mut self, factor: u32) {
        if factor == 0 || self.is_zero() {
            *self = IntVal::zero();
            return;
        }
        let mut carry: u32 = 0;
        let mut bytes: Vec<u8> = self.digits.bytes().collect();
        for byte in bytes.iter_mut().rev() {
            let current = (*byte - b'0') as u32 * factor + carry;
            *byte = b'0' + (current % 10) as u8;
            carry = current / 10;
        }
        let mut head = String::new();
        while carry > 0 {
            head.push((b'0' + (carry % 10) as u8) as char);
            carry /= 10;
        }
        let head: String = head.chars().rev().collect();
        self.digits = format!("{head}{}", String::from_utf8_lossy(&bytes));
    }

    /// Add a small amount to the magnitude in place.
    pub fn add_small(&mut self, amount: u32) {
        let mut carry = amount;
        let mut bytes: Vec<u8> = self.digits.bytes().collect();
        let mut at = bytes.len();
        while carry > 0 {
            if at == 0 {
                bytes.insert(0, b'0');
                at = 1;
            }
            at -= 1;
            let current = (bytes[at] - b'0') as u32 + carry;
            bytes[at] = b'0' + (current % 10) as u8;
            carry = current / 10;
        }
        self.digits = String::from_utf8_lossy(&bytes).into_owned();
    }

    /// Add another magnitude to this one in place.
    ///
    /// Both values are read as unsigned here, the
    /// sexagesimal fold works on magnitudes only.
    pub fn add_magnitude(&mut self, other: &IntVal) {
        let mine: Vec<u8> = self.digits.bytes().collect();
        let theirs: Vec<u8> = other.digits.bytes().collect();
        let mut out: Vec<u8> = Vec::new();
        let mut carry: u32 = 0;
        let mut i = mine.len();
        let mut j = theirs.len();
        while i > 0 || j > 0 || carry > 0 {
            let mut current = carry;
            if i > 0 {
                i -= 1;
                current += (mine[i] - b'0') as u32;
            }
            if j > 0 {
                j -= 1;
                current += (theirs[j] - b'0') as u32;
            }
            out.push(b'0' + (current % 10) as u8);
            carry = current / 10;
        }
        out.reverse();
        let text = String::from_utf8_lossy(&out).into_owned();
        *self = IntVal::from_parts(self.negative, &text);
    }
}

impl Ord for IntVal {
    fn cmp(&self, other: &IntVal) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self
                .digits
                .len()
                .cmp(&other.digits.len())
                .then_with(|| self.digits.cmp(&other.digits)),
            (true, true) => other
                .digits
                .len()
                .cmp(&self.digits.len())
                .then_with(|| other.digits.cmp(&self.digits)),
        }
    }
}

impl PartialOrd for IntVal {
    fn partial_cmp(&self, other: &IntVal) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for IntVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.negative {
            f.write_str("-")?;
        }
        f.write_str(&self.digits)
    }
}

/// One value in the model PORT4-1 states.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(IntVal),
    Float(f64),
    Text(String),
    /// A date or datetime, carried as its CPython str form.
    ///
    /// The variant has no other behaviour, so the carried
    /// text is the whole of it and two timestamps are equal
    /// when their carried texts are equal.
    Timestamp(String),
    List(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

impl Value {
    /// The value a text key holds in a map, when it stands there.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find_map(|(held, value)| match held {
                Value::Text(text) if text == key => Some(value),
                _ => None,
            }),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&Vec<(Value, Value)>> {
        match self {
            Value::Map(entries) => Some(entries),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&Vec<Value>> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// Insert into a map under the Python's dict key rules.
///
/// On a collision the first key object is retained and the
/// last value wins, exactly as a dict built key by key in
/// CPython keeps the first key and the last value.
pub fn map_insert(map: &mut Vec<(Value, Value)>, key: Value, value: Value) {
    for (held, existing) in map.iter_mut() {
        if keys_equal(held, &key) {
            *existing = value;
            return;
        }
    }
    map.push((key, value));
}

/// Whether two scalar keys collide in a Python dict.
///
/// A bool, an integer, and a float form one equality family
/// by numeric value. The integer and float comparison is
/// exact, so an integer past the float's precision never
/// equals the float it rounds to, and a float with a
/// fractional part never equals an integer. A NaN equals
/// nothing, not even another NaN. A timestamp equals only a
/// timestamp carrying the same text, and a text equals only
/// the same text.
pub fn keys_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Bool(a), Value::Int(i)) | (Value::Int(i), Value::Bool(a)) => {
            if *a {
                !i.is_negative() && i.digits() == "1"
            } else {
                i.is_zero()
            }
        }
        (Value::Bool(a), Value::Float(f)) | (Value::Float(f), Value::Bool(a)) => {
            *f == if *a { 1.0 } else { 0.0 }
        }
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Int(i), Value::Float(f)) | (Value::Float(f), Value::Int(i)) => {
            float_int_equal(*f, i)
        }
        (Value::Text(a), Value::Text(b)) => a == b,
        (Value::Timestamp(a), Value::Timestamp(b)) => a == b,
        _ => false,
    }
}

/// Compare a float and an exact integer for numeric equality.
///
/// The float is decomposed into a mantissa and a power of
/// two and rebuilt as exact decimal digits, so the answer
/// is the one CPython's exact comparison gives.
fn float_int_equal(value: f64, integer: &IntVal) -> bool {
    if value.is_nan() || value.is_infinite() {
        return false;
    }
    if value == 0.0 {
        return integer.is_zero();
    }
    let bits = value.to_bits();
    let negative = (bits >> 63) != 0;
    if integer.is_negative() != negative {
        return false;
    }
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mantissa, exponent) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1u64 << 52), biased - 1075)
    };
    if mantissa == 0 {
        return integer.is_zero();
    }
    if exponent >= 0 {
        let mut rebuilt = IntVal::from_decimal(&mantissa.to_string());
        for _ in 0..exponent {
            rebuilt.mul_small(2);
        }
        rebuilt.digits() == integer.digits()
    } else {
        let shift = (-exponent) as u32;
        if shift >= 64 {
            return false;
        }
        if mantissa & ((1u64 << shift) - 1) != 0 {
            return false;
        }
        let whole = mantissa >> shift;
        whole.to_string() == integer.digits()
    }
}

/// Spell a float the way CPython's repr spells it.
///
/// The digits are the shortest that round trip, taken from
/// the platform's own shortest spelling, and the rendering
/// follows CPython's rule. Fixed notation stands when the
/// decimal point would fall after more than minus four and
/// at most sixteen digits, with a trailing point zero for
/// an integral value, and scientific notation stands
/// outside that band with a signed exponent of at least two
/// digits.
pub fn float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent_text) = scientific
        .split_once('e')
        .expect("the scientific spelling always carries an exponent");
    let exponent: i32 = exponent_text
        .parse()
        .expect("the scientific exponent is a small integer");
    let digits: String = mantissa.chars().filter(|ch| *ch != '.').collect();
    let point = exponent + 1;
    let mut out = String::new();
    if value < 0.0 {
        out.push('-');
    }
    if (-4) < point && point <= 16 {
        if point <= 0 {
            out.push_str("0.");
            for _ in 0..(-point) {
                out.push('0');
            }
            out.push_str(&digits);
        } else if (point as usize) >= digits.len() {
            out.push_str(&digits);
            for _ in 0..(point as usize - digits.len()) {
                out.push('0');
            }
            out.push_str(".0");
        } else {
            out.push_str(&digits[..point as usize]);
            out.push('.');
            out.push_str(&digits[point as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if exponent < 0 { '-' } else { '+' });
        let magnitude = exponent.abs();
        if magnitude < 10 {
            out.push('0');
        }
        out.push_str(&magnitude.to_string());
    }
    out
}

/// Spell a float the way CPython's json spelling spells it.
///
/// The finite spellings are the repr spellings, and the
/// three non finite values carry the JSON names CPython
/// writes for them. The report renderer does not route
/// document floats through this spelling today, because
/// its documents carry no floats. The spelling stands as
/// the value model's JSON form under PORT4-1, beside the
/// repr form the workflow key coercions use.
pub fn float_json(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value < 0.0 {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else {
        float_repr(value)
    }
}

/// The stack the document readings run on.
///
/// A deeply nested document recurses as deep as it nests,
/// and the contract bounds nesting at four thousand for the
/// YAML subset. Both readings run on a spawned thread with
/// a 256 MiB stack, the discipline the trace parse's
/// deepest readings already follow.
pub(crate) fn run_on_parse_stack<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    let spawned = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(work);
    match spawned {
        Ok(handle) => match handle.join() {
            Ok(result) => result,
            Err(_) => Err("the reading failed on its own thread".to_string()),
        },
        Err(_) => Err("the reading could not be started".to_string()),
    }
}

/// The deepest JSON nesting the reading follows.
///
/// CPython's own reader dies somewhere past eight thousand
/// levels with a recursion failure the config reader never
/// catches, so a document nested past this bound is a crash
/// family in the Python and the port refuses it in words of
/// its own rather than follow it down.
const JSON_DEPTH_BOUND: usize = 10_000;

/// Read a JSON text into the value model.
///
/// The reading is CPython's json reading as far as the
/// model carries it. Object keys keep the first position
/// and the last value on a duplicate, integers are exact at
/// any size, numbers with a fraction or an exponent are
/// floats, and the three non finite constants CPython
/// accepts stand as floats. The error text is the port's
/// own, naming the failure and its position, because
/// PORT4-3 states that the decoder's texts are not
/// reproduced.
pub fn parse_json(text: &str) -> Result<Value, String> {
    let owned = text.to_string();
    run_on_parse_stack(move || {
        let mut parser = JsonParser {
            text: &owned,
            bytes: owned.as_bytes(),
            pos: 0,
        };
        parser.skip_space();
        let value = parser.parse_value(0)?;
        parser.skip_space();
        if parser.pos != parser.bytes.len() {
            return Err(parser.failure("characters stand after the document"));
        }
        Ok(value)
    })
}

struct JsonParser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn failure(&self, reason: &str) -> String {
        let upto = self.pos.min(self.bytes.len());
        let mut line = 1usize;
        let mut line_start = 0usize;
        for (index, byte) in self.bytes.iter().enumerate().take(upto) {
            if *byte == b'\n' {
                line += 1;
                line_start = index + 1;
            }
        }
        let column = self.text[line_start..upto].chars().count() + 1;
        format!("{reason} at line {line} column {column}")
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while let Some(byte) = self.peek() {
            match byte {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > JSON_DEPTH_BOUND {
            return Err(self.failure("the document nests deeper than the reading allows"));
        }
        match self.peek() {
            None => Err(self.failure("expected a value")),
            Some(b'{') => self.parse_object(depth),
            Some(b'[') => self.parse_array(depth),
            Some(b'"') => Ok(Value::Text(self.parse_string()?)),
            Some(b't') => self.parse_word("true", Value::Bool(true)),
            Some(b'f') => self.parse_word("false", Value::Bool(false)),
            Some(b'n') => self.parse_word("null", Value::Null),
            Some(b'N') => self.parse_word("NaN", Value::Float(f64::NAN)),
            Some(b'I') => self.parse_word("Infinity", Value::Float(f64::INFINITY)),
            Some(b'-') => {
                if self.bytes[self.pos..].starts_with(b"-Infinity") {
                    self.pos += "-Infinity".len();
                    Ok(Value::Float(f64::NEG_INFINITY))
                } else {
                    self.parse_number()
                }
            }
            Some(byte) if byte.is_ascii_digit() => self.parse_number(),
            Some(_) => Err(self.failure("expected a value")),
        }
    }

    fn parse_word(&mut self, word: &str, value: Value) -> Result<Value, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.failure("a literal is misspelled"))
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<Value, String> {
        self.pos += 1;
        let mut entries: Vec<(Value, Value)> = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Map(entries));
        }
        loop {
            self.skip_space();
            if self.peek() != Some(b'"') {
                return Err(self.failure("expected an object key string"));
            }
            let key = self.parse_string()?;
            self.skip_space();
            if self.peek() != Some(b':') {
                return Err(self.failure("expected a colon after the object key"));
            }
            self.pos += 1;
            self.skip_space();
            let value = self.parse_value(depth + 1)?;
            map_insert(&mut entries, Value::Text(key), value);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Map(entries));
                }
                _ => return Err(self.failure("expected a comma or the closing brace")),
            }
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<Value, String> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::List(items));
        }
        loop {
            let value = self.parse_value(depth + 1)?;
            items.push(value);
            self.skip_space();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::List(items));
                }
                _ => return Err(self.failure("expected a comma or the closing bracket")),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let byte = match self.peek() {
                Some(byte) => byte,
                None => return Err(self.failure("the string is not terminated")),
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    self.parse_escape(&mut out)?;
                }
                0x00..=0x1f => {
                    return Err(self.failure("a control character stands in the string"));
                }
                _ => {
                    let width = utf8_width(byte);
                    let end = (self.pos + width).min(self.bytes.len());
                    out.push_str(&self.text[self.pos..end]);
                    self.pos = end;
                }
            }
        }
    }

    fn parse_escape(&mut self, out: &mut String) -> Result<(), String> {
        let byte = match self.peek() {
            Some(byte) => byte,
            None => return Err(self.failure("the string is not terminated")),
        };
        self.pos += 1;
        match byte {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{0008}'),
            b'f' => out.push('\u{000C}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let first = self.parse_hex4()?;
                if (0xD800..0xDC00).contains(&first) {
                    if self.bytes[self.pos..].starts_with(b"\\u") {
                        self.pos += 2;
                        let second = self.parse_hex4()?;
                        if (0xDC00..0xE000).contains(&second) {
                            let combined = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                            match char::from_u32(combined) {
                                Some(ch) => out.push(ch),
                                None => {
                                    return Err(self.failure("a string escape names no character"))
                                }
                            }
                        } else {
                            return Err(self.failure("a lone surrogate stands in a string escape"));
                        }
                    } else {
                        return Err(self.failure("a lone surrogate stands in a string escape"));
                    }
                } else if (0xDC00..0xE000).contains(&first) {
                    return Err(self.failure("a lone surrogate stands in a string escape"));
                } else {
                    match char::from_u32(first) {
                        Some(ch) => out.push(ch),
                        None => return Err(self.failure("a string escape names no character")),
                    }
                }
            }
            _ => return Err(self.failure("an escape in the string is not known")),
        }
        Ok(())
    }

    fn parse_hex4(&mut self) -> Result<u32, String> {
        let mut value: u32 = 0;
        for _ in 0..4 {
            let byte = match self.peek() {
                Some(byte) => byte,
                None => return Err(self.failure("the string is not terminated")),
            };
            let digit = match (byte as char).to_digit(16) {
                Some(digit) => digit,
                None => return Err(self.failure("a string escape is not hexadecimal")),
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(byte) if (b'1'..=b'9').contains(&byte) => {
                while matches!(self.peek(), Some(next) if next.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            _ => return Err(self.failure("the number is not shaped as JSON shapes numbers")),
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            if !matches!(self.peek(), Some(next) if next.is_ascii_digit()) {
                return Err(self.failure("the number is not shaped as JSON shapes numbers"));
            }
            while matches!(self.peek(), Some(next) if next.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(next) if next.is_ascii_digit()) {
                return Err(self.failure("the number is not shaped as JSON shapes numbers"));
            }
            while matches!(self.peek(), Some(next) if next.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let literal = &self.text[start..self.pos];
        if is_float {
            match literal.parse::<f64>() {
                Ok(value) => Ok(Value::Float(value)),
                Err(_) => Err(self.failure("the number is not shaped as JSON shapes numbers")),
            }
        } else {
            Ok(Value::Int(IntVal::from_decimal(literal)))
        }
    }
}

fn utf8_width(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_spellings_match_cpython() {
        let cases: &[(f64, &str)] = &[
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (0.1, "0.1"),
            (100.0, "100.0"),
            (-0.0, "-0.0"),
            (0.0, "0.0"),
            (90.5, "90.5"),
            (1e20, "1e+20"),
            (1.5, "1.5"),
            (0.0012, "0.0012"),
            (3723.25, "3723.25"),
            (123456789.12345679, "123456789.12345679"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (1500.0, "1500.0"),
            (-12.0, "-12.0"),
        ];
        for (value, expected) in cases {
            assert_eq!(&float_repr(*value), expected, "float {value}");
        }
        assert_eq!(float_repr(f64::NAN), "nan");
        assert_eq!(float_repr(f64::INFINITY), "inf");
        assert_eq!(float_repr(f64::NEG_INFINITY), "-inf");
        assert_eq!(float_json(f64::NAN), "NaN");
        assert_eq!(float_json(f64::INFINITY), "Infinity");
        assert_eq!(float_json(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn dict_key_collisions_follow_the_numeric_family() {
        let one_int = Value::Int(IntVal::from_decimal("1"));
        let one_float = Value::Float(1.0);
        let truth = Value::Bool(true);
        assert!(keys_equal(&one_int, &truth));
        assert!(keys_equal(&one_float, &truth));
        assert!(keys_equal(&one_int, &one_float));
        assert!(keys_equal(
            &Value::Int(IntVal::from_decimal("0")),
            &Value::Float(-0.0)
        ));
        assert!(keys_equal(
            &Value::Int(IntVal::from_decimal("9007199254740992")),
            &Value::Float(9007199254740992.0)
        ));
        assert!(!keys_equal(
            &Value::Int(IntVal::from_decimal("9007199254740993")),
            &Value::Float(9007199254740992.0)
        ));
        assert!(!keys_equal(
            &Value::Int(IntVal::from_decimal("1")),
            &Value::Float(1.5)
        ));
        assert!(!keys_equal(
            &Value::Float(f64::NAN),
            &Value::Float(f64::NAN)
        ));
        assert!(!keys_equal(
            &Value::Float(f64::NAN),
            &Value::Int(IntVal::from_decimal("1"))
        ));
        assert!(!keys_equal(
            &Value::Text("2024-01-01".to_string()),
            &Value::Timestamp("2024-01-01".to_string())
        ));
        // Insertion keeps the first key object and the last value.
        let mut map = Vec::new();
        map_insert(
            &mut map,
            Value::Int(IntVal::from_decimal("1")),
            Value::Text("a".to_string()),
        );
        map_insert(&mut map, Value::Bool(true), Value::Text("b".to_string()));
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].0, Value::Int(IntVal::from_decimal("1")));
        assert_eq!(map[0].1, Value::Text("b".to_string()));
    }

    #[test]
    fn json_reading_keeps_exact_integers_and_last_duplicate_values() {
        let value = parse_json("{\"b\": 123456789012345678901234567890, \"a\": 1e3, \"b\": 2}")
            .expect("the document reads");
        let entries = value.as_map().expect("an object stands");
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            (
                Value::Text("b".to_string()),
                Value::Int(IntVal::from_decimal("2"))
            )
        );
        assert_eq!(
            entries[1],
            (Value::Text("a".to_string()), Value::Float(1000.0))
        );
        let big = parse_json("[123456789012345678901234567890]").expect("the document reads");
        match &big.as_list().expect("a list stands")[0] {
            Value::Int(integer) => {
                assert_eq!(integer.to_string(), "123456789012345678901234567890")
            }
            other => panic!("expected an exact integer, found {other:?}"),
        }
    }

    #[test]
    fn json_reading_refuses_malformed_documents_in_its_own_words() {
        for text in [
            "{\"a\": }",
            "[1,]",
            "\"unterminated",
            "{\"a\" 1}",
            "tru",
            "[01]",
        ] {
            assert!(parse_json(text).is_err(), "document {text}");
        }
        let error = parse_json("{\"a\": }").expect_err("the document refuses");
        assert!(error.contains("line 1 column 7"), "error {error}");
    }

    #[test]
    fn json_reading_refuses_a_byte_order_mark() {
        // The mark strip belongs to the YAML reader alone.
        // CPython's json reading refuses a marked text, and
        // so does this one, before and after that fix.
        assert!(parse_json("\u{FEFF}{\"a\": 1}").is_err());
    }
}
