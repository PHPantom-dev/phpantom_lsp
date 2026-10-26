use super::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_span::HasSpan;
use tower_lsp_server::gen_lsp_types::Uri;

use crate::Backend;
use crate::text_position::LineIndex;
use crate::virtual_members::laravel::provider_resources::{PathContext, resolve_path_arg};

// ─── Route source traversal ──────────────────────────────────────────────────

/// Where a route source sits on disk, and which files the chain of includes
/// reaching it has already opened.
///
/// `dir` anchors `__DIR__`-relative paths and `root` anchors `base_path()`
/// ones.  `open` is the cycle breaker: route files legitimately pull each
/// other in, and following a `require` back into a file already on the
/// include stack would not terminate.  `program` is the parse of the source
/// being scanned, which is what lets an include target written as a local
/// variable be followed back to the path it was assigned; every hop that
/// switches source (an included file, a macro body) replaces it along with
/// `dir`.
#[derive(Clone, Copy)]
pub(super) struct ScanPaths<'a> {
    pub(super) dir: Option<&'a Path>,
    pub(super) root: Option<&'a Path>,
    pub(super) open: &'a RefCell<HashSet<PathBuf>>,
    pub(super) macros: &'a MacroScope,
    pub(super) program: &'a Program<'a>,
    /// The file the walk is currently reading, which is where the route names
    /// it finds are declared.
    pub(super) uri: &'a Arc<Uri>,
    /// That file's line table, for the position of each declaration.
    pub(super) lines: &'a LineIndex<'a>,
}

impl<'a> ScanPaths<'a> {
    pub(super) fn new(
        dir: Option<&'a Path>,
        root: Option<&'a Path>,
        open: &'a RefCell<HashSet<PathBuf>>,
        macros: &'a MacroScope,
        program: &'a Program<'a>,
        uri: &'a Arc<Uri>,
        lines: &'a LineIndex<'a>,
    ) -> Self {
        Self {
            dir,
            root,
            open,
            macros,
            program,
            uri,
            lines,
        }
    }

    /// A route registered at byte `offset` of the file being read, with the
    /// byte range of the `->name()` argument spelling its name.
    pub(super) fn declaration(
        &self,
        offset: usize,
        name_arg: Option<(u32, u32)>,
    ) -> RouteDeclaration {
        RouteDeclaration {
            file: Arc::clone(self.uri),
            position: self.lines.position(offset),
            name_arg,
        }
    }
}

/// Parse a route file and hand the program to `walk`.
///
/// The arena the program is allocated in lives only as long as the call, so
/// every walk of a file — the route file itself, one it includes, a macro
/// body — runs inside one of these rather than holding the program.
pub(super) fn in_parsed_route_file<R>(
    content: &str,
    name: &'static [u8],
    walk: impl FnOnce(&Program<'_>) -> R,
) -> R {
    let arena = LocalArena::new();
    let file_id = FileId::new(name);
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    walk(program)
}

// ─── Router macros ───────────────────────────────────────────────────────────

/// The classes a route-registering macro attaches to.
///
/// `Route::macro('auth', …)` records the facade, and the macro index carries a
/// second copy targeting the concrete router the facade proxies.
/// `Route::mixin(new AuthRouteMethods)` — how `laravel/ui` ships
/// `Route::auth()` — lands on the same pair.
const ROUTER_MACRO_TARGETS: &[&str] = &[
    "Illuminate\\Support\\Facades\\Route",
    "Illuminate\\Routing\\Router",
    "Illuminate\\Routing\\RouteRegistrar",
];

/// The router macros a scan may need to expand, and the state of the scan's
/// expansions so far.
///
/// A route file that calls `Route::auth()` registers whatever that macro's
/// closure registers, but the closure lives outside the route tree (in a
/// service provider, or in a mixin class a provider passes to
/// `Route::mixin()`).  This resolves such calls through the project's macro
/// index and reads the file holding the body on demand, so a project with no
/// router macros pays only for the (empty) lookup table.
#[derive(Default)]
pub(super) struct MacroScope {
    /// Lowercased macro name → the URI of the file its closure was written in
    /// and the closure's byte offset there.
    pub(super) bodies: HashMap<String, (String, u32)>,
    /// The macro names on the expansion stack.  `Route::auth()` calls
    /// `$this->resetPassword()`, so bodies legitimately nest; this is what
    /// stops a macro that reaches itself again from recursing forever.
    pub(super) expanding: RefCell<HashSet<String>>,
    /// Source of each macro file, read once per scan.  `None` records a file
    /// that could not be read so the failure is not retried.
    pub(super) sources: RefCell<HashMap<String, Option<Arc<str>>>>,
}

impl MacroScope {
    /// The router macros registered in `backend`'s project.
    pub(super) fn for_backend(backend: &Backend) -> Self {
        if !backend.laravel.macros.has_any() {
            return Self::default();
        }
        Self {
            bodies: backend
                .laravel
                .macros
                .read()
                .macro_closures_on(ROUTER_MACRO_TARGETS),
            ..Self::default()
        }
    }

    /// Read (and memoize) the source of the file a macro closure lives in.
    fn source(&self, uri: &str) -> Option<Arc<str>> {
        if let Some(cached) = self.sources.borrow().get(uri) {
            return cached.clone();
        }
        let loaded = crate::util::uri_to_path(uri)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(Arc::<str>::from);
        self.sources
            .borrow_mut()
            .insert(uri.to_string(), loaded.clone());
        loaded
    }
}

/// A router macro's body, opened for walking.
///
/// Dropping it takes the macro's name back off the expansion stack, so a
/// second, sibling call to the same macro still expands while a cycle stops.
pub(super) struct OpenMacro<'a> {
    name: String,
    pub(super) content: Arc<str>,
    pub(super) uri: Arc<Uri>,
    pub(super) path: PathBuf,
    /// Byte offset of the closure within `content`.
    pub(super) offset: u32,
    scope: &'a MacroScope,
}

impl Drop for OpenMacro<'_> {
    fn drop(&mut self) {
        self.scope.expanding.borrow_mut().remove(&self.name);
    }
}

/// Resolve a call to `method` against the project's router macros and open the
/// file holding the macro's body.
///
/// Returns `None` when no router macro of that name is registered, when its
/// file cannot be read, or when the macro is already being expanded further up
/// the call stack.
pub(super) fn open_route_macro<'a>(scope: &'a MacroScope, method: &[u8]) -> Option<OpenMacro<'a>> {
    if scope.bodies.is_empty() {
        return None;
    }
    let name = std::str::from_utf8(method).ok()?.to_ascii_lowercase();
    let (uri, offset) = scope.bodies.get(&name)?;
    let url = crate::util::parse_url(uri)?;
    let path = crate::util::url_to_path(&url)?;
    if !scope.expanding.borrow_mut().insert(name.clone()) {
        return None;
    }
    // Constructed before the read so a failed read still pops the name off the
    // expansion stack when `open` is dropped.
    let mut open = OpenMacro {
        name,
        content: Arc::from(""),
        uri: Arc::new(url),
        path,
        offset: *offset,
        scope,
    };
    open.content = scope.source(uri)?;
    Some(open)
}

/// A macro body: the closure a `macro()` registration was given, or the one a
/// mixin method returns.
pub(super) enum MacroBody<'ast, 'arena> {
    Closure(&'ast Closure<'arena>),
    Arrow(&'ast ArrowFunction<'arena>),
}

/// Find the closure a macro registration points at, by its byte offset.
pub(super) fn macro_body_at<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    offset: u32,
) -> Option<MacroBody<'ast, 'arena>> {
    match node {
        Node::Closure(closure) if closure.span().start.offset == offset => {
            return Some(MacroBody::Closure(closure));
        }
        Node::ArrowFunction(arrow) if arrow.span().start.offset == offset => {
            return Some(MacroBody::Arrow(arrow));
        }
        _ => {}
    }
    let mut found = None;
    node.visit_children(|child| {
        if found.is_none() {
            found = macro_body_at(child, offset);
        }
    });
    found
}

/// A file that a route source pulls in, opened for scanning.
///
/// Removing the path from the include stack again is what
/// [`IncludedFile::drop`] is for, so a diamond include (two groups pulling
/// in the same helper file) still scans both times while a true cycle stops.
pub(super) struct IncludedFile<'a> {
    pub(super) path: PathBuf,
    pub(super) content: String,
    open: &'a RefCell<HashSet<PathBuf>>,
}

impl Drop for IncludedFile<'_> {
    fn drop(&mut self) {
        self.open.borrow_mut().remove(&self.path);
    }
}

/// Resolve an include target (`require __DIR__ . '/x.php'`,
/// `Route::group([], base_path('routes/x.php'))`) and read the file it
/// names, or return `None` when the path is not statically recoverable, the
/// file cannot be read, or following it would re-enter a file already open.
pub(super) fn open_included_file<'a>(
    target: &Expression<'_>,
    content: &str,
    paths: ScanPaths<'a>,
) -> Option<IncludedFile<'a>> {
    let dir = paths.dir?;
    let path = resolve_path_arg(
        target,
        content,
        &PathContext {
            file_dir: dir,
            workspace_root: paths.root.unwrap_or(dir),
            program: paths.program,
        },
    )?;
    let path = path.canonicalize().unwrap_or(path);
    if !paths.open.borrow_mut().insert(path.clone()) {
        return None;
    }
    // Constructed before the read so that a failed read still pops the path
    // off the include stack when `included` is dropped.
    let mut included = IncludedFile {
        content: String::new(),
        path,
        open: paths.open,
    };
    included.content = std::fs::read_to_string(&included.path).ok()?;
    Some(included)
}

/// Invoke `f` on every statement nested inside `stmt` that a route
/// registration can appear in.
///
/// Route sources are ordinary PHP: registrations sit inside `if` blocks and
/// loops, and a service provider registers them from a method body.
/// Closures are deliberately not descended into here — a closure registers
/// routes only as a `->group()` body, and that path applies the group's name
/// and URI prefixes before walking it.  A `foreach` is likewise absent: its
/// body is walked once per element with the loop variables bound, which the
/// callers do themselves.
pub(crate) fn for_each_nested_statement(stmt: &Statement<'_>, f: &mut dyn FnMut(&Statement<'_>)) {
    let mut visit = |statements: &[Statement<'_>]| statements.iter().for_each(&mut *f);

    match stmt {
        Statement::Namespace(ns) => visit(ns.statements().as_slice()),
        Statement::Block(b) => visit(b.statements.as_slice()),
        Statement::If(if_stmt) => crate::parser::for_each_if_branch(if_stmt, visit),
        Statement::While(w) => visit(w.body.statements()),
        Statement::DoWhile(dw) => f(dw.statement),
        Statement::For(fs) => visit(fs.body.statements()),
        Statement::Try(t) => {
            visit(t.block.statements.as_slice());
            for catch in t.catch_clauses.iter() {
                visit(catch.block.statements.as_slice());
            }
            if let Some(finally) = &t.finally_clause {
                visit(finally.block.statements.as_slice());
            }
        }
        Statement::Switch(sw) => {
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(c) => visit(c.statements.as_slice()),
                    SwitchCase::Default(c) => visit(c.statements.as_slice()),
                }
            }
        }
        Statement::Function(func) => visit(func.body.statements.as_slice()),
        // A service provider is a route source in its own right: its
        // `map()`/`boot()` bodies are where `Route::group(…)` runs.
        _ if let Some(body) = crate::parser::class_like_body(stmt) => {
            crate::parser::for_each_concrete_method(body.members, |_, block| {
                visit(block.statements.as_slice())
            });
        }
        _ => {}
    }
}

/// The expression naming the file an `include`/`require` construct pulls in.
pub(super) fn include_target<'arena>(
    construct: &Construct<'arena>,
) -> Option<&'arena Expression<'arena>> {
    match construct {
        Construct::Include(c) => Some(c.value),
        Construct::IncludeOnce(c) => Some(c.value),
        Construct::Require(c) => Some(c.value),
        Construct::RequireOnce(c) => Some(c.value),
        _ => None,
    }
}
