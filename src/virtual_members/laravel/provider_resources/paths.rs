//! Resolving the file or directory a registration's path argument names.

use std::path::{Path, PathBuf};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::super::expr_walker::{
    beats_best, enclosing_body, walk_before_cursor, walk_file_scope_before_cursor,
};
use super::super::helpers::extract_string_literal;
use super::super::path_helpers::path_helper_base;
use crate::atom::bytes_to_str;
use crate::util::short_name;

/// What a path argument is resolved against: the referring file's directory
/// and the workspace root, plus the parse a variable's assignment is looked
/// up in.
pub(crate) struct PathContext<'a, 'arena> {
    pub(crate) file_dir: &'a Path,
    pub(crate) workspace_root: &'a Path,
    pub(crate) program: &'a Program<'arena>,
}

/// Resolve an expression that names a file to the path it points at.
///
/// Covers the forms Laravel projects use to locate route, config, view, and
/// translation files: `__DIR__ . '/…'` and `dirname(__DIR__) . '/…'`,
/// `base_path('…')` and Laravel's other path helpers, a bare literal
/// (absolute, or relative to the referring file), and a local variable
/// assigned one of those forms earlier in the
/// same scope (Livewire's service provider writes
/// `$config = __DIR__.'/../config/x.php';` before passing `$config` to
/// `mergeConfigFrom`).  `paths.program` is the parse of `content`, which that
/// last form is resolved against.
pub(crate) fn resolve_path_arg(
    expr: &Expression<'_>,
    content: &str,
    paths: &PathContext<'_, '_>,
) -> Option<PathBuf> {
    // `base_path('app/.../web.php')` resolves relative to the workspace
    // root, `resource_path('views/components')` relative to `resources/`
    // inside it, and so on for every path helper.  Each takes an optional
    // argument, and naming the base directory alone is a path in its own
    // right.
    if let Expression::Call(Call::Function(fc)) = expr
        && let Expression::Identifier(id) = fc.function
        && let Some(mut resolved) =
            path_helper_base(paths.workspace_root, short_name(bytes_to_str(id.value())))
        && let Some(val) = match fc.argument_list.arguments.iter().next() {
            Some(first_arg) => {
                extract_string_literal(first_arg.value(), content).map(|(val, _, _)| val)
            }
            None => Some(""),
        }
    {
        let relative = val.trim_start_matches('/');
        if !relative.is_empty() {
            resolved.push(relative);
        }
        return resolved.canonicalize().ok().or(Some(resolved));
    }

    if let Expression::Variable(Variable::Direct(dv)) = expr {
        let assigned = last_assignment_before(paths.program, dv.start_offset(), dv.name)?;
        return resolve_path_arg(assigned, content, paths);
    }

    let resolved = PathBuf::from(crate::document_links::try_evaluate_path_expr(
        expr,
        paths.file_dir,
    )?);
    resolved.canonicalize().ok().or(Some(resolved))
}

/// The RHS of the last `$name = <expr>;` assignment before `offset` in the
/// scope enclosing it: PHP's own resolution rule for a variable read, the
/// most recent write to it in the same scope.
///
/// A service provider assigns inside a method; a route file assigns at the
/// top level of the script, where the enclosing scope is the file itself.
fn last_assignment_before<'ast, 'arena>(
    program: &'ast Program<'arena>,
    offset: u32,
    name: &[u8],
) -> Option<&'ast Expression<'arena>> {
    let mut best: Option<(u32, &'ast Expression<'arena>)> = None;
    let mut record = |node: Node<'ast, 'arena>| {
        let Node::Assignment(assignment) = node else {
            return;
        };
        if !assignment.operator.is_assign() {
            return;
        }
        let Expression::Variable(Variable::Direct(target)) = assignment.lhs else {
            return;
        };
        if target.name != name {
            return;
        }
        let end = node.span().end.offset;
        if beats_best(&best, end, offset) {
            best = Some((end, assignment.rhs));
        }
    };

    match enclosing_body(Node::Program(program), offset) {
        Some(body) => walk_before_cursor(body, offset, &mut record),
        None => walk_file_scope_before_cursor(Node::Program(program), offset, &mut record),
    }
    best.map(|(_, rhs)| rhs)
}
