use std::collections::HashMap;
use std::fs;

/// Settle the dot segments of an absolute path lexically.
pub fn normpath(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            part => stack.push(part),
        }
    }
    format!("/{}", stack.join("/"))
}

pub fn posix_join(first: &str, second: &str) -> String {
    if second.starts_with('/') {
        return second.to_string();
    }
    if first.is_empty() || first.ends_with('/') {
        format!("{first}{second}")
    } else {
        format!("{first}/{second}")
    }
}

fn posix_split(path: &str) -> (String, String) {
    let at = path.rfind('/').map(|index| index + 1).unwrap_or(0);
    let (head, tail) = (&path[..at], &path[at..]);
    let head = if !head.is_empty() && !head.bytes().all(|byte| byte == b'/') {
        head.trim_end_matches('/')
    } else {
        head
    };
    (head.to_string(), tail.to_string())
}

/// Resolve a path the way Python os.path.realpath resolves it.
///
/// The walk settles one component at a time against the
/// filesystem, the way the Python walk settles them. A
/// component that is a symlink is followed by resolving its
/// target against the directory the link stands in, and the
/// walk remembers every link path it has resolved, so a chain
/// of any length resolves to its target and a link met a second
/// time answers from the memory. A link met again while its own
/// resolution is still open is a loop, and there the walk rests
/// the way the Python rests, returning the link as it was
/// spelled with the rest of the path joined unchanged behind
/// it, so a loop names the spelling it was entered by.
pub fn realpath(path: &str) -> String {
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    let (resolved, _) = join_real_path("", path, &mut seen);
    normpath(&resolved)
}

fn join_real_path(
    path: &str,
    rest: &str,
    seen: &mut HashMap<String, Option<String>>,
) -> (String, bool) {
    let mut path = path.to_string();
    let mut rest = rest;
    if let Some(stripped) = rest.strip_prefix('/') {
        rest = stripped;
        path = "/".to_string();
    }
    while !rest.is_empty() {
        let (name, remainder) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index + 1..]),
            None => (rest, ""),
        };
        rest = remainder;
        if name.is_empty() || name == "." {
            continue;
        }
        if name == ".." {
            if path.is_empty() {
                path = "..".to_string();
            } else {
                let (head, tail) = posix_split(&path);
                path = if tail == ".." {
                    posix_join(&posix_join(&head, ".."), "..")
                } else {
                    head
                };
            }
            continue;
        }
        let newpath = posix_join(&path, name);
        let is_link = fs::symlink_metadata(&newpath)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false);
        if !is_link {
            path = newpath;
            continue;
        }
        match seen.get(&newpath) {
            Some(Some(resolved)) => {
                path = resolved.clone();
                continue;
            }
            Some(None) => return (posix_join(&newpath, rest), false),
            None => {}
        }
        seen.insert(newpath.clone(), None);
        let target = fs::read_link(&newpath)
            .map(|target| target.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (resolved, ok) = join_real_path(&path, &target, seen);
        if !ok {
            return (posix_join(&resolved, rest), false);
        }
        seen.insert(newpath, Some(resolved.clone()));
        path = resolved;
    }
    (path, true)
}

pub fn is_within(path: &str, root: &str) -> bool {
    if root == "/" {
        return path.starts_with('/');
    }
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub fn relative_to(path: &str, root: &str) -> Option<String> {
    if root == "/" {
        return Some(path.trim_start_matches('/').to_string());
    }
    path.strip_prefix(&format!("{root}/")).map(str::to_string)
}

fn lexical_parent(path: &str) -> String {
    // The Python takes the parent of a path after its trailing
    // separator drops, so the parent of a path spelled with a
    // trailing slash is the parent of the path it spells, never
    // the path itself.
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(index) => trimmed[..index].to_string(),
    }
}

/// Resolve a traced path canonically and scope it to the root.
///
/// Returns a pair. The first item is the resolved path relative
/// to the root when the path lands inside the root, else None.
/// The second item is the resolved absolute path when the path
/// is an outside read, else None. Canonical resolution follows
/// symlink components on disk and settles dot segments, so a
/// path is judged by where it actually lands. A path that
/// resolves to the root itself names no file inside it and
/// lands in neither item.
///
/// An outside read is a path whose written form lands inside
/// the root once dot segments settle lexically, yet whose
/// canonical resolution lands outside it, which is the shape
/// of an escape through a symlink. The same naming covers a
/// path spelled through an alias of the root itself, when the
/// directory the path hangs from resolves inside the root
/// while the path itself resolves outside it. A path that was
/// already outside the root in its written form and hangs from
/// a directory outside the root, the way system library opens
/// are, is dropped unnamed.
pub fn canonical_scoped(raw: &str, root: &str) -> (Option<String>, Option<String>) {
    canonical_scoped_based(raw, root, None)
}

/// Resolve a traced path taken against a base directory.
///
/// The base is the directory in force for the process that
/// opened the path, or the directory a recorded descriptor
/// names, and a relative path is taken against it instead of
/// the root. A path that resolves to the base itself names no
/// file inside it and lands in neither item, the same resting
/// the root itself gets. With no base standing, this is the
/// slice 1 reading the root alone gives.
pub fn canonical_scoped_based(
    raw: &str,
    root: &str,
    base: Option<&str>,
) -> (Option<String>, Option<String>) {
    if raw.is_empty() {
        return (None, None);
    }
    let candidate = if raw.starts_with('/') {
        raw.to_string()
    } else {
        match base {
            Some(base) => posix_join(base, raw),
            None => posix_join(root, raw),
        }
    };
    let written = normpath(&candidate);
    let written_inside = is_within(&written, root);
    let resolved = realpath(&candidate);
    if resolved == root {
        return (None, None);
    }
    if let Some(base) = base {
        if resolved == realpath(base) {
            return (None, None);
        }
    }
    if let Some(relative) = relative_to(&resolved, root) {
        return (Some(relative), None);
    }
    let parent_resolved = realpath(&lexical_parent(&candidate));
    if written_inside || is_within(&parent_resolved, root) {
        return (None, Some(resolved));
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn fresh_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("blindgate-scope-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(&dir).unwrap()
    }

    #[test]
    fn normpath_settles_dot_segments() {
        assert_eq!(normpath("/a/b/../c/./d"), "/a/c/d");
        assert_eq!(normpath("/a/../../b"), "/b");
        assert_eq!(normpath("/"), "/");
    }

    #[cfg(unix)]
    #[test]
    fn realpath_follows_a_symlink_and_a_dangling_tail() {
        let base = fresh_dir();
        let outside = base.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "x").unwrap();
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), project.join("escape.txt")).unwrap();
        std::os::unix::fs::symlink(outside.join("missing.txt"), project.join("dangling")).unwrap();
        let base = base.to_string_lossy().into_owned();
        assert_eq!(
            realpath(&format!("{base}/project/escape.txt")),
            format!("{base}/outside/secret.txt")
        );
        assert_eq!(
            realpath(&format!("{base}/project/dangling")),
            format!("{base}/outside/missing.txt")
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn lexical_parent_drops_a_trailing_separator_first() {
        assert_eq!(lexical_parent("/a/b/"), "/a");
        assert_eq!(lexical_parent("/a/b"), "/a");
        assert_eq!(lexical_parent("/a/"), "/");
        assert_eq!(lexical_parent("/"), "/");
    }

    #[cfg(unix)]
    #[test]
    fn realpath_resolves_a_chain_of_any_length() {
        let base = fresh_dir();
        let project = base.join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("src/app.py"), "x").unwrap();
        let names: Vec<String> = (1..=50).map(|n| format!("link{n:02}")).collect();
        for (index, name) in names.iter().enumerate() {
            let target = names
                .get(index + 1)
                .map_or("src/app.py", |next| next.as_str());
            std::os::unix::fs::symlink(target, project.join(name)).unwrap();
        }
        let base = base.to_string_lossy().into_owned();
        assert_eq!(
            realpath(&format!("{base}/project/link01")),
            format!("{base}/project/src/app.py")
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn realpath_rests_on_the_spelling_a_loop_was_entered_by() {
        let base = fresh_dir();
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        std::os::unix::fs::symlink("triob", project.join("trioa")).unwrap();
        std::os::unix::fs::symlink("trioc", project.join("triob")).unwrap();
        std::os::unix::fs::symlink("trioa", project.join("trioc")).unwrap();
        std::os::unix::fs::symlink("selfl", project.join("selfl")).unwrap();
        let base = base.to_string_lossy().into_owned();
        for name in ["trioa", "triob", "trioc", "selfl"] {
            assert_eq!(
                realpath(&format!("{base}/project/{name}")),
                format!("{base}/project/{name}"),
                "loop entry {name}"
            );
        }
        assert_eq!(
            realpath(&format!("{base}/project/trioa/tail.txt")),
            format!("{base}/project/trioa/tail.txt")
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn canonical_scoped_names_a_trailing_slash_alias_escape() {
        let base = fresh_dir();
        let outside = base.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "x").unwrap();
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), project.join("escape.txt")).unwrap();
        std::os::unix::fs::symlink(&project, base.join("rootlink")).unwrap();
        let root = project.to_string_lossy().into_owned();
        let base = base.to_string_lossy().into_owned();
        assert_eq!(
            canonical_scoped(&format!("{base}/rootlink/escape.txt/"), &root),
            (None, Some(format!("{base}/outside/secret.txt")))
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn canonical_scoped_names_an_escape_and_reports_an_inside_read() {
        let base = fresh_dir();
        let outside = base.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "x").unwrap();
        let project = base.join("project");
        fs::create_dir_all(project.join("docs")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), project.join("escape.txt")).unwrap();
        let root = project.to_string_lossy().into_owned();
        let base = base.to_string_lossy().into_owned();
        assert_eq!(
            canonical_scoped("escape.txt", &root),
            (None, Some(format!("{base}/outside/secret.txt")))
        );
        assert_eq!(
            canonical_scoped("docs/real.md", &root),
            (Some("docs/real.md".to_string()), None)
        );
        assert_eq!(canonical_scoped(".", &root), (None, None));
        assert_eq!(canonical_scoped("/etc/hostname", &root), (None, None));
        fs::remove_dir_all(&base).unwrap();
    }
}
