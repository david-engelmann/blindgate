//! The analysis PORT4-5 states.
//!
//! A check is judged by pairing it with a read set. The
//! matched paths are the read set's paths that fall in the
//! claimed surface under the surface rule, in read set
//! order, and the verdict follows from the read set and
//! the match. The surface rule first tries the glob
//! translation CPython's fnmatch performs, and where the
//! pattern holds no glob character it falls back to the
//! directory prefix rule.

use std::collections::HashSet;

use crate::errors::BlindgateError;
use crate::model::{
    Check, CheckResult, TraceSource, VERDICT_EMPTY, VERDICT_PROVEN, VERDICT_UNPROVEN,
};

/// The source kind for a check judged on its own trace file.
pub const SOURCE_OWN_TRACE: &str = "own_trace";

/// The source kind for checks judged on a shared trace file.
pub const SOURCE_SHARED_TRACE: &str = "shared_trace";

/// The source kind for checks judged on a shared run.
pub const SOURCE_SHARED_RUN: &str = "shared_run";

/// One check's judgement, a read set, a source, and outside reads.
///
/// The counterpart of one triple in the Python's
/// analyse_judged. The parts constructor carries the
/// Python's guard, that a judgement is a triple of read
/// set, source, and outside reads, so the refusal the
/// Python raises as a ValueError stands where a caller
/// can reach it.
#[derive(Debug, Clone, PartialEq)]
pub struct Judgement {
    pub read_set: Vec<String>,
    pub source: Option<TraceSource>,
    pub outside_reads: Vec<String>,
}

/// One part of a judgement under construction.
#[derive(Debug, Clone, PartialEq)]
pub enum JudgementPart {
    ReadSet(Vec<String>),
    Source(Option<TraceSource>),
    OutsideReads(Vec<String>),
}

impl Judgement {
    pub fn new(
        read_set: Vec<String>,
        source: Option<TraceSource>,
        outside_reads: Vec<String>,
    ) -> Judgement {
        Judgement {
            read_set,
            source,
            outside_reads,
        }
    }

    /// Build a judgement from its parts in order.
    ///
    /// Anything but a read set, a source, and outside
    /// reads, in that order, refuses with the Python's
    /// own words for the malformed triple.
    pub fn from_parts(parts: Vec<JudgementPart>) -> Result<Judgement, BlindgateError> {
        let refusal = || {
            BlindgateError::new("each judgement is a triple of read set, source, and outside reads")
        };
        if parts.len() != 3 {
            return Err(refusal());
        }
        let mut parts = parts.into_iter();
        let read_set = match parts.next() {
            Some(JudgementPart::ReadSet(read_set)) => read_set,
            _ => return Err(refusal()),
        };
        let source = match parts.next() {
            Some(JudgementPart::Source(source)) => source,
            _ => return Err(refusal()),
        };
        let outside_reads = match parts.next() {
            Some(JudgementPart::OutsideReads(outside_reads)) => outside_reads,
            _ => return Err(refusal()),
        };
        Ok(Judgement {
            read_set,
            source,
            outside_reads,
        })
    }
}

/// Whether a read path falls in a claimed surface entry.
///
/// A pattern holding a glob character is tried as
/// CPython's fnmatch case sensitive translation reads it,
/// where a star crosses separators, a question mark takes
/// exactly one character, and a bracket class takes one
/// character from its set, a leading exclamation negating
/// and a caret standing as a member, with an unclosed
/// bracket read as a literal bracket. Matching is whole
/// string. A pattern with no glob character is a
/// directory prefix, its trailing slashes stripped, and
/// a path equal to it or standing under it with a slash
/// matches. The empty pattern matches nothing a read set
/// can hold.
pub fn path_matches_surface(path: &str, pattern: &str) -> bool {
    if pattern.contains(['*', '?', '[']) {
        glob_match(path, pattern)
    } else {
        let prefix = pattern.trim_end_matches('/');
        path == prefix || path.starts_with(&format!("{prefix}/"))
    }
}

/// The verdict for a read set and its matched paths.
pub fn verdict_for(read_set: &[String], matched: &[String]) -> &'static str {
    if read_set.is_empty() {
        VERDICT_EMPTY
    } else if matched.is_empty() {
        VERDICT_UNPROVEN
    } else {
        VERDICT_PROVEN
    }
}

/// Judge every check against one shared read set.
pub fn analyse(checks: &[Check], read_set: &[String]) -> Vec<CheckResult> {
    checks
        .iter()
        .map(|check| judge(check, &Judgement::new(read_set.to_vec(), None, Vec::new())))
        .collect()
}

/// Judge every check against one shared read set and outside reads.
pub fn analyse_with_outside(
    checks: &[Check],
    read_set: &[String],
    outside_reads: &[String],
) -> Vec<CheckResult> {
    checks
        .iter()
        .map(|check| {
            judge(
                check,
                &Judgement::new(read_set.to_vec(), None, outside_reads.to_vec()),
            )
        })
        .collect()
}

/// Judge every check against its own judgement.
///
/// The judgements must run parallel to the checks. The
/// Python raises that as a ValueError, a crash family,
/// and the port returns the same words as an error rather
/// than produce a result the Python cannot produce.
pub fn analyse_judged(
    checks: &[Check],
    judgements: &[Judgement],
) -> Result<Vec<CheckResult>, BlindgateError> {
    if judgements.len() != checks.len() {
        return Err(BlindgateError::new(
            "judgements must run parallel to checks",
        ));
    }
    Ok(checks
        .iter()
        .zip(judgements.iter())
        .map(|(check, judgement)| judge(check, judgement))
        .collect())
}

fn judge(check: &Check, judgement: &Judgement) -> CheckResult {
    let matched: Vec<String> = judgement
        .read_set
        .iter()
        .filter(|path| {
            check
                .surface
                .iter()
                .any(|pattern| path_matches_surface(path, pattern))
        })
        .cloned()
        .collect();
    CheckResult {
        name: check.name.clone(),
        job: check.job.clone(),
        surface: check.surface.clone(),
        read_set: judgement.read_set.clone(),
        matched: matched.clone(),
        verdict: verdict_for(&judgement.read_set, &matched).to_string(),
        trace_source: judgement.source.clone(),
        outside_reads: judgement.outside_reads.clone(),
    }
}

/// One glob token, as the fnmatch translation produces it.
#[derive(Debug, Clone, PartialEq)]
enum GlobToken {
    Star,
    AnyChar,
    Class { negated: bool, members: ClassSet },
    Literal(char),
}

#[derive(Debug, Clone, PartialEq)]
struct ClassSet {
    singles: HashSet<char>,
    ranges: Vec<(char, char)>,
}

impl ClassSet {
    fn holds(&self, ch: char) -> bool {
        if self.singles.contains(&ch) {
            return true;
        }
        self.ranges
            .iter()
            .any(|(low, high)| *low <= ch && ch <= *high)
    }
}

/// Translate a glob pattern to its tokens.
///
/// The translation is CPython fnmatch's. A class opens at
/// a bracket, a leading exclamation negates it, a bracket
/// that follows the opening or the negation stands as a
/// member, and a class that never closes leaves the
/// bracket a literal. Inside a class a dash between two
/// members forms a range and otherwise stands as a member.
/// Backslashes are ordinary characters.
fn glob_tokens(pattern: &str) -> Vec<GlobToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '*' => {
                tokens.push(GlobToken::Star);
                index += 1;
            }
            '?' => {
                tokens.push(GlobToken::AnyChar);
                index += 1;
            }
            '[' => match parse_class(&chars, index) {
                Some((token, next)) => {
                    tokens.push(token);
                    index = next;
                }
                None => {
                    tokens.push(GlobToken::Literal('['));
                    index += 1;
                }
            },
            _ => {
                tokens.push(GlobToken::Literal(ch));
                index += 1;
            }
        }
    }
    tokens
}

/// Parse a bracket class at an opening bracket.
///
/// Returns the class token and the index just past the
/// closing bracket, or nothing when no closing bracket
/// stands, which leaves the bracket literal.
fn parse_class(chars: &[char], open: usize) -> Option<(GlobToken, usize)> {
    let mut index = open + 1;
    let mut negated = false;
    if chars.get(index) == Some(&'!') {
        negated = true;
        index += 1;
    }
    let member_start = index;
    if chars.get(index) == Some(&']') {
        index += 1;
    }
    while index < chars.len() && chars[index] != ']' {
        index += 1;
    }
    if index >= chars.len() {
        return None;
    }
    let members = &chars[member_start..index];
    let mut set = ClassSet {
        singles: HashSet::new(),
        ranges: Vec::new(),
    };
    let mut at = 0usize;
    while at < members.len() {
        if at + 2 < members.len() + 1
            && at + 1 < members.len()
            && members[at + 1] == '-'
            && at + 2 < members.len()
        {
            set.ranges.push((members[at], members[at + 2]));
            at += 3;
        } else {
            set.singles.insert(members[at]);
            at += 1;
        }
    }
    Some((
        GlobToken::Class {
            negated,
            members: set,
        },
        index + 1,
    ))
}

/// Whole string glob matching over the translated tokens.
fn glob_match(path: &str, pattern: &str) -> bool {
    let tokens = glob_tokens(pattern);
    let chars: Vec<char> = path.chars().collect();
    glob_match_at(&tokens, 0, &chars, 0)
}

fn glob_match_at(tokens: &[GlobToken], at: usize, chars: &[char], from: usize) -> bool {
    if at == tokens.len() {
        return from == chars.len();
    }
    match &tokens[at] {
        GlobToken::Star => {
            // A run of stars is one star, as the translation
            // collapses nothing but the semantics agree.
            let mut next = at + 1;
            while matches!(tokens.get(next), Some(GlobToken::Star)) {
                next += 1;
            }
            for consumed in from..=chars.len() {
                if glob_match_at(tokens, next, chars, consumed) {
                    return true;
                }
            }
            false
        }
        GlobToken::AnyChar => from < chars.len() && glob_match_at(tokens, at + 1, chars, from + 1),
        GlobToken::Class { negated, members } => {
            if from >= chars.len() {
                return false;
            }
            let holds = members.holds(chars[from]);
            (holds != *negated) && glob_match_at(tokens, at + 1, chars, from + 1)
        }
        GlobToken::Literal(expected) => {
            from < chars.len()
                && chars[from] == *expected
                && glob_match_at(tokens, at + 1, chars, from + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnmatch_pairs_match_the_probe_record() {
        let pairs: &[(&str, &str, bool)] = &[
            ("src/app.py", "src/**", true),
            ("src/app.py", "src/*", true),
            ("src/app.py", "SRC/*", false),
            ("a/b/c", "a/*/c", true),
            ("a/b/c/d", "a/*/d", true),
            ("file.txt", "*.txt", true),
            ("file.txt", "file?.txt", false),
            ("file1.txt", "file?.txt", true),
            ("ab", "a[b]", true),
            ("a[b", "a[b", true),
            ("ax", "a[!b]", true),
            ("ab", "a[!b]", false),
            ("ax", "a[^b]", false),
            ("ab", "a[^b]", true),
            ("a^", "a[^b]", true),
            ("-x", "[-]x", true),
            ("docs/guide.md", "docs/*.md", true),
            ("docs/a/b.md", "docs/*.md", true),
            ("]", "[]]", true),
            ("a", "[!]", false),
            ("-", "[a-]", true),
            ("a", "[a-]", true),
            ("b", "[a-c]", true),
            ("A", "[a-z]", false),
        ];
        for (path, pattern, expected) in pairs {
            assert_eq!(
                &path_matches_surface(path, pattern),
                expected,
                "path {path} pattern {pattern}"
            );
        }
    }

    #[test]
    fn prefix_rule_edges() {
        assert!(path_matches_surface("docs", "docs"));
        assert!(path_matches_surface("docs/guide.md", "docs"));
        assert!(path_matches_surface("docs/guide.md", "docs/"));
        assert!(path_matches_surface("docs/guide.md", "docs//"));
        assert!(!path_matches_surface("docsx/guide.md", "docs"));
        assert!(!path_matches_surface("docs", "docs/guide.md"));
        assert!(!path_matches_surface("anything", ""));
        assert!(!path_matches_surface("src/app.py", "docs"));
    }

    #[test]
    fn verdicts_and_guards_carry_the_pythons_words() {
        assert_eq!(verdict_for(&[], &[]), VERDICT_EMPTY);
        assert_eq!(verdict_for(&["a".to_string()], &[]), VERDICT_UNPROVEN);
        assert_eq!(
            verdict_for(&["a".to_string()], &["a".to_string()]),
            VERDICT_PROVEN
        );
        let checks = vec![Check {
            name: "one".to_string(),
            job: "job".to_string(),
            surface: vec!["src/**".to_string()],
            trace: None,
            trace_path: None,
        }];
        let error = analyse_judged(&checks, &[]).expect_err("the guard fires");
        assert_eq!(error.message(), "judgements must run parallel to checks");
        let error = Judgement::from_parts(vec![JudgementPart::ReadSet(Vec::new())])
            .expect_err("the guard fires");
        assert_eq!(
            error.message(),
            "each judgement is a triple of read set, source, and outside reads"
        );
    }
}
