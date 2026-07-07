use bumpalo::Bump;
use mago_database::file::FileId;
use mago_syntax::ast::*;
use tower_lsp::lsp_types::{Location, Url};

use crate::Backend;
use crate::util::offset_to_position;

use super::helpers::extract_string_literal;

/// Resolve `route('name')` to the `->name('name')` declaration in `routes/`.
///
/// Supports both explicit full-name declarations:
///   `Route::get('/create', ...)->name('admin.email.template.create')`
///
/// And group prefix notation:
///   `Route::name('admin.email.template.')->group(fn() { Route::get(...)->name('create'); })`
pub(crate) fn resolve_route_definitions(backend: &Backend, name: &str) -> Vec<Location> {
    let mut results = Vec::new();
    let snapshot = backend.user_file_symbol_maps();

    for (file_uri, _) in snapshot {
        // Only scan files in the routes/ directory.
        if !file_uri.contains("/routes/") {
            continue;
        }
        let Ok(uri) = Url::parse(&file_uri) else {
            continue;
        };
        let Some(content) = backend.get_file_content(&file_uri) else {
            continue;
        };
        results.extend(scan_route_file(&content, name, &uri));
    }
    results
}

// ─── Route file scanner ──────────────────────────────────────────────────────

fn scan_route_file(content: &str, target: &str, uri: &Url) -> Vec<Location> {
    let arena = Bump::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let mut results = Vec::new();

    for stmt in program.statements.iter() {
        results.extend(scan_stmt(stmt, content, "", target, uri));
    }
    results
}

fn scan_stmt<'a>(
    stmt: &Statement<'a>,
    content: &str,
    prefix: &str,
    target: &str,
    uri: &Url,
) -> Vec<Location> {
    match stmt {
        Statement::Expression(e) => scan_expr(e.expression, content, prefix, target, uri),
        Statement::Return(r) => r
            .value
            .map(|v| scan_expr(v, content, prefix, target, uri))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Walk a call-chain expression while tracking the accumulated group name prefix.
///
/// Handles two forms of group calls:
/// - Fluent chain: `Route::name('prefix.')->middleware(…)->group(fn(){…})`
///   (outermost node is `Call::Method`)
/// - Direct static: `Route::group(['as'=>'prefix.', …], fn(){…})`
///   (outermost node is `Call::StaticMethod`)
fn scan_expr<'a>(
    expr: &Expression<'a>,
    content: &str,
    prefix: &str,
    target: &str,
    uri: &Url,
) -> Vec<Location> {
    match expr {
        // ── Fluent instance-method chain: ->group() / ->name() / other ──────
        Expression::Call(Call::Method(mc)) => {
            let mut results = Vec::new();
            let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                return scan_expr(mc.object, content, prefix, target, uri);
            };
            let method = ident.value.to_ascii_lowercase();

            if method == b"group" {
                let chain_prefix = chain_name_prefix(mc.object, content);
                let new_prefix = format!("{prefix}{chain_prefix}");
                for arg in mc.argument_list.arguments.iter() {
                    results.extend(scan_group_body(
                        arg.value(),
                        content,
                        &new_prefix,
                        target,
                        uri,
                    ));
                }
            } else if method == b"name" {
                if let Some(first_arg) = mc.argument_list.arguments.iter().next()
                    && let Some((name_val, start, _)) =
                        extract_string_literal(first_arg.value(), content)
                {
                    let full = format!("{prefix}{name_val}");
                    if full == target {
                        results.push(crate::definition::point_location(
                            uri.clone(),
                            offset_to_position(content, start),
                        ));
                    }
                }
                results.extend(scan_expr(mc.object, content, prefix, target, uri));
            } else {
                results.extend(scan_expr(mc.object, content, prefix, target, uri));
            }
            results
        }

        // ── Direct static call: Route::group([options,] fn(){…}) ────────────
        //
        // This fires for `Route::group(['as'=>'admin.', …], fn(){…})` where
        // there is no preceding fluent chain to carry the name prefix.
        Expression::Call(Call::StaticMethod(sc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return Vec::new();
            };
            if !ident.value.eq_ignore_ascii_case(b"group") {
                return Vec::new();
            }
            let mut results = Vec::new();
            // Extract name prefix from 'as' => '...' in an array argument.
            let array_prefix = extract_as_prefix_from_args(
                sc.argument_list.arguments.iter().map(|a| a.value()),
                content,
            );
            let new_prefix = format!("{prefix}{array_prefix}");
            for arg in sc.argument_list.arguments.iter() {
                results.extend(scan_group_body(
                    arg.value(),
                    content,
                    &new_prefix,
                    target,
                    uri,
                ));
            }
            results
        }

        _ => Vec::new(),
    }
}

/// Walk the argument that was passed to `->group()`.
fn scan_group_body<'a>(
    expr: &Expression<'a>,
    content: &str,
    prefix: &str,
    target: &str,
    uri: &Url,
) -> Vec<Location> {
    match expr {
        Expression::Closure(closure) => {
            let mut results = Vec::new();
            for stmt in closure.body.statements.iter() {
                results.extend(scan_stmt(stmt, content, prefix, target, uri));
            }
            results
        }
        Expression::ArrowFunction(af) => scan_expr(af.expression, content, prefix, target, uri),
        _ => Vec::new(),
    }
}

pub(crate) use super::helpers::{chain_name_prefix, extract_as_prefix_from_args};
