//! Laravel Folio page-route discovery.
//!
//! [Folio](https://laravel.com/docs/folio) derives routes from the
//! filesystem: a Blade page under a mounted directory becomes a route, and
//! `Laravel\Folio\name()` inside the page gives it a name.  Nothing calls
//! `Route::` for these, so the conventional scanner in [`super::route_names`]
//! never sees them — this module bridges that gap, and its results are
//! folded into [`super::route_names::enumerate_all_routes`] and
//! [`super::route_names::resolve_route_definitions`] so every existing route
//! consumer (completion, hover, diagnostics, go-to-definition,
//! find-references) picks up Folio pages for free.
//!
//! A mount is registered one of three ways, all recovered statically:
//!
//! - `Application::configure(...)->withRouting(pages: '...', ...)` in
//!   `bootstrap/app.php` (`php artisan folio:install`'s default), which the
//!   framework forwards to `Folio::route()` internally.
//! - An explicit `Folio::route(...)` call, written by hand in
//!   `bootstrap/app.php` or a service provider.
//! - `Folio::path(...)->middleware([...])->uri('...')->name('...')`, for
//!   additional mounts (multi-tenant apps, admin sections), conventionally
//!   registered from a service provider's `boot()`.
//!
//! When no explicit registration is found, a project that otherwise looks
//! like it uses Folio (`composer.json` requires `laravel/folio`, or the
//! package is installed) falls back to the framework's own default mount,
//! `resources/views/pages`.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::{Location, Url};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;
use crate::text_position::offset_to_position;

use super::helpers::{
    chain_name_prefix, chain_uri_modifier, extract_string_literal, join_uri_segments,
};
use super::provider_resources::resolve_path_arg;
use super::route_names::{RouteEntry, for_each_nested_statement, route_name_matches};

/// The FQN of Folio's page-naming helper, as `mago-names` resolves it.
const FOLIO_NAME_FUNCTION: &str = "Laravel\\Folio\\name";

/// Folio's default page directory, relative to the workspace root.  Used
/// when a project uses Folio but registers no mount of its own — the shape
/// `php artisan folio:install` scaffolds.
const DEFAULT_FOLIO_MOUNT: &str = "resources/views/pages";

/// A discovered Folio mount: one page directory, plus the URI and
/// route-name prefixes every page beneath it inherits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FolioMount {
    pub directory: PathBuf,
    /// URI prefix from a chained `->uri('...')` (no surrounding slashes).
    /// Empty for the default mount and for `Folio::route()`/`withRouting()`
    /// registrations, neither of which support one.
    pub uri_prefix: String,
    /// Route-name prefix from a chained `->name('...')`.  Empty when the
    /// mount sets none.
    pub name_prefix: String,
}

// ─── Mount discovery ─────────────────────────────────────────────────────

/// Discover every Folio mount the project registers, falling back to the
/// framework default when none are found but the project appears to use
/// Folio.
pub(crate) fn discover_folio_mounts(backend: &Backend) -> Vec<FolioMount> {
    let Some(workspace_root) = backend.workspace.workspace_root.read().clone() else {
        return Vec::new();
    };

    let mut mounts: Vec<FolioMount> = backend
        .laravel_provider_resources
        .read()
        .folio_mounts
        .clone();

    // `withRouting(pages: ...)` / `Folio::route(...)` conventionally live in
    // `bootstrap/app.php`, which is not a service provider and so is never
    // reached by the provider scan that populates `folio_mounts` above.
    let bootstrap_path = workspace_root.join("bootstrap/app.php");
    if let Ok(content) = std::fs::read_to_string(&bootstrap_path) {
        let arena = LocalArena::new();
        let file_id = FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
        let file_dir = bootstrap_path.parent().unwrap_or(&workspace_root);
        mounts.extend(scan_folio_mounts_in_program(
            program,
            &content,
            file_dir,
            &workspace_root,
        ));
    }

    if mounts.is_empty() && folio_in_use(&workspace_root) {
        let default_dir = workspace_root.join(DEFAULT_FOLIO_MOUNT);
        if default_dir.is_dir() {
            mounts.push(FolioMount {
                directory: default_dir,
                uri_prefix: String::new(),
                name_prefix: String::new(),
            });
        }
    }

    mounts
}

/// Whether the project looks like it uses Folio at all, independent of
/// whether an explicit mount was found — gates the default-mount fallback so
/// a project that merely happens to have a `resources/views/pages`
/// directory is left alone.
fn folio_in_use(workspace_root: &Path) -> bool {
    if let Ok(composer) = std::fs::read_to_string(workspace_root.join("composer.json"))
        && composer.contains("laravel/folio")
    {
        return true;
    }
    workspace_root.join("vendor/laravel/folio").is_dir()
}

/// Scan one already-parsed program for every Folio mount registration it
/// makes: `Folio::path(...)`/`Folio::route(...)` chains and
/// `->withRouting(pages: ...)` calls.
///
/// Used both for service-provider files (via [`extract_provider_resources`]
/// (super::provider_resources::extract_provider_resources)) and for
/// `bootstrap/app.php`, which is scanned directly since it registers no
/// provider of its own.
pub(crate) fn scan_folio_mounts_in_program(
    program: &Program<'_>,
    content: &str,
    file_dir: &Path,
    workspace_root: &Path,
) -> Vec<FolioMount> {
    let mut mounts = Vec::new();
    for stmt in program.statements.iter() {
        collect_folio_path_mounts(
            stmt,
            content,
            file_dir,
            workspace_root,
            program,
            &mut mounts,
        );
    }
    super::helpers::walk_program_expressions(program, &mut |expr| {
        if let Some(mount) =
            with_routing_pages_mount(expr, content, file_dir, workspace_root, program)
        {
            mounts.push(mount);
        }
        ControlFlow::Continue(())
    });
    mounts
}

/// Walk every statement (including nested ones, inside `if`/method bodies/…)
/// looking for a top-level `Folio::path(...)`/`Folio::route(...)` chain.
///
/// Matched at the *statement* level rather than per-expression-node: unlike
/// `Route::…->group()`, a Folio mount chain has no fixed terminal method
/// (`->uri()`, `->name()`, `->middleware()` can all be omitted or reordered),
/// so matching every node whose root is `Folio::path(...)` would report the
/// same mount once per chain link.  A mount registration is always written
/// as its own statement, so checking each statement's expression exactly
/// once is both sufficient and free of duplicates.
fn collect_folio_path_mounts(
    stmt: &Statement<'_>,
    content: &str,
    file_dir: &Path,
    workspace_root: &Path,
    program: &Program<'_>,
    out: &mut Vec<FolioMount>,
) {
    if let Statement::Expression(e) = stmt
        && let Some(mount) =
            folio_path_mount(e.expression, content, file_dir, workspace_root, program)
    {
        out.push(mount);
    }
    for_each_nested_statement(stmt, &mut |nested| {
        collect_folio_path_mounts(nested, content, file_dir, workspace_root, program, out);
    });
}

/// If `expr` is a `Folio::path(...)`/`Folio::route(...)` chain, the mount it
/// registers.
fn folio_path_mount(
    expr: &Expression<'_>,
    content: &str,
    file_dir: &Path,
    workspace_root: &Path,
    program: &Program<'_>,
) -> Option<FolioMount> {
    let args = folio_mount_root_args(expr, content)?;
    let path_arg = args.arguments.iter().next()?;
    let directory = resolve_path_arg(path_arg.value(), content, file_dir, workspace_root, program)?;
    Some(FolioMount {
        directory,
        uri_prefix: chain_uri_modifier(expr, content),
        name_prefix: chain_name_prefix(expr, content),
    })
}

/// If `expr`'s call chain roots at `Folio::path(...)` or `Folio::route(...)`,
/// that root call's arguments.
fn folio_mount_root_args<'e, 'a>(
    expr: &'e Expression<'a>,
    content: &str,
) -> Option<&'e ArgumentList<'a>> {
    match expr {
        Expression::Call(Call::Method(mc)) => folio_mount_root_args(mc.object, content),
        Expression::Call(Call::StaticMethod(sc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return None;
            };
            if !is_folio_facade(sc.class, content) {
                return None;
            }
            (ident.value.eq_ignore_ascii_case(b"path")
                || ident.value.eq_ignore_ascii_case(b"route"))
            .then_some(&sc.argument_list)
        }
        _ => None,
    }
}

/// Whether `class` names the `Folio` facade, bare or fully qualified.
fn is_folio_facade(class: &Expression<'_>, content: &str) -> bool {
    let span = class.span();
    let Some(text) = content.get(span.start.offset as usize..span.end.offset as usize) else {
        return false;
    };
    let text = text.trim_start_matches('\\');
    text.eq_ignore_ascii_case("Folio") || text.eq_ignore_ascii_case("Laravel\\Folio\\Folio")
}

/// If `expr` is a `->withRouting(...)` call naming a `pages:` argument, the
/// mount it registers.
///
/// Unlike `Folio::path(...)`, this needs no statement-level dedup: the mount
/// is fully described by this one call's own arguments, not by chain links
/// around it, so it is safe to match from the generic per-expression walker.
fn with_routing_pages_mount(
    expr: &Expression<'_>,
    content: &str,
    file_dir: &Path,
    workspace_root: &Path,
    program: &Program<'_>,
) -> Option<FolioMount> {
    let Expression::Call(Call::Method(mc)) = expr else {
        return None;
    };
    let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
        return None;
    };
    if !ident.value.eq_ignore_ascii_case(b"withRouting") {
        return None;
    }
    let pages_arg = mc
        .argument_list
        .arguments
        .iter()
        .find_map(|arg| match arg {
            Argument::Named(named)
                if bytes_to_str(named.name.value).eq_ignore_ascii_case("pages") =>
            {
                Some(named.value)
            }
            _ => None,
        })?;
    let directory = resolve_path_arg(pages_arg, content, file_dir, workspace_root, program)?;
    Some(FolioMount {
        directory,
        uri_prefix: String::new(),
        name_prefix: String::new(),
    })
}

// ─── Page walking ────────────────────────────────────────────────────────

/// Every named route a project's Folio pages register.
///
/// Unnamed pages have a URI but no `route()`-reachable name, so they
/// contribute nothing here — matching how a conventional route with no
/// `->name()` is invisible to `route()` too.
pub(crate) fn enumerate_folio_routes(backend: &Backend) -> Vec<RouteEntry> {
    let mut routes = Vec::new();
    for mount in discover_folio_mounts(backend) {
        let mut pages = Vec::new();
        collect_page_files(&mount.directory, &mount.directory, &mut pages);
        for (relative, path) in pages {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some((page_name, _offset)) = extract_page_route_name(&content) else {
                continue;
            };
            routes.push(RouteEntry {
                name: compose_folio_name(&mount.name_prefix, &page_name),
                uri: compose_folio_uri(&mount.uri_prefix, &derive_folio_uri(&relative)),
                from_vendor: false,
            });
        }
    }
    routes
}

/// Resolve `name` to the Folio page that declares it, if any.
pub(crate) fn resolve_folio_route_definition(backend: &Backend, name: &str) -> Vec<Location> {
    let mut results = Vec::new();
    for mount in discover_folio_mounts(backend) {
        let mut pages = Vec::new();
        collect_page_files(&mount.directory, &mount.directory, &mut pages);
        for (_relative, path) in pages {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some((page_name, offset)) = extract_page_route_name(&content) else {
                continue;
            };
            let full_name = compose_folio_name(&mount.name_prefix, &page_name);
            if !route_name_matches(name, &full_name) {
                continue;
            }
            let Ok(uri) = Url::from_file_path(&path) else {
                continue;
            };
            results.push(crate::definition::point_location(
                uri,
                offset_to_position(&content, offset),
            ));
        }
    }
    results
}

/// Recursively record every `.blade.php` file under `dir` as
/// `(path relative to base, absolute path)`.
fn collect_page_files(base: &Path, dir: &Path, out: &mut Vec<(PathBuf, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Recurse only into real directories: following a link back up the
        // tree would re-enter it until the kernel's symlink limit stops the
        // walk.  A linked page is still recorded, through the link.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_page_files(base, &path, out);
            continue;
        }
        let is_page = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".blade.php"));
        if !is_page {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(base) {
            out.push((relative.to_path_buf(), path.clone()));
        }
    }
}

/// Derive a Folio route URI from a page path relative to its mount
/// directory (e.g. `users/[User].blade.php`).
///
/// Drops the `.blade.php` suffix and a trailing `index` segment, rewrites
/// `[id]` to `{id}`, `[...slug]` to `{slug}`, and implicit model-binding
/// segments (`[User]`, `[Post:slug]`) to `{user}`/`{post}`.  Returns the
/// path segments without a leading slash; the empty string denotes the
/// mount root (composed to `/` by [`compose_folio_uri`]).
fn derive_folio_uri(relative: &Path) -> String {
    let rel = relative.to_string_lossy().replace('\\', "/");
    let stem = rel.strip_suffix(".blade.php").unwrap_or(&rel);
    let raw: Vec<&str> = stem.split('/').filter(|s| !s.is_empty()).collect();
    let last = raw.len().saturating_sub(1);
    let mut segments = Vec::new();
    for (i, seg) in raw.iter().enumerate() {
        if *seg == "index" && i == last {
            continue;
        }
        segments.push(rewrite_folio_segment(seg));
    }
    segments.join("/")
}

/// Rewrite one filename segment's Folio placeholder syntax into a route
/// parameter.  A literal segment passes through untouched.
fn rewrite_folio_segment(segment: &str) -> String {
    let Some(inner) = segment.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return segment.to_string();
    };
    let inner = inner.strip_prefix("...").unwrap_or(inner);
    // `[Post:slug]` binds by a custom key; the URI segment name is still the
    // (lowercased) model name, exactly as `[Post]` alone would produce.
    let name = inner.split_once(':').map_or(inner, |(model, _)| model);
    format!("{{{}}}", name.to_ascii_lowercase())
}

/// Join a mount's URI prefix and a page's derived URI, the same way route
/// group prefixes are joined, collapsing the empty (mount-root) case to `/`
/// per [`RouteEntry`]'s `uri` field convention.
fn compose_folio_uri(prefix: &str, derived: &str) -> String {
    let joined = join_uri_segments(prefix, derived);
    if joined.is_empty() {
        "/".to_string()
    } else {
        joined
    }
}

/// Combine a mount's name prefix with a page's own name, the same way a
/// route group's name prefix combines with the routes inside it.
fn compose_folio_name(prefix: &str, page_name: &str) -> String {
    format!("{prefix}{page_name}")
}

/// Extract a page's own Folio route name — the argument of its `name(...)`
/// call — and the byte offset the name's content starts at, for
/// go-to-definition.
///
/// Only a call resolving to `Laravel\Folio\name` counts, which requires
/// `use function Laravel\Folio\name;` (bare or grouped) or a fully qualified
/// `\Laravel\Folio\name(...)` call — the same import-gated recognition
/// `route()`/`view()` calls get from the symbol map, applied here by hand
/// since a Folio page's own declaration is found by this standalone scan
/// rather than through symbol-map extraction (see [`super::route_names`]'s
/// module docs: route *declarations* are never symbol-map spans).
fn extract_page_route_name(content: &str) -> Option<(String, usize)> {
    // Cheap pre-filter: every page that names a route mentions the
    // namespace segment somewhere, whether through the `use function`
    // import or a fully qualified call.
    memchr::memmem::find(content.as_bytes(), b"Folio")?;

    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = OwnedResolvedNames::from_resolved(&NameResolver::new(&arena).resolve(program));

    let mut found = None;
    super::helpers::walk_program_expressions(program, &mut |expr| {
        let Expression::Call(Call::Function(fc)) = expr else {
            return ControlFlow::Continue(());
        };
        let Expression::Identifier(ident) = fc.function else {
            return ControlFlow::Continue(());
        };
        let is_folio_name = resolved.get(ident.span().start.offset).is_some_and(|fqn| {
            fqn.trim_start_matches('\\')
                .eq_ignore_ascii_case(FOLIO_NAME_FUNCTION)
        });
        if !is_folio_name {
            return ControlFlow::Continue(());
        }
        if let Some(arg) = fc.argument_list.arguments.iter().next()
            && let Some((value, start, _)) = extract_string_literal(arg.value(), content)
            && !value.is_empty()
        {
            found = Some((value.to_string(), start));
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    found
}

#[cfg(test)]
#[path = "folio_tests.rs"]
mod tests;
