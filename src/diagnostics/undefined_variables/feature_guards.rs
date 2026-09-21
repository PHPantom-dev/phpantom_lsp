//! Whole-scope feature detection for the undefined/unused-variable
//! diagnostics.
//!
//! These helpers scan a function/method body for constructs that make
//! per-variable static analysis unsound (variable variables, `extract()`)
//! or that reference variables by string name rather than direct use
//! (`compact()`, `get_defined_vars()`). The caller uses the results to
//! bail out of the scope entirely or to treat the named variables as
//! always defined/used.
//!
//! Each detector is a small [`Walker`] visitor: the generated traversal
//! recurses through every node kind, so the visitor only overrides the
//! hooks it cares about. Nested closures, arrow functions, named function
//! declarations, and anonymous-class bodies are their own variable scopes,
//! so the visitors stop at those boundaries (an anonymous class still has
//! its constructor arguments walked, since those live in the enclosing
//! scope).

use std::collections::HashSet;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::scope_collector::ScopeBody;

// ─── Dynamic variable / extract detection ───────────────────────────────────

/// Returns `true` if the body contains variable variables (`$$x` or
/// `${expr}`) anywhere in it (excluding nested scopes).
pub(super) fn has_dynamic_variables(body: ScopeBody<'_, '_>) -> bool {
    let mut found = false;
    body.walk_with(&DynamicVariableWalker, &mut found);
    found
}

struct DynamicVariableWalker;

impl<'ast, 'arena> Walker<'ast, 'arena, bool> for DynamicVariableWalker {
    fn walk_in_indirect_variable(&self, _node: &'ast IndirectVariable<'arena>, context: &mut bool) {
        *context = true;
    }

    fn walk_in_nested_variable(&self, _node: &'ast NestedVariable<'arena>, context: &mut bool) {
        *context = true;
    }

    // A nested/indirect variable in a member, method, or constant name
    // position (`self::$$prop`, `$obj->$$prop`, `$obj->{$name}()`) names a
    // dynamic member, not a dynamic local, so it does not make local
    // variable analysis unsound. Walk only the base expression, skipping
    // the selector.
    fn walk_static_property_access(
        &self,
        node: &'ast StaticPropertyAccess<'arena>,
        context: &mut bool,
    ) {
        self.walk_expression(node.class, context);
    }

    fn walk_class_constant_access(
        &self,
        node: &'ast ClassConstantAccess<'arena>,
        context: &mut bool,
    ) {
        self.walk_expression(node.class, context);
    }

    fn walk_class_like_member_selector(
        &self,
        _node: &'ast ClassLikeMemberSelector<'arena>,
        _context: &mut bool,
    ) {
    }

    stop_at_inner_scopes!(bool);
}

/// Returns `true` if the body contains a call to `extract()`.
pub(super) fn has_extract_call(body: ScopeBody<'_, '_>) -> bool {
    has_named_call(body, b"extract")
}

/// Whether the body calls the function `name` anywhere outside a nested
/// scope.
fn has_named_call(body: ScopeBody<'_, '_>, name: &'static [u8]) -> bool {
    let mut found = false;
    body.walk_with(&NamedCallWalker(name), &mut found);
    found
}

struct NamedCallWalker(&'static [u8]);

impl<'ast, 'arena> Walker<'ast, 'arena, bool> for NamedCallWalker {
    fn walk_in_function_call(&self, node: &'ast FunctionCall<'arena>, context: &mut bool) {
        if is_named_call(node, self.0) {
            *context = true;
        }
    }

    stop_at_inner_scopes!(bool);
}

// ─── compact() variable collection ──────────────────────────────────────────

/// Collect variable names referenced by `compact('var1', 'var2', …)`
/// calls.  These variables are used by string name and should be
/// considered defined.
pub(crate) fn collect_compact_vars(body: ScopeBody<'_, '_>) -> HashSet<String> {
    let mut vars = HashSet::new();
    body.walk_with(&CompactWalker, &mut vars);
    vars
}

struct CompactWalker;

impl<'ast, 'arena> Walker<'ast, 'arena, HashSet<String>> for CompactWalker {
    fn walk_in_function_call(
        &self,
        node: &'ast FunctionCall<'arena>,
        context: &mut HashSet<String>,
    ) {
        if is_named_call(node, b"compact") {
            // Each argument is a variable name (string literal) or an
            // array of names (possibly nested), matching the forms
            // compact() accepts.
            for arg in node.argument_list.arguments.iter() {
                collect_compact_name_from_arg(arg.value(), context);
            }
        }
    }

    stop_at_inner_scopes!(HashSet<String>);
}

/// Collect variable names from a single `compact()` argument. A string
/// literal names a variable directly; an array literal is descended
/// into recursively so `compact(['a', ['b']])` collects both names.
fn collect_compact_name_from_arg(expr: &Expression<'_>, vars: &mut HashSet<String>) {
    match expr {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the interpreted string content (without
            // quotes); fall back to `raw` and strip quotes manually
            // if `value` is `None`.
            let name: &str = if let Some(v) = s.value {
                // A value that is not UTF-8 (`compact("\x8b")`) cannot name
                // a variable, so there is nothing to collect.
                let Some(v) = crate::atom::literal_bytes_to_str(v) else {
                    return;
                };
                v
            } else {
                let raw = crate::atom::bytes_to_str(s.raw);
                raw.strip_prefix('\'')
                    .or_else(|| raw.strip_prefix('"'))
                    .and_then(|inner| inner.strip_suffix('\'').or_else(|| inner.strip_suffix('"')))
                    .unwrap_or(raw)
            };
            if !name.is_empty() {
                vars.insert(format!("${}", name));
            }
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                collect_compact_name_from_elem(elem, vars);
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                collect_compact_name_from_elem(elem, vars);
            }
        }
        _ => {}
    }
}

/// Collect variable names from one element of an array passed to
/// `compact()`. Keys are ignored; values are names or nested arrays.
fn collect_compact_name_from_elem(elem: &ArrayElement<'_>, vars: &mut HashSet<String>) {
    match elem {
        ArrayElement::KeyValue(kv) => collect_compact_name_from_arg(kv.value, vars),
        ArrayElement::Value(v) => collect_compact_name_from_arg(v.value, vars),
        ArrayElement::Variadic(s) => collect_compact_name_from_arg(s.value, vars),
        ArrayElement::Missing(_) => {}
    }
}

// ─── get_defined_vars() detection ───────────────────────────────────────────

/// Returns true if the body contains a call to `get_defined_vars()`.
/// When present in a scope, all variables defined in that scope are
/// considered used (e.g. for debug dumps), so unused-variable diagnostics
/// should be suppressed for them.
pub(crate) fn has_get_defined_vars(body: ScopeBody<'_, '_>) -> bool {
    has_named_call(body, b"get_defined_vars")
}

// ─── include / require detection ────────────────────────────────────────────

/// Collect the start offset of every `include`/`include_once`/`require`/
/// `require_once` construct in the body, nested scopes included.
///
/// The included file's code runs in the *including* scope, so it can read
/// any local there by name — a known idiom for handing a variable to
/// dynamically included code (`(function () use ($container) { require
/// $file; })()`). Since the target is resolved at runtime, no analysis can
/// tell which names it reads, so the whole scope's variables have to count
/// as used.
///
/// Offsets are collected rather than a single flag because the caller
/// needs to know which frame's scope each import sits in, and unlike the
/// other detectors here that means walking into nested scopes.
pub(crate) fn collect_import_offsets(body: ScopeBody<'_, '_>) -> Vec<u32> {
    let mut offsets = Vec::new();
    body.walk_with(&ImportWalker, &mut offsets);
    offsets
}

struct ImportWalker;

impl<'ast, 'arena> Walker<'ast, 'arena, Vec<u32>> for ImportWalker {
    fn walk_in_construct(&self, node: &'ast Construct<'arena>, context: &mut Vec<u32>) {
        if node.is_import() {
            context.push(node.span().start.offset);
        }
    }
}

// ─── Shared helpers ─────────────────────────────────────────────────────────

/// Returns `true` if `call` is an unqualified call to a global function
/// whose name matches `name` (case-insensitively).
fn is_named_call(call: &FunctionCall<'_>, name: &[u8]) -> bool {
    matches!(call.function, Expression::Identifier(ident) if ident.value().eq_ignore_ascii_case(name))
}
