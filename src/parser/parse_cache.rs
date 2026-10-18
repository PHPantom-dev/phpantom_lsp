//! The thread-local parse cache.
//!
//! During a single pass (an LSP request, or diagnostics over one file) the
//! file content is immutable and `with_parsed_program` may be called dozens
//! of times for the same content string (once per unique variable subject,
//! plus secondary helpers).  Each call would otherwise allocate a fresh
//! `LocalArena` and re-parse the entire file from scratch.
//!
//! The cache below eliminates that redundancy.  [`with_parse_cache`] stores
//! only the content (no parsing).  The first [`with_parsed_program`] call
//! whose content matches then lazily parses the file and keeps the arena
//! and the resulting `Program`.  Subsequent calls reuse the cached AST.
//!
//! A pass also reads other files: inferring the return type of a method
//! declared elsewhere walks that method's body, and every resolution inside
//! the walk parses the declaring file.  A nested [`with_parse_cache`] for a
//! different file therefore registers it alongside the pass's own file, so
//! all of those parses share one AST until the pass ends.  Only the most
//! recently registered few are kept, which bounds what a pass that reaches
//! into many files holds at once.
//!
//! The cache is activated by the outermost [`with_parse_cache`], which sets
//! it up at the start of a block and tears it down (via `Drop`) when the
//! block finishes.  Outside that scope `with_parsed_program` parses every
//! time.
//!
//! ## Safety
//!
//! The `Program<'arena>` borrows from the `LocalArena` and the content
//! string, both owned by the same [`CachedFile`].  The raw pointer is
//! reconstituted to a reference only through a `&CachedFile`, and a caller
//! holds an `Rc` to the file for as long as it uses the program, so neither
//! eviction nor the guard clearing the cache can free them underneath it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use mago_syntax::cst::Program;

/// How many files besides the pass's own one the cache keeps parsed.
const MAX_CACHED_OTHER_FILES: usize = 4;

/// One file's content and, once something has asked for it, its AST.
struct CachedFile {
    content: Arc<String>,
    /// The arena that owns the AST nodes, and a pointer to the `Program`
    /// allocated in it.  Unset until the first parse of this file.
    parsed: std::cell::OnceCell<(mago_allocator::LocalArena, *const ())>,
    /// The file's `namespace` declarations, scanned on first use.
    namespaces: std::cell::OnceCell<crate::text_scan::NamespaceDecls>,
    /// The file's first-class callable assignments, collected on first use.
    fcc_assignments: std::cell::OnceCell<Vec<crate::signature_help::FccAssignment>>,
    /// The template parameters and type aliases the file's docblocks
    /// declare, with their scopes, collected on first use.
    docblock_type_names: std::cell::OnceCell<crate::names::DocblockTypeNames>,
    /// The file's comments, collected from its trivia on first use.
    comments: std::cell::OnceCell<Vec<CommentSpan>>,
    /// Address and length of each borrowed copy of `content` a live
    /// [`ParseCacheGuard`] registered this file from.
    ///
    /// [`with_parse_cache`] copies borrowed text into the cache, so the
    /// caller's own `&str`, which is what every later lookup passes, never
    /// shares an address with `content`.  Without these, each lookup would
    /// compare the whole file byte by byte.  An entry lives only as long as
    /// the guard that added it, whose lifetime keeps the borrowed text
    /// alive and unchanged, so an address match here is a content match.
    aliases: RefCell<Vec<(usize, usize)>>,
}

impl CachedFile {
    fn new(content: Arc<String>) -> Rc<Self> {
        Rc::new(Self {
            content,
            parsed: std::cell::OnceCell::new(),
            namespaces: std::cell::OnceCell::new(),
            fcc_assignments: std::cell::OnceCell::new(),
            docblock_type_names: std::cell::OnceCell::new(),
            comments: std::cell::OnceCell::new(),
            aliases: RefCell::new(Vec::new()),
        })
    }

    fn holds(&self, content: &str) -> bool {
        // The pointer comparison includes the length, so a prefix of the
        // cached text, which starts at the same address, is not mistaken
        // for the whole of it.
        std::ptr::eq(self.content.as_str(), content)
            || self.aliases.borrow().contains(&text_address(content))
            || self.content.as_str() == content
    }

    fn program(&self) -> &Program<'_> {
        let (_, program_ptr) = self.parsed.get_or_init(|| {
            let arena = mago_allocator::LocalArena::new();
            let file_id = mago_database::file::FileId::new(b"input.php");
            let program =
                mago_syntax::parser::parse_file_content(&arena, file_id, self.content.as_bytes());
            // The arena's chunks are heap-allocated and do not move when
            // the arena itself does, so the pointer outlives this move.
            let program_ptr: *const () = (program as *const Program<'_>).cast();
            (arena, program_ptr)
        });
        // SAFETY: the pointer came from a `&Program` that borrows only
        // the arena and `self.content`, both owned by `self`, which the
        // returned reference cannot outlive.
        unsafe { &*program_ptr.cast::<Program<'_>>() }
    }
}

/// The files of the active pass: its own file first, then the other files
/// it registered, least recently registered first.
struct ParseCache {
    files: Vec<Rc<CachedFile>>,
}

impl ParseCache {
    fn find(&self, content: &str) -> Option<Rc<CachedFile>> {
        self.files.iter().find(|file| file.holds(content)).cloned()
    }

    /// Make `content` available to the pass, evicting the oldest other
    /// file when that leaves too many.
    fn register(&mut self, content: &str, to_arc: impl FnOnce() -> Arc<String>) -> Rc<CachedFile> {
        let file = match self.files.iter().position(|file| file.holds(content)) {
            Some(0) => return Rc::clone(&self.files[0]),
            Some(index) => self.files.remove(index),
            None => {
                if self.files.len() > MAX_CACHED_OTHER_FILES {
                    self.files.remove(1);
                }
                CachedFile::new(to_arc())
            }
        };
        self.files.push(Rc::clone(&file));
        file
    }

    fn evict(&mut self, file: &Rc<CachedFile>) {
        self.files.retain(|cached| !Rc::ptr_eq(cached, file));
    }
}

thread_local! {
    static PARSE_CACHE: RefCell<Option<ParseCache>> = const { RefCell::new(None) };
}

/// RAII guard that clears the thread-local parse cache on drop.
///
/// Created by [`with_parse_cache`].  Must not be leaked (e.g. via
/// `std::mem::forget`) — doing so would leave the cache active past the
/// pass it belongs to.
pub(crate) struct ParseCacheGuard<'a> {
    /// `true` when this guard owns the cache and must clear it on drop.
    /// Nested guards have `owns_cache = false` and leave it untouched.
    owns_cache: bool,
    /// The file and the address of the borrowed text this guard added to
    /// its [`CachedFile::aliases`], withdrawn on drop.
    alias: Option<(std::rc::Weak<CachedFile>, (usize, usize))>,
    _content: std::marker::PhantomData<&'a str>,
}

impl Drop for ParseCacheGuard<'_> {
    fn drop(&mut self) {
        if let Some((file, address)) = self.alias.take()
            && let Some(file) = file.upgrade()
        {
            let mut aliases = file.aliases.borrow_mut();
            if let Some(index) = aliases.iter().position(|a| *a == address) {
                aliases.swap_remove(index);
            }
        }
        if self.owns_cache {
            PARSE_CACHE.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }
}

/// Activate the thread-local parse cache for `content`.
///
/// While the returned [`ParseCacheGuard`] is alive, every call to
/// [`with_parsed_program`] whose `content` argument is byte-equal to
/// the cached content will reuse the already-parsed `Program` instead
/// of re-parsing.
///
/// Typical usage:
///
/// ```ignore
/// let _guard = with_parse_cache(content);
/// // … many calls to resolve_variable_types / resolve_variable_type / etc.
/// // All of them hit the cache instead of re-parsing.
/// // Guard is dropped here, clearing the cache.
/// ```
///
/// Called while a cache is already active, this adds `content` to it for
/// the rest of the outer pass and returns a guard that does nothing.
pub(crate) fn with_parse_cache(content: &str) -> ParseCacheGuard<'_> {
    let (mut guard, file) = install_parse_cache(content, || Arc::new(content.to_string()));
    let address = text_address(content);
    file.aliases.borrow_mut().push(address);
    guard.alias = Some((Rc::downgrade(&file), address));
    guard
}

/// [`with_parse_cache`] for a caller that already holds the content
/// behind an `Arc`, which the cache then shares instead of copying.
///
/// Every LSP request passes through here, so the copy would otherwise be
/// paid once per request whether or not anything goes on to parse.
pub(crate) fn with_parse_cache_arc(content: Arc<String>) -> ParseCacheGuard<'static> {
    let text = Arc::clone(&content);
    install_parse_cache(&text, move || content).0
}

fn text_address(content: &str) -> (usize, usize) {
    (content.as_ptr() as usize, content.len())
}

fn install_parse_cache<'a>(
    content: &str,
    to_arc: impl FnOnce() -> Arc<String>,
) -> (ParseCacheGuard<'a>, Rc<CachedFile>) {
    PARSE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        let (file, owns_cache) = match cache.as_mut() {
            Some(cache) => (cache.register(content, to_arc), false),
            None => {
                // Store only the content.  The parse is deferred until the
                // first `with_parsed_program` call that asks for it.
                let file = CachedFile::new(to_arc());
                *cache = Some(ParseCache {
                    files: vec![Rc::clone(&file)],
                });
                (file, true)
            }
        };
        let guard = ParseCacheGuard {
            owns_cache,
            alias: None,
            _content: std::marker::PhantomData,
        };
        (guard, file)
    })
}

/// Parse `content` with the mago-syntax parser and pass the resulting
/// `Program` (plus the content string) to `f`.
///
/// Handles the boilerplate that every parse entry-point needs:
/// allocating a `LocalArena` arena, creating a `FileId`, calling
/// `parse_file_content`, and wrapping the whole thing in
/// `catch_unwind` so that a parser panic doesn't crash the LSP
/// server.  On panic the error is logged (using `method_name` for
/// context) and `T::default()` is returned.
///
/// When the thread-local parse cache is active (see
/// [`with_parse_cache`]) and holds `content`, the previously parsed
/// `Program` is reused — no allocation or parsing occurs.
pub(crate) fn with_parsed_program<T: Default>(
    content: &str,
    method_name: &str,
    f: impl FnOnce(&Program<'_>, &str) -> T,
) -> T {
    // ── Fast path: check the thread-local cache ─────────────────
    // The borrow ends before `f` runs, so `f` is free to register more
    // files or parse others through the cache.
    let cached = PARSE_CACHE.with(|cell| cell.borrow().as_ref().and_then(|c| c.find(content)));

    if let Some(file) = cached {
        return with_cached_file(file, method_name, |file| {
            f(file.program(), file.content.as_str())
        });
    }

    // ── Slow path: parse from scratch ───────────────────────────
    let content_owned = content.to_string();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program =
            mago_syntax::parser::parse_file_content(&arena, file_id, content_owned.as_bytes());
        f(program, &content_owned)
    }));

    match result {
        Ok(value) => value,
        Err(_) => {
            tracing::error!("PHPantom: parser panicked in {}", method_name);
            T::default()
        }
    }
}

/// Run `f` on a file of the active cache.
///
/// `f` may parse the file lazily and then read the program, and either can
/// panic. Like a fresh parse, a panic is logged (with `method_name`) and
/// answered with `T::default()` instead of taking the LSP server down, and
/// the file is evicted so the next call re-parses it rather than reading a
/// half-built entry.
fn with_cached_file<T: Default>(
    file: Rc<CachedFile>,
    method_name: &str,
    f: impl FnOnce(&CachedFile) -> T,
) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&file))) {
        Ok(value) => value,
        Err(_) => {
            PARSE_CACHE.with(|cell| {
                if let Some(cache) = cell.borrow_mut().as_mut() {
                    cache.evict(&file);
                }
            });
            tracing::error!("PHPantom: parser panicked in {}", method_name);
            T::default()
        }
    }
}

/// The namespace in force at `offset` in `content`, or `None` for the
/// global namespace.
///
/// A file in the active parse cache answers from declarations scanned
/// once per pass, so a pass that asks at every access in a long file does
/// not rescan the text up to each one.  Other text falls back to
/// [`text_scan::namespace_at_offset`](crate::text_scan::namespace_at_offset).
pub(crate) fn namespace_at_offset(content: &str, offset: usize) -> Option<&str> {
    let cached = PARSE_CACHE.with(|cell| cell.borrow().as_ref().and_then(|c| c.find(content)));
    match cached {
        Some(file) => file
            .namespaces
            .get_or_init(|| crate::text_scan::NamespaceDecls::scan(&file.content))
            .at(content, offset),
        None => crate::text_scan::namespace_at_offset(content, offset),
    }
}

/// The offset of the `namespace` keyword whose declaration is in force at
/// `offset` in `content`, which tells the file's blocks apart, or `None`
/// before the first one.  Answered the way [`namespace_at_offset`] is.
pub(crate) fn namespace_decl_at(content: &str, offset: usize) -> Option<usize> {
    let cached = PARSE_CACHE.with(|cell| cell.borrow().as_ref().and_then(|c| c.find(content)));
    match cached {
        Some(file) => file
            .namespaces
            .get_or_init(|| crate::text_scan::NamespaceDecls::scan(&file.content))
            .decl_at(offset),
        None => crate::text_scan::namespace_decl_at(content, offset),
    }
}

/// Whether a docblock in `content` declares `name` as a template
/// parameter or a type alias on a declaration overlapping `region`.
///
/// A file in the active parse cache collects its declared names once per
/// pass, so a pass that checks every docblock name it cannot resolve does
/// not rescan the file for each.  Other text is searched for `name` first
/// and parsed for scopes only when some docblock declares it.
pub(crate) fn declares_docblock_type_name(content: &str, name: &str, region: (u32, u32)) -> bool {
    let cached = PARSE_CACHE.with(|cell| cell.borrow().as_ref().and_then(|c| c.find(content)));
    match cached {
        Some(file) => file
            .docblock_type_names
            .get_or_init(|| crate::names::DocblockTypeNames::collect(&file.content))
            .declares(name, region),
        None => {
            crate::names::scan_declares_docblock_type_name(content, name)
                && crate::names::DocblockTypeNames::collect(content).declares(name, region)
        }
    }
}

/// Pass `content`'s first-class callable assignments (`$fn = target(...)`)
/// to `f`.
///
/// A file in the active parse cache collects them once per pass, so a pass
/// that asks at every call through a variable in a long file does not walk
/// the whole file at each one.  Other text is parsed and walked per call.
pub(crate) fn with_fcc_assignments<T: Default>(
    content: &str,
    f: impl FnOnce(&[crate::signature_help::FccAssignment]) -> T,
) -> T {
    let cached = PARSE_CACHE.with(|cell| cell.borrow().as_ref().and_then(|c| c.find(content)));
    match cached {
        Some(file) => with_cached_file(file, "with_fcc_assignments", |file| {
            let assignments = file.fcc_assignments.get_or_init(|| {
                crate::signature_help::collect_fcc_assignments(
                    file.program().statements.as_slice(),
                    &file.content,
                )
            });
            f(assignments.as_slice())
        }),
        None => with_parsed_program(content, "with_fcc_assignments", |program, content| {
            f(&crate::signature_help::collect_fcc_assignments(
                program.statements.as_slice(),
                content,
            ))
        }),
    }
}

/// What kind of comment a [`CommentSpan`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommentKind {
    /// `// …` or `# …`.
    Line,
    /// `/* … */`.
    Block,
    /// `/** … */`.
    Doc,
}

/// One comment in a file, by byte range.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CommentSpan {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) kind: CommentKind,
}

fn collect_comments(program: &Program<'_>) -> Vec<CommentSpan> {
    use mago_syntax::cst::TriviaKind;
    program
        .trivia
        .iter()
        .filter_map(|trivia| {
            let kind = match trivia.kind {
                TriviaKind::WhiteSpace => return None,
                TriviaKind::SingleLineComment | TriviaKind::HashComment => CommentKind::Line,
                TriviaKind::MultiLineComment => CommentKind::Block,
                TriviaKind::DocBlockComment => CommentKind::Doc,
            };
            Some(CommentSpan {
                start: trivia.span.start.offset,
                end: trivia.span.end.offset,
                kind,
            })
        })
        .collect()
}

/// Pass `content`'s comments, in source order, to `f`.
///
/// A file in the active parse cache collects them once per pass, so the
/// forward walker can look up the docblock above every statement it visits
/// without scanning the text before it.  Other text is parsed per call.
pub(crate) fn with_comments<T: Default>(content: &str, f: impl FnOnce(&[CommentSpan]) -> T) -> T {
    let cached = PARSE_CACHE.with(|cell| cell.borrow().as_ref().and_then(|c| c.find(content)));
    match cached {
        Some(file) => with_cached_file(file, "with_comments", |file| {
            f(file
                .comments
                .get_or_init(|| collect_comments(file.program()))
                .as_slice())
        }),
        None => with_parsed_program(content, "with_comments", |program, _| {
            f(&collect_comments(program))
        }),
    }
}

/// The docblock above the declaration starting at `node_start`, which may
/// be separated from it by anything but a statement or a block boundary
/// (modifiers, attributes, other comments).
///
/// Comments are lexed whether or not the code around them parses, so this
/// also answers in a file that is mid-edit.
pub(crate) fn preceding_docblock_text(content: &str, node_start: usize) -> Option<&str> {
    let mut found = None;
    with_comments(content, |comments| {
        let mut covered_from = node_start.min(content.len());
        let above = comments.partition_point(|c| c.end as usize <= covered_from);
        for comment in comments[..above].iter().rev() {
            let gap = content
                .get(comment.end as usize..covered_from)
                .unwrap_or("");
            if gap.contains([';', '{', '}']) {
                return;
            }
            if comment.kind == CommentKind::Doc {
                found = content.get(comment.start as usize..comment.end as usize);
                return;
            }
            covered_from = comment.start as usize;
        }
    });
    found
}

#[cfg(test)]
mod with_parsed_program_tests {
    //! A panic in the parser or extraction closure must be caught on the
    //! thread-local-cache fast path too (not just the slow path), and the
    //! poisoned cache entry must be evicted so the next call re-parses.

    use super::{with_parse_cache, with_parsed_program};

    #[test]
    fn fast_path_lazy_parse_recovers_from_panic() {
        let content = "<?php class Foo {}";
        let _guard = with_parse_cache(content);

        // First matching call lazily parses, then the closure panics.
        // The fast path must catch it and return the default.
        let caught: bool =
            with_parsed_program(content, "test", |_program, _content| panic!("boom"));
        assert!(
            !caught,
            "fast-path panic should be caught and default returned"
        );

        // The poisoned entry was evicted, so a later call re-parses and
        // runs the closure normally.
        let ok: bool = with_parsed_program(content, "test", |_program, _content| true);
        assert!(ok, "after eviction the next call must re-parse and succeed");
    }

    #[test]
    fn fast_path_ready_cache_recovers_from_panic() {
        let content = "<?php class Bar {}";
        let _guard = with_parse_cache(content);

        // Prime the cache so the program is already parsed (ready state).
        let primed: bool = with_parsed_program(content, "test", |_program, _content| true);
        assert!(primed);

        // A panic on the ready fast path is also caught.
        let caught: bool =
            with_parsed_program(content, "test", |_program, _content| panic!("boom"));
        assert!(
            !caught,
            "ready-cache panic should be caught and default returned"
        );

        // And the entry is evicted so subsequent calls still work.
        let ok: bool = with_parsed_program(content, "test", |_program, _content| true);
        assert!(ok, "after eviction the next call must re-parse and succeed");
    }

    fn program_address(content: &str) -> usize {
        with_parsed_program(content, "test", |program, _| {
            std::ptr::from_ref(program) as usize
        })
    }

    /// A prefix of the pass's file starts at the same address as the file
    /// itself, but it is different text and has to be parsed as itself.
    #[test]
    fn a_prefix_of_the_cached_file_is_not_handed_the_whole_files_program() {
        let content = "<?php class Foo {}\nclass Bar {}\n";
        let _guard = with_parse_cache(content);
        let prefix = &content[.."<?php class Foo {}".len()];

        let whole = with_parsed_program(content, "test", |program, _| program.statements.len());
        let (statements, text) = with_parsed_program(prefix, "test", |program, text| {
            (program.statements.len(), text.len())
        });
        assert_eq!(text, prefix.len());
        assert!(statements < whole, "{statements} statements of {whole}");
    }

    #[test]
    fn nested_scope_shares_one_parse_of_another_file_for_the_pass() {
        let own = "<?php class Own {}";
        let other = "<?php class Other {}";
        let _guard = with_parse_cache(own);

        // Registered from inside a parse of the pass's own file, the way a
        // body inference reaches into a declaring file mid-walk.
        let first = with_parsed_program(own, "test", |_, _| {
            let _nested = super::with_parse_cache(other);
            program_address(other)
        });
        assert_ne!(first, 0);
        assert_eq!(
            program_address(other),
            first,
            "the other file stays parsed after the nested scope ends"
        );
    }

    #[test]
    fn other_files_are_evicted_oldest_first_but_never_the_own_file() {
        let own = "<?php class Own {}";
        let _guard = with_parse_cache(own);
        let own_program = program_address(own);

        let others: Vec<String> = (0..=super::MAX_CACHED_OTHER_FILES)
            .map(|i| format!("<?php class Other{i} {{}}"))
            .collect();
        for other in &others {
            let _nested = super::with_parse_cache(other);
        }

        assert_eq!(program_address(own), own_program);
        let cached = |content: &str| {
            super::PARSE_CACHE.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .is_some_and(|cache| cache.find(content).is_some())
            })
        };
        assert!(!cached(&others[0]), "the oldest other file was evicted");
        assert!(others[1..].iter().all(|other| cached(other)));
    }
}
