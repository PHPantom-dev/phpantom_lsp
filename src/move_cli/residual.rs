//! Post-plan scan for occurrences a move could not rewrite.
//!
//! The rewriter only reaches what it can resolve as a symbol: PHP
//! declarations, imports, and references. A namespace named in a Blade
//! template, a `.neon` baseline, or a plain path string is invisible to
//! it, so `files_changed` alone cannot tell a complete rewrite from a
//! partial one. This pass greps the project as it will look once the
//! plan is applied and reports every leftover mention of the old name
//! or the old location, so the count can be read against a stated list
//! of what was left alone.
//!
//! Rewriting those hits is deliberately out of scope. Nothing here can
//! know whether `base_path('app/Domain/Foo.json')` names a path or a
//! label, and the matching key in a deployment secret is out of reach
//! entirely. Reporting them is not.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use memchr::memmem;

use super::{MovePlan, MoveWarning};
use crate::Backend;

/// Files larger than this are skipped.
///
/// PHP sources, configs, and templates sit far below it; what lives
/// above is a fixture dump or a bundled asset, where reading the whole
/// file to grep for a namespace costs more than the warning is worth.
const MAX_SCANNED_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// How many leading bytes are checked for a NUL before a file is
/// treated as binary and skipped.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

/// What a needle is spelled like, which decides what counts as a
/// boundary around a match.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NeedleKind {
    /// A PHP name, whose segments are separated by backslashes.
    Name,
    /// A project-relative path, whose segments are separated by slashes.
    Path,
}

struct Needle {
    text: String,
    kind: NeedleKind,
}

struct Hit {
    /// Where the occurrence will be once the plan is applied.
    path: PathBuf,
    /// 1-based line number within that file.
    line: usize,
    /// Index into the needle list, so the same line matching two
    /// needles is reported twice but the same needle twice on one line
    /// only once.
    needle: usize,
}

/// Scan the post-move project for mentions of the old name or location
/// that the plan does not rewrite.
///
/// `old_path` is the pre-move location the source occupied: the
/// declaring file for a class move, the PSR-4 directory for a namespace
/// move. That is what turns up in path strings.
pub(super) fn residual_warnings(
    backend: &Backend,
    root: &Path,
    from_name: &str,
    old_path: Option<&Path>,
    plan: &MovePlan,
) -> Vec<MoveWarning> {
    let needles = build_needles(backend, root, from_name, old_path);
    if needles.is_empty() {
        return Vec::new();
    }

    // Where the file that declared the source ends up, so a needle
    // that is a bare name can leave the declaration it names alone.
    let declaration =
        old_path.map(|path| planned_path(path, &plan.moves).unwrap_or_else(|| path.to_path_buf()));

    let mut hits = collect_hits(backend, root, &needles, declaration.as_deref(), plan);
    hits.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.needle.cmp(&b.needle))
    });
    hits.dedup_by(|a, b| a.path == b.path && a.line == b.line && a.needle == b.needle);

    hits.into_iter()
        .map(|hit| {
            let needle = &needles[hit.needle];
            let subject = match needle.kind {
                NeedleKind::Name => "name",
                NeedleKind::Path => "path",
            };
            MoveWarning {
                message: format!(
                    "The old {subject} `{}` still appears here. The move does not rewrite it, \
                     because nothing here resolves it to the symbol that moved.",
                    needle.text
                ),
                file: Some(super::relative_display(root, &hit.path)),
                line: Some(hit.line),
            }
        })
        .collect()
}

/// The strings whose presence after the move means something was left
/// behind.
fn build_needles(
    backend: &Backend,
    root: &Path,
    from_name: &str,
    old_path: Option<&Path>,
) -> Vec<Needle> {
    let mut needles = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |text: String, kind: NeedleKind| {
        if !text.is_empty() && seen.insert(text.clone()) {
            needles.push(Needle { text, kind });
        }
    };

    push(from_name.to_string(), NeedleKind::Name);
    // A double-quoted PHP string, a JSON document, and a PHP namespace
    // written into either escape the separator, so the same name reads
    // as `App\\Old` on disk.
    push(from_name.replace('\\', "\\\\"), NeedleKind::Name);

    let mappings = backend.psr4_mappings().read();
    if let Some(old) = old_path
        && let Ok(relative) = old.strip_prefix(root)
    {
        let relative = relative.to_string_lossy().replace('\\', "/");
        // A single leading segment (`app`, `src`) matches half the
        // project on its own, so a path needle has to name at least two.
        if relative.contains('/') {
            push(relative.clone(), NeedleKind::Path);
            // Laravel's `app_path()`, Symfony's `%kernel.project_dir%`,
            // and every hand-rolled equivalent spell paths relative to
            // the autoload root rather than the project root.
            for mapping in mappings.iter() {
                let base = mapping.base_path.trim_end_matches('/');
                if base.is_empty() {
                    continue;
                }
                if let Some(below) = relative
                    .strip_prefix(base)
                    .and_then(|below| below.strip_prefix('/'))
                    && below.contains('/')
                {
                    push(below.to_string(), NeedleKind::Path);
                }
            }
        }
    }

    needles
}

/// Walk the project and search every file for every needle, reading the
/// content the plan will leave behind rather than what is on disk now.
fn collect_hits(
    backend: &Backend,
    root: &Path,
    needles: &[Needle],
    declaration: Option<&Path>,
    plan: &MovePlan,
) -> Vec<Hit> {
    use ignore::{WalkBuilder, WalkState};

    let written: std::collections::HashMap<&Path, &str> = plan
        .writes
        .iter()
        .map(|(path, content)| (path.as_path(), content.as_str()))
        .collect();
    let finders: Vec<memmem::Finder<'_>> = needles
        .iter()
        .map(|needle| memmem::Finder::new(needle.text.as_bytes()))
        .collect();

    // A class that was in the global namespace has a bare short name for
    // an old FQN, and the move leaves the declaration spelled exactly
    // that way on purpose, so such a needle cannot count its own
    // declaration site as something left behind.
    let bare_names: Vec<bool> = needles
        .iter()
        .map(|needle| needle.kind == NeedleKind::Name && !needle.text.contains('\\'))
        .collect();
    let bare_names = &bare_names;

    let vendor_dirs = backend.workspace.vendor_dir_paths.lock().clone();
    let filters = backend.index_filters();
    let mut builder = WalkBuilder::new(root);
    builder
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        // Unlike the PHP walkers, dotfiles are in scope: a committed
        // `.env.example` or `.github/workflows/*.yml` naming the old
        // path is exactly the kind of leftover this pass exists to find.
        // `.gitignore` still keeps the untracked ones out.
        .hidden(false)
        .parents(true)
        .ignore(true)
        .threads(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
        )
        .filter_entry(move |entry| {
            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            if is_dir {
                if entry.file_name() == ".git" {
                    return false;
                }
                if vendor_dirs.iter().any(|vendor| vendor == entry.path()) {
                    return false;
                }
            }
            !filters.is_excluded_entry(entry.path(), is_dir)
        });

    let (tx, rx) = std::sync::mpsc::channel::<Hit>();
    let written = &written;
    let finders = &finders;
    let needles_kinds: Vec<NeedleKind> = needles.iter().map(|needle| needle.kind).collect();
    let needles_kinds = &needles_kinds;
    let moves = plan.moves.as_slice();
    builder.build_parallel().run(|| {
        let tx = tx.clone();
        Box::new(move |entry| {
            let Ok(entry) = entry else {
                return WalkState::Continue;
            };
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                return WalkState::Continue;
            }
            let source = entry.path();
            let planned = planned_path(source, moves);
            let target = planned.as_deref().unwrap_or(source);
            let content = match written.get(target) {
                Some(content) => std::borrow::Cow::Borrowed(*content),
                None => {
                    if entry
                        .metadata()
                        .is_ok_and(|meta| meta.len() > MAX_SCANNED_FILE_BYTES)
                    {
                        return WalkState::Continue;
                    }
                    match std::fs::read(source) {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(text) => std::borrow::Cow::Owned(text),
                            Err(_) => return WalkState::Continue,
                        },
                        Err(_) => return WalkState::Continue,
                    }
                }
            };
            let bytes = content.as_bytes();
            if bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
                return WalkState::Continue;
            }
            for (index, finder) in finders.iter().enumerate() {
                for offset in finder.find_iter(bytes) {
                    let end = offset + finder.needle().len();
                    if !is_bounded(bytes, offset, end, needles_kinds[index]) {
                        continue;
                    }
                    if bare_names[index]
                        && declaration == Some(target)
                        && is_declaration_site(bytes, offset)
                    {
                        continue;
                    }
                    let _ = tx.send(Hit {
                        path: target.to_path_buf(),
                        line: line_of(bytes, offset),
                        needle: index,
                    });
                }
            }
            WalkState::Continue
        })
    });
    drop(tx);

    rx.into_iter().collect()
}

/// Where a file ends up once the plan's renames are applied, or `None`
/// when no rename touches it.
fn planned_path(source: &Path, moves: &[(PathBuf, PathBuf)]) -> Option<PathBuf> {
    moves.iter().find_map(|(from, to)| {
        if source == from {
            return Some(to.clone());
        }
        source
            .strip_prefix(from)
            .ok()
            .and_then(|relative| (!relative.as_os_str().is_empty()).then(|| to.join(relative)))
    })
}

/// Whether a match sits on its own rather than inside a longer name or
/// path.
///
/// `App\Old` must not match `App\Older`, and `app/Config` must not match
/// `app/Configuration`. A leading separator is allowed on both sides,
/// since `\App\Old` and `/app/Config` are the same subject written
/// absolutely, but only when the character before it is not itself part
/// of a name: `Vendor\App\Old` is a different symbol that happens to end
/// the same way.
fn is_bounded(bytes: &[u8], start: usize, end: usize, kind: NeedleKind) -> bool {
    let separator = match kind {
        NeedleKind::Name => b'\\',
        NeedleKind::Path => b'/',
    };
    let continues = |byte: u8| match kind {
        NeedleKind::Name => byte.is_ascii_alphanumeric() || byte == b'_',
        NeedleKind::Path => byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'),
    };

    if bytes.get(end).is_some_and(|&byte| continues(byte)) {
        return false;
    }
    match start.checked_sub(1).map(|index| bytes[index]) {
        None => true,
        Some(byte) if byte == separator => !start
            .checked_sub(2)
            .map(|index| bytes[index])
            .is_some_and(continues),
        Some(byte) => !continues(byte),
    }
}

/// Whether the name starting at `offset` is the one a class-like
/// declaration gives itself.
///
/// Only the keyword immediately before it decides this; `abstract` and
/// `final` sit further left and change nothing about what follows.
fn is_declaration_site(bytes: &[u8], offset: usize) -> bool {
    const KEYWORDS: [&[u8]; 4] = [b"class", b"interface", b"trait", b"enum"];

    let mut keyword_end = offset;
    while keyword_end > 0 && bytes[keyword_end - 1].is_ascii_whitespace() {
        keyword_end -= 1;
    }
    if keyword_end == offset {
        return false;
    }
    let mut keyword_start = keyword_end;
    while keyword_start > 0 && bytes[keyword_start - 1].is_ascii_alphabetic() {
        keyword_start -= 1;
    }
    KEYWORDS
        .iter()
        .any(|keyword| bytes[keyword_start..keyword_end].eq_ignore_ascii_case(keyword))
}

/// The 1-based line the byte at `offset` falls on.
fn line_of(bytes: &[u8], offset: usize) -> usize {
    memchr::memchr_iter(b'\n', &bytes[..offset]).count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_longer_name_is_not_a_match() {
        let haystack = b"App\\Older";
        assert!(!is_bounded(haystack, 0, 7, NeedleKind::Name));
    }

    #[test]
    fn a_sub_namespace_is_a_match() {
        let haystack = b"App\\Old\\Widget";
        assert!(is_bounded(haystack, 0, 7, NeedleKind::Name));
    }

    #[test]
    fn a_leading_backslash_is_a_match() {
        let haystack = b"'\\App\\Old'";
        assert!(is_bounded(haystack, 2, 9, NeedleKind::Name));
    }

    #[test]
    fn a_longer_prefix_is_not_a_match() {
        let haystack = b"Vendor\\App\\Old";
        assert!(!is_bounded(haystack, 7, 14, NeedleKind::Name));
    }

    #[test]
    fn a_path_segment_boundary_is_required() {
        let haystack = b"app/Configuration/x";
        assert!(!is_bounded(haystack, 0, 10, NeedleKind::Path));
        let haystack = b"app/Config/x";
        assert!(is_bounded(haystack, 0, 10, NeedleKind::Path));
    }

    #[test]
    fn a_class_declaration_is_a_declaration_site() {
        let haystack = b"final class Widget\n{";
        assert!(is_declaration_site(haystack, 12));
    }

    #[test]
    fn a_reference_is_not_a_declaration_site() {
        let haystack = b"return new Widget();";
        assert!(!is_declaration_site(haystack, 11));
        let haystack = b"'Widget'";
        assert!(!is_declaration_site(haystack, 1));
    }

    #[test]
    fn lines_are_one_based() {
        assert_eq!(line_of(b"a\nb\nc", 0), 1);
        assert_eq!(line_of(b"a\nb\nc", 2), 2);
        assert_eq!(line_of(b"a\nb\nc", 4), 3);
    }
}
