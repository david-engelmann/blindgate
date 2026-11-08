//! The file reading entries and the live capture.
//!
//! Slice 1 ported the parse of a trace text and slice 2 the
//! state machinery the parse runs on. This module carries the
//! last Python only surface of the trace reading, the two
//! entries that read a trace from a file and the one entry
//! that captures a trace live. The entries are orchestration
//! over the ported parse. The file entries read the whole
//! file as bytes, decode it with the replacement decoding the
//! contract states, and parse with the shape check on. The
//! capture shells to strace with the argument list the Python
//! passes, reads the trace back through the same decoding,
//! and parses with the shape check off, on the ground that
//! strace itself produced the text.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::errors::BlindgateError;
use crate::model::TraceRead;
use crate::parse::{parse_trace, parse_trace_with_switch};

/// The calls the capture asks strace to trace, the counterpart
/// of the Python _TRACED_CALLS constant, spelled as one
/// argument.
const TRACED_CALLS: &str = "trace=open,openat,chdir,fchdir,close,dup,dup2,dup3,fcntl,clone,fork,vfork,clone3,execve,execveat";

/// Return the read set and outside reads from a trace log file.
///
/// The file is read whole as bytes and decoded under the
/// replacement decoding, then parsed with the shape check on,
/// exactly as the Python composes its read_text with
/// parse_trace. A file that cannot be read refuses with the
/// wording PORT3-3 states.
pub fn read_trace_from_log_file(path: &Path, root: &Path) -> Result<TraceRead, BlindgateError> {
    let normalized = normalize_path(path);
    let bytes = fs::read(&normalized).map_err(|err| cannot_read_wording(&normalized, &err))?;
    let text = decode_replacement(&bytes);
    parse_trace(&text, root)
}

/// Normalize a path the way the Python Path conversion does.
///
/// The Python entry converts its argument with Path first, and
/// the read and the wording both use the converted spelling.
/// The conversion here works over the path components without
/// resolving anything against the filesystem. A leading dot
/// component is dropped, duplicate separators collapse, dot
/// components drop, parent components stand as spelled because
/// Path does not resolve them, a trailing slash drops, and the
/// empty path reads as a single dot.
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

/// Return the reads half of a trace log file read.
///
/// The counterpart of the Python read_set_from_log_file, one
/// half of read_trace_from_log_file kept under its own name.
pub fn read_set_from_log_file(path: &Path, root: &Path) -> Result<Vec<String>, BlindgateError> {
    Ok(read_trace_from_log_file(path, root)?.reads)
}

/// Run a command under strace and return its reads and outside reads.
///
/// The pair matches parse_trace, so a live capture names its
/// outside reads exactly the way a handed over trace file
/// does. Two refusals precede any work, in the Python's
/// order. When no strace stands on the path the entry refuses
/// that no read set can be captured, and when the command is
/// empty it refuses that no command was given. The traced
/// command's own exit status is ignored, the trace temp file
/// stands inside the resolved root while the capture runs and
/// is removed in all paths, and the traced command inherits
/// the caller's standard streams.
pub fn run_traced(command: &[String], root: &Path) -> Result<TraceRead, BlindgateError> {
    let strace = which_strace().ok_or_else(|| {
        BlindgateError::new(
            "strace is not available on this machine, so no read set can be captured",
        )
    })?;
    if command.is_empty() {
        return Err(BlindgateError::new("no command was given to run"));
    }
    let root_path = resolve_root(root);
    let trace_file = create_trace_file(&root_path)?;
    let result = run_capture(&strace, &trace_file, command, &root_path);
    let _ = fs::remove_file(&trace_file);
    result
}

fn run_capture(
    strace: &Path,
    trace_file: &Path,
    command: &[String],
    root_path: &Path,
) -> Result<TraceRead, BlindgateError> {
    let status = Command::new(strace)
        .arg("-f")
        .arg("-e")
        .arg(TRACED_CALLS)
        .arg("-o")
        .arg(trace_file)
        .arg("--")
        .args(command)
        .status();
    if let Err(err) = status {
        return Err(BlindgateError::new(err.to_string()));
    }
    let bytes = fs::read(trace_file).map_err(|err| BlindgateError::new(err.to_string()))?;
    let text = decode_replacement(&bytes);
    parse_trace_with_switch(&text, root_path, false)
}

/// Resolve a root the way the Python's Path.resolve does.
///
/// A root that stands on disk resolves through the filesystem,
/// symlinks and all. One that does not stand keeps the lexical
/// absolute form, so the capture's temp file creation meets
/// the missing directory itself and surfaces its refusal, as
/// the Python's unwrapped error does.
fn resolve_root(root: &Path) -> PathBuf {
    if let Ok(resolved) = fs::canonicalize(root) {
        return resolved;
    }
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(root)
    };
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

static TRACE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Create the capture's temp file inside the resolved root.
///
/// The name carries the prefix the Python's mkstemp carries,
/// and its uniqueness comes from the process number, a
/// counter, and the nanosecond clock, retried against
/// collision. The file is created with the owner only
/// permissions mkstemp gives. A creation failure surfaces
/// the io error's own words, the stated asymmetry with the
/// file entries' wrapped wording.
fn create_trace_file(root_path: &Path) -> Result<PathBuf, BlindgateError> {
    use std::os::unix::fs::OpenOptionsExt;
    let pid = std::process::id();
    let mut last_err: Option<io::Error> = None;
    for _ in 0..128 {
        let n = TRACE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let candidate = root_path.join(format!(".blindgate-trace-{pid}-{n}-{nanos}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(_file) => return Ok(candidate),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                last_err = Some(err);
            }
            Err(err) => return Err(BlindgateError::new(err.to_string())),
        }
    }
    Err(BlindgateError::new(
        last_err
            .map(|err| err.to_string())
            .unwrap_or_else(|| "the trace temp file could not be created".to_string()),
    ))
}

/// Find strace the way the Python's shutil.which finds it.
///
/// An executable candidate named strace on a PATH directory,
/// with an empty PATH element reading as the current directory
/// and a missing PATH reading as the platform default. A PATH
/// value that is set but empty performs no lookup at all, as
/// shutil.which answers None for it. A candidate that is not
/// executable and a directory named strace both miss, as they
/// miss for the Python.
fn which_strace() -> Option<PathBuf> {
    let path_var = match env::var_os("PATH") {
        Some(value) if value.is_empty() => return None,
        Some(value) => value,
        None => "/bin:/usr/bin".into(),
    };
    for dir in env::split_paths(&path_var) {
        let candidate = if dir.as_os_str().is_empty() {
            PathBuf::from("strace")
        } else {
            dir.join("strace")
        };
        if is_executable_candidate(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Judge a candidate the way os.access judges execute access.
///
/// A candidate is accepted when it is not a directory and its
/// execute bits allow the process to execute it, without
/// requiring a regular file, so a FIFO with execute bits is
/// accepted exactly as the Python accepts it. The governing
/// bit class is chosen from the file owner and group against
/// the process effective user and group and its supplementary
/// groups, and a process running as root is allowed by any
/// execute bit standing. Two approximations remain and are
/// stated here. Access control lists are not visible in the
/// mode bits, so a candidate whose access rests on one is
/// judged by its bits alone. When the process status cannot
/// be read, the check falls back to any standing execute bit.
fn is_executable_candidate(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(_) => return false,
    };
    if meta.is_dir() {
        return false;
    }
    let mode = meta.permissions().mode();
    match effective_ids() {
        Some((euid, egid, groups)) => {
            if euid == 0 {
                mode & 0o111 != 0
            } else if meta.uid() == euid {
                mode & 0o100 != 0
            } else if meta.gid() == egid || groups.contains(&meta.gid()) {
                mode & 0o010 != 0
            } else {
                mode & 0o001 != 0
            }
        }
        None => mode & 0o111 != 0,
    }
}

/// Read the process effective ids and supplementary groups.
///
/// The values come from the process status, where the
/// effective user and group stand second on their lines.
/// A status that cannot be read or parsed answers None and
/// the caller falls back as its own comment states.
fn effective_ids() -> Option<(u32, u32, Vec<u32>)> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let mut uid = None;
    let mut gid = None;
    let mut groups = Vec::new();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            uid = rest
                .split_whitespace()
                .nth(1)
                .and_then(|value| value.parse().ok());
        } else if let Some(rest) = line.strip_prefix("Gid:") {
            gid = rest
                .split_whitespace()
                .nth(1)
                .and_then(|value| value.parse().ok());
        } else if let Some(rest) = line.strip_prefix("Groups:") {
            groups = rest
                .split_whitespace()
                .filter_map(|value| value.parse().ok())
                .collect();
        }
    }
    Some((uid?, gid?, groups))
}

/// The wording PORT3-3 states for a file that cannot be read.
///
/// The outer form is the Python's, and the parenthesised part
/// is CPython's own spelling of the operating system error,
/// the bracketed number, the error's words, and the filename
/// under the repr rules the helper below states. The outer
/// wording carries the normalized path as spelled, raw,
/// through the lossy rendering the Python gets from
/// formatting a Path into its message. The reading itself
/// never passes the path through that rendering.
fn cannot_read_wording(path: &Path, err: &io::Error) -> BlindgateError {
    let spelled = path.to_string_lossy().into_owned();
    let inner = match err.raw_os_error() {
        Some(errno) => format!(
            "[Errno {errno}] {}: {}",
            errno_words(errno),
            cpython_repr(&spelled)
        ),
        None => err.to_string(),
    };
    BlindgateError::new(format!("cannot read trace log {spelled} ({inner})"))
}

/// Spell a name the way CPython repr spells a string.
///
/// The rules stated here are exactly the ones the unit tests
/// pin. Single quotes wrap the name by default, and double
/// quotes wrap it when the name holds a single quote and no
/// double quote. The chosen quote and backslashes stand
/// escaped with a backslash. A newline, a carriage return,
/// and a tab stand as their escape spellings. Any other
/// control character stands as a hexadecimal escape with
/// two digits. Every other character, including non ASCII
/// printable characters, stands as itself.
fn cpython_repr(name: &str) -> String {
    let quote = if name.contains('\'') && !name.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(name.len() + 2);
    out.push(quote);
    for ch in name.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            ch if ch.is_control() => out.push_str(&format!("\\x{:02x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// The words CPython's spelling carries for an error number.
///
/// The three numbers the port's own reading can produce on
/// this platform are stated in PORT3-3. Any other number
/// takes the platform's own words for it, read back out of
/// the io error's spelling with its suffix removed.
fn errno_words(errno: i32) -> String {
    match errno {
        2 => "No such file or directory".to_string(),
        13 => "Permission denied".to_string(),
        21 => "Is a directory".to_string(),
        _ => {
            let display = io::Error::from_raw_os_error(errno).to_string();
            let suffix = format!(" (os error {errno})");
            match display.strip_suffix(&suffix) {
                Some(words) => words.to_string(),
                None => display,
            }
        }
    }
}

/// Decode bytes as UTF-8 with undecodable material replaced.
///
/// This is CPython's errors replace reading, written for
/// PORT3-2 as amended, and its grouping is the contract's
/// own statement. A well formed sequence decodes to its
/// scalar. A starter together with the continuation bytes
/// that validly follow it, fewer than the sequence needs,
/// yields one U+FFFD for that whole prefix, whether the text
/// ended or a byte that is not a continuation broke it, and
/// decoding resumes at the breaking byte. A continuation byte
/// with no starter before it yields one U+FFFD per byte, and
/// a starter in the ranges 0xF5 through 0xFF likewise. A
/// sequence whose second byte falls outside the range its
/// starter allows yields one U+FFFD for the starter alone,
/// and decoding resumes at the second byte, which is then
/// read on its own terms.
pub(crate) fn decode_replacement(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let starter = bytes[at];
        if starter < 0x80 {
            out.push(starter as char);
            at += 1;
            continue;
        }
        if !(0xC2..=0xF4).contains(&starter) {
            out.push('\u{FFFD}');
            at += 1;
            continue;
        }
        let (needed, lowest, highest) = match starter {
            0xC2..=0xDF => (1, 0x80, 0xBF),
            0xE0 => (2, 0xA0, 0xBF),
            0xE1..=0xEC => (2, 0x80, 0xBF),
            0xED => (2, 0x80, 0x9F),
            0xEE..=0xEF => (2, 0x80, 0xBF),
            0xF0 => (3, 0x90, 0xBF),
            0xF1..=0xF3 => (3, 0x80, 0xBF),
            _ => (3, 0x80, 0x8F),
        };
        if at + 1 < bytes.len() && !(lowest..=highest).contains(&bytes[at + 1]) {
            out.push('\u{FFFD}');
            at += 1;
            continue;
        }
        let mut have = 0;
        while have < needed && at + 1 + have < bytes.len() {
            let next = bytes[at + 1 + have];
            let valid = if have == 0 {
                (lowest..=highest).contains(&next)
            } else {
                (0x80..=0xBF).contains(&next)
            };
            if !valid {
                break;
            }
            have += 1;
        }
        if have == needed {
            let text = std::str::from_utf8(&bytes[at..=at + needed])
                .expect("a sequence the range checks validated");
            out.push_str(text);
            at += needed + 1;
        } else {
            out.push('\u{FFFD}');
            at += 1 + have;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(bytes: &[u8]) -> String {
        decode_replacement(bytes)
    }

    #[test]
    fn rule_a_well_formed_sequences_decode_to_their_scalars() {
        assert_eq!(decoded(b"plain ascii"), "plain ascii");
        assert_eq!(decoded("café".as_bytes()), "café");
        assert_eq!(decoded("a\u{1F600}b".as_bytes()), "a\u{1F600}b");
        assert_eq!(decoded(&[]), "");
    }

    #[test]
    fn rule_a_overlong_starters_never_start_a_sequence() {
        assert_eq!(decoded(&[0xC0, 0xAF]), "\u{FFFD}\u{FFFD}");
        assert_eq!(decoded(&[0xC1, 0xBF]), "\u{FFFD}\u{FFFD}");
    }

    #[test]
    fn rule_b_a_sequence_truncated_by_the_end_yields_one_replacement() {
        assert_eq!(decoded(&[0xC3]), "\u{FFFD}");
        assert_eq!(decoded(&[0xE2, 0x82]), "\u{FFFD}");
        assert_eq!(decoded(&[0xF0, 0x9F, 0x82]), "\u{FFFD}");
        assert_eq!(decoded(b"caf\xC3"), "caf\u{FFFD}");
    }

    #[test]
    fn rule_c_a_starter_broken_at_once_yields_one_replacement() {
        assert_eq!(decoded(&[0xC3, b'"']), "\u{FFFD}\"");
        assert_eq!(decoded(&[0xE2, b'y']), "\u{FFFD}y");
    }

    #[test]
    fn rule_c_as_amended_a_valid_prefix_broken_late_yields_one_replacement() {
        // Probe record B4: a three byte starter with one valid
        // continuation, broken by a plain byte.
        assert_eq!(decoded(&[0xE2, 0x82, b'y']), "\u{FFFD}y");
        // Probe record B10: a four byte starter with one valid
        // continuation, broken by a quote.
        assert_eq!(decoded(&[0xF0, 0x9F, b'"']), "\u{FFFD}\"");
        // Two valid continuations of a four byte sequence,
        // broken by a plain byte.
        assert_eq!(decoded(&[0xF0, 0x9F, 0x82, b'x']), "\u{FFFD}x");
    }

    #[test]
    fn rule_d_lone_continuations_yield_one_replacement_per_byte() {
        assert_eq!(decoded(&[0x80]), "\u{FFFD}");
        assert_eq!(decoded(&[0x80, 0x80]), "\u{FFFD}\u{FFFD}");
        assert_eq!(decoded(b"a\x80\x80\x80b"), "a\u{FFFD}\u{FFFD}\u{FFFD}b");
    }

    #[test]
    fn rule_e_a_second_byte_outside_the_allowed_range_resumes_at_it() {
        // Probe record B12: the surrogate range after 0xED.
        assert_eq!(decoded(&[0xED, 0xA0, 0x80]), "\u{FFFD}\u{FFFD}\u{FFFD}");
        // Probe record B13: the range past U+10FFFF after 0xF4.
        assert_eq!(
            decoded(&[0xF4, 0x90, 0x80, 0x80]),
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}"
        );
        // The overlong range after 0xE0: the second byte is a
        // continuation, but below the floor its starter allows,
        // so it is resumed at and read as a lone continuation.
        assert_eq!(decoded(&[0xE0, 0x80]), "\u{FFFD}\u{FFFD}");
        // The overlong range after 0xF0, the same shape.
        assert_eq!(decoded(&[0xF0, 0x80]), "\u{FFFD}\u{FFFD}");
    }

    #[test]
    fn rule_f_starters_past_the_range_yield_one_replacement_per_byte() {
        assert_eq!(decoded(&[0xFF]), "\u{FFFD}");
        assert_eq!(decoded(&[0xFF, 0xFE]), "\u{FFFD}\u{FFFD}");
        assert_eq!(decoded(&[0xF5]), "\u{FFFD}");
        assert_eq!(decoded(&[0xF8, 0x88]), "\u{FFFD}\u{FFFD}");
    }

    #[test]
    fn the_probe_record_lines_decode_as_recorded() {
        // B1: a lone continuation inside the spelled path.
        assert_eq!(
            decoded(b"101 openat(AT_FDCWD, \"a\x80b.txt\", O_RDONLY) = 3\n"),
            "101 openat(AT_FDCWD, \"a\u{FFFD}b.txt\", O_RDONLY) = 3\n"
        );
        // B5: the overlong form.
        assert_eq!(
            decoded(b"101 openat(AT_FDCWD, \"a\xC0\xAFb.txt\", O_RDONLY) = 3\n"),
            "101 openat(AT_FDCWD, \"a\u{FFFD}\u{FFFD}b.txt\", O_RDONLY) = 3\n"
        );
        // B9: a valid sequence followed by a lone continuation.
        assert_eq!(
            decoded(b"101 openat(AT_FDCWD, \"caf\xC3\xA9\x80.txt\", O_RDONLY) = 3\n"),
            "101 openat(AT_FDCWD, \"café\u{FFFD}.txt\", O_RDONLY) = 3\n"
        );
        // B11: a truncated sequence at the end of the file.
        assert_eq!(
            decoded(b"101 openat(AT_FDCWD, \"x\xE2\x82"),
            "101 openat(AT_FDCWD, \"x\u{FFFD}"
        );
    }

    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn the_strace_lookup_mirrors_shutil_which_on_the_probed_shapes() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = PATH_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let original = env::var_os("PATH");
        let pid = std::process::id();
        let base = std::env::temp_dir().join(format!("blindgate-which-{pid}"));
        let fifo_dir = base.join("fifo");
        let dir_dir = base.join("dircase");
        let plain_dir = base.join("plain");
        let exec_dir = base.join("exec");
        for dir in [&fifo_dir, &dir_dir, &plain_dir, &exec_dir] {
            fs::create_dir_all(dir).expect("stand the lookup directory");
        }
        let fifo = fifo_dir.join("strace");
        let status = Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo for the FIFO candidate");
        assert!(status.success(), "mkfifo stands the FIFO candidate");
        fs::set_permissions(&fifo, fs::Permissions::from_mode(0o755))
            .expect("give the FIFO candidate execute bits");
        fs::create_dir_all(dir_dir.join("strace")).expect("stand the directory candidate");
        let plain = plain_dir.join("strace");
        fs::write(&plain, "not runnable\n").expect("write the plain candidate");
        fs::set_permissions(&plain, fs::Permissions::from_mode(0o644))
            .expect("keep the plain candidate non executable");
        let exec_file = exec_dir.join("strace");
        fs::write(&exec_file, "#!/bin/sh\n").expect("write the executable candidate");
        fs::set_permissions(&exec_file, fs::Permissions::from_mode(0o755))
            .expect("give the executable candidate execute bits");

        // A PATH set but empty performs no lookup at all, and
        // the entry therefore refuses that strace is missing.
        env::set_var("PATH", "");
        assert_eq!(which_strace(), None, "an empty PATH performs no lookup");
        let err = run_traced(&[], Path::new("/")).expect_err("the entry refuses");
        assert_eq!(
            err.message(),
            "strace is not available on this machine, so no read set can be captured"
        );
        // A FIFO with execute bits is accepted, so the lookup
        // passes and the empty command refusal is what answers.
        env::set_var("PATH", &fifo_dir);
        assert_eq!(which_strace(), Some(fifo.clone()));
        let err = run_traced(&[], Path::new("/")).expect_err("the entry refuses");
        assert_eq!(err.message(), "no command was given to run");
        // The standing shapes miss, and a regular executable hits.
        env::set_var("PATH", &dir_dir);
        assert_eq!(which_strace(), None, "a directory named strace misses");
        env::set_var("PATH", &plain_dir);
        assert_eq!(which_strace(), None, "a non executable file misses");
        env::set_var("PATH", &exec_dir);
        assert_eq!(which_strace(), Some(exec_file.clone()));

        match original {
            Some(value) => env::set_var("PATH", value),
            None => env::remove_var("PATH"),
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn run_traced_refuses_an_empty_command() {
        let _guard = PATH_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if which_strace().is_none() {
            return;
        }
        let err = run_traced(&[], Path::new("/"))
            .expect_err("an empty command refuses once strace stands on the path");
        assert_eq!(err.message(), "no command was given to run");
    }

    #[test]
    fn run_traced_fails_the_capture_at_a_root_that_does_not_stand() {
        let _guard = PATH_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if which_strace().is_none() {
            return;
        }
        let root =
            std::env::temp_dir().join(format!("blindgate-entries-missing-{}", std::process::id()));
        let command = vec!["/bin/true".to_string()];
        let err = run_traced(&command, &root)
            .expect_err("a root that does not stand fails the capture at the temp file");
        assert!(
            err.message().contains("No such file or directory"),
            "the failure carries the io refusal's own words for the temp file site, got {}",
            err.message()
        );
        assert!(
            !err.message().contains("cannot read trace log"),
            "the failure does not carry the PORT3-3 outer wording, got {}",
            err.message()
        );
    }

    #[test]
    fn the_path_spelling_normalizes_the_way_path_converts_it() {
        let cases = [
            ("./missing.log", "missing.log"),
            ("sub//missing.log", "sub/missing.log"),
            ("sub/./missing.log", "sub/missing.log"),
            ("./project", "project"),
            ("project/", "project"),
            ("", "."),
            ("plain.txt/", "plain.txt"),
            (".", "."),
            ("./", "."),
            ("a/../b", "a/../b"),
            ("../x", "../x"),
            ("/a//b/./c", "/a/b/c"),
        ];
        for (spelled, want) in cases {
            assert_eq!(
                normalize_path(Path::new(spelled)),
                PathBuf::from(want),
                "spelling {spelled}"
            );
        }
    }

    #[test]
    fn the_file_entries_read_and_name_the_normalized_path() {
        let pid = std::process::id();
        let root = Path::new("/");
        // The three missing spellings name the one normalized path.
        for spelled in [
            format!("./blindgate-missing-{pid}.log"),
            format!("sub//blindgate-missing-{pid}.log"),
            format!("sub/./blindgate-missing-{pid}.log"),
        ] {
            let named = normalize_path(Path::new(&spelled));
            let named = named.to_string_lossy().into_owned();
            let err = read_trace_from_log_file(Path::new(&spelled), root)
                .expect_err("a missing file refuses");
            assert_eq!(
                err.message(),
                format!(
                    "cannot read trace log {named} ([Errno 2] No such file or directory: '{named}')"
                ),
                "spelling {spelled}"
            );
        }
        // The empty path reads as a single dot and refuses there.
        let err =
            read_trace_from_log_file(Path::new(""), root).expect_err("the empty path refuses");
        assert_eq!(
            err.message(),
            "cannot read trace log . ([Errno 21] Is a directory: '.')"
        );
        // A directory under a dot spelling and a trailing slash.
        let base = std::env::temp_dir().join(format!("blindgate-entries-path-{pid}"));
        let project = base.join("project");
        fs::create_dir_all(&project).expect("stand the project directory");
        for spelled in [
            format!("{}/./project", base.display()),
            format!("{}/", project.display()),
        ] {
            let err = read_trace_from_log_file(Path::new(&spelled), root)
                .expect_err("a directory refuses");
            assert_eq!(
                err.message(),
                format!(
                    "cannot read trace log {} ([Errno 21] Is a directory: '{}')",
                    project.display(),
                    project.display()
                ),
                "spelling {spelled}"
            );
        }
        // A regular file spelled with a trailing slash reads
        // through the normalized spelling to the shape refusal.
        let plain = base.join("plain.txt");
        fs::write(&plain, "not a trace\n").expect("write the plain file");
        let spelled = format!("{}/", plain.display());
        let err = read_trace_from_log_file(Path::new(&spelled), &base)
            .expect_err("an unshaped file refuses on its shape");
        assert_eq!(err.message(), "trace log does not look like strace output");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_inner_wording_spells_the_name_the_way_cpython_repr_does() {
        let err = io::Error::from_raw_os_error(2);
        let cases = [
            (
                "a'b.log",
                "cannot read trace log a'b.log ([Errno 2] No such file or directory: \"a'b.log\")",
            ),
            (
                "a\\b.log",
                "cannot read trace log a\\b.log ([Errno 2] No such file or directory: 'a\\\\b.log')",
            ),
            (
                "a\nb.log",
                "cannot read trace log a\nb.log ([Errno 2] No such file or directory: 'a\\nb.log')",
            ),
            (
                "a'b\"c.log",
                "cannot read trace log a'b\"c.log ([Errno 2] No such file or directory: 'a\\'b\"c.log')",
            ),
        ];
        for (spelled, want) in cases {
            assert_eq!(
                cannot_read_wording(Path::new(spelled), &err).message(),
                want,
                "spelling {spelled}"
            );
        }
        assert_eq!(cpython_repr("a\tb.log"), "'a\\tb.log'");
        assert_eq!(cpython_repr("a\rb.log"), "'a\\rb.log'");
        assert_eq!(cpython_repr("a\u{1}b.log"), "'a\\x01b.log'");
        assert_eq!(cpython_repr("Ünïcode.log"), "'Ünïcode.log'");
        assert_eq!(cpython_repr("plain.log"), "'plain.log'");
    }

    #[test]
    fn the_file_error_wording_carries_the_stated_spellings() {
        let path = Path::new("some.log");
        let cases = [
            (2, "No such file or directory"),
            (13, "Permission denied"),
            (21, "Is a directory"),
        ];
        for (errno, words) in cases {
            let err = io::Error::from_raw_os_error(errno);
            assert_eq!(
                cannot_read_wording(path, &err).message(),
                format!("cannot read trace log some.log ([Errno {errno}] {words}: 'some.log')")
            );
        }
    }
}
