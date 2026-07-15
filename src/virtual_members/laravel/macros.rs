//! Best-effort static scan of Laravel `Target::macro('name', closure)`
//! registrations.
//!
//! Laravel's `Illuminate\Support\Traits\Macroable` trait lets any class
//! register new methods at runtime via `SomeClass::macro('name', $closure)`,
//! typically from a service provider's `boot()`.  Full runtime fidelity is
//! not achievable statically (Larastan boots the app and reads the runtime
//! `static::$macros` via reflection), but the common literal registration
//! pattern is recoverable from source.
//!
//! This module extracts registrations of the shape
//! `Target::macro('name', function (...) {...})` /
//! `Target::macro('name', fn (...) => ...)` where `Target` resolves via the
//! file's `use` statements to a class name.  The macro name, closure
//! parameters, and closure return type become a synthesized method that is
//! later injected onto the target's `ClassInfo` (see
//! [`crate::Backend::inject_laravel_macros`]).  Registrations whose name or
//! closure is not a literal (variable/computed targets, string/array
//! callables, `Macroable::mixin()`) are skipped; those keep falling through
//! to the concrete class's `__call`, which is the current gracefully
//! degraded behaviour.

use std::collections::HashMap;
use std::sync::Arc;

use bumpalo::Bump;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::ast::*;
use mago_syntax::parser::parse_file_content;

use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;
use crate::types::{ClassInfo, MethodInfo, PhpVersion};

/// A single `Target::macro('name', closure)` registration recovered from
/// source.
pub(crate) struct MacroRegistration {
    /// FQN of the class written before `::macro`, resolved via the file's
    /// `use` statements.  This may be a `Macroable` class (the macro attaches
    /// to it directly) or a facade (the caller resolves it to the facade's
    /// root class before injecting).
    pub target: String,
    /// The synthesized method (name + parameters + return type).  Callers
    /// inject both a static and an instance variant so that `Str::slug()` and
    /// `$collection->macro()` both resolve.
    pub method: MethodInfo,
}

/// Extract every literal macro registration from a file's source.
///
/// Returns an empty vector when the file contains no `macro(` substring
/// (a cheap byte pre-filter) so the parse is only paid for candidate files.
pub(crate) fn extract_macro_registrations(
    content: &str,
    php_version: Option<PhpVersion>,
) -> Vec<MacroRegistration> {
    // Byte pre-filter: every registration contains the `macro(` call token.
    if memchr::memmem::find(content.as_bytes(), b"macro(").is_none() {
        return Vec::new();
    }

    let arena = Bump::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);

    let mut calls: Vec<&StaticMethodCall<'_>> = Vec::new();
    collect_macro_calls(Node::Program(program), &mut calls);

    let mut out = Vec::new();
    for call in calls {
        if let Some(reg) = build_registration(call, &owned, content, php_version) {
            out.push(reg);
        }
    }
    out
}

/// Project-wide index of Laravel macro registrations, keyed by the FQN of
/// the class each macro attaches to.
///
/// Stored on [`Backend`](crate::Backend) and built for Laravel projects after
/// indexing.  `by_uri` is the source of truth (one entry per contributing
/// file, so an edit to a file can replace just that file's registrations);
/// `merged` is the derived lookup map used when injecting members onto a
/// loaded class.  Each macro is stored as both a static and an instance
/// method so that `Str::slug()` and `$collection->macro()` both resolve.
#[derive(Default)]
pub(crate) struct LaravelMacroIndex {
    by_uri: HashMap<String, Vec<MacroRegistration>>,
    merged: HashMap<String, Vec<Arc<MethodInfo>>>,
}

impl LaravelMacroIndex {
    /// Replace the registrations contributed by `uri`.  Passing an empty
    /// vector removes the file's contributions.  Call [`Self::rebuild`]
    /// afterwards to refresh the merged lookup map (deferred so a bulk build
    /// rebuilds once rather than per file).
    pub(crate) fn set_file(&mut self, uri: String, regs: Vec<MacroRegistration>) {
        if regs.is_empty() {
            self.by_uri.remove(&uri);
        } else {
            self.by_uri.insert(uri, regs);
        }
    }

    /// Rebuild the merged lookup map from the per-file registrations.
    pub(crate) fn rebuild(&mut self) {
        self.rebuild_merged();
    }

    /// Whether `uri` currently contributes any registrations.
    pub(crate) fn has_uri(&self, uri: &str) -> bool {
        self.by_uri.contains_key(uri)
    }

    /// Whether the merged map has no macros at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.merged.is_empty()
    }

    /// The macro methods that attach to `fqn`, if any.
    pub(crate) fn get(&self, fqn: &str) -> Option<&[Arc<MethodInfo>]> {
        self.merged.get(fqn).map(Vec::as_slice)
    }

    /// Every class FQN that has at least one macro (used to evict stale
    /// resolved-class cache entries when the index changes).
    pub(crate) fn target_fqns(&self) -> Vec<String> {
        self.merged.keys().cloned().collect()
    }

    /// Rebuild `merged` from `by_uri`.  For each registration the macro is
    /// added as both a static and an instance method; duplicates
    /// (same name + staticness on the same target) keep the first seen.
    fn rebuild_merged(&mut self) {
        let mut merged: HashMap<String, Vec<Arc<MethodInfo>>> = HashMap::new();
        for regs in self.by_uri.values() {
            for reg in regs {
                let bucket = merged.entry(reg.target.clone()).or_default();
                for is_static in [false, true] {
                    let exists = bucket
                        .iter()
                        .any(|m| m.name == reg.method.name && m.is_static == is_static);
                    if exists {
                        continue;
                    }
                    let mut method = reg.method.clone();
                    method.is_static = is_static;
                    bucket.push(Arc::new(method));
                }
            }
        }
        self.merged = merged;
    }
}

/// Add the macro methods registered on `class` (by FQN), returning a new
/// `Arc` when any were added and the original otherwise.
///
/// Macro methods are added only when no real method of the same name and
/// staticness already exists, so a genuine declaration always wins.
pub(crate) fn inject_macros(index: &LaravelMacroIndex, class: Arc<ClassInfo>) -> Arc<ClassInfo> {
    let Some(macros) = index.get(class.fqn().as_str()) else {
        return class;
    };

    let to_add: Vec<Arc<MethodInfo>> = macros
        .iter()
        .filter(|m| {
            !class
                .methods
                .iter()
                .any(|existing| existing.name == m.name && existing.is_static == m.is_static)
        })
        .cloned()
        .collect();

    if to_add.is_empty() {
        return class;
    }

    let mut cloned = ClassInfo::clone(&class);
    for method in to_add {
        cloned.methods.push(method);
    }
    cloned.rebuild_method_index();
    Arc::new(cloned)
}

/// Recursively collect every `X::macro(...)` static-method-call node.
fn collect_macro_calls<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    out: &mut Vec<&'ast StaticMethodCall<'arena>>,
) {
    if let Node::StaticMethodCall(smc) = node
        && let ClassLikeMemberSelector::Identifier(ident) = &smc.method
        && bytes_to_str(ident.value).eq_ignore_ascii_case("macro")
    {
        out.push(smc);
    }
    node.visit_children(|child| collect_macro_calls(child, out));
}

/// Build a [`MacroRegistration`] from a `Target::macro('name', closure)` call,
/// or `None` when the call does not match the supported literal shape.
fn build_registration(
    smc: &StaticMethodCall<'_>,
    resolved: &OwnedResolvedNames,
    content: &str,
    php_version: Option<PhpVersion>,
) -> Option<MacroRegistration> {
    let target = resolve_target_fqn(smc.class, resolved)?;

    let mut args = smc.argument_list.arguments.iter();
    let name = macro_name(args.next()?.value())?;
    let (parameter_list, return_type_hint) = closure_signature(args.next()?.value())?;

    let parameters =
        crate::parser::extract_parameters(parameter_list, Some(content), php_version, None);
    let return_type = return_type_hint.map(|rth| crate::parser::extract_hint_type(&rth.hint));

    let mut method = MethodInfo::virtual_method_typed(&name, return_type.as_ref());
    method.parameters = parameters;
    method.native_return_type = return_type;

    Some(MacroRegistration { target, method })
}

/// Resolve the class written before `::macro` to a fully-qualified name via
/// the file's resolved `use` statements.  `self`/`static`/`parent` are
/// skipped (a relative target carries no concrete FQN here).
fn resolve_target_fqn(class: &Expression<'_>, resolved: &OwnedResolvedNames) -> Option<String> {
    let Expression::Identifier(ident) = class else {
        return None;
    };
    let raw = bytes_to_str(ident.value());
    if matches!(
        raw.to_ascii_lowercase().as_str(),
        "self" | "static" | "parent"
    ) {
        return None;
    }
    let offset = ident.span().start.offset;
    if let Some(fqn) = resolved.get(offset) {
        return Some(fqn.trim_start_matches('\\').to_string());
    }
    (!raw.is_empty()).then(|| raw.trim_start_matches('\\').to_string())
}

/// Extract the string value of the macro-name argument.
fn macro_name(expr: &Expression<'_>) -> Option<String> {
    if let Expression::Literal(Literal::String(s)) = expr
        && let Some(v) = s.value
    {
        let name = bytes_to_str(v);
        // Macro names are valid PHP identifiers; reject anything else
        // (interpolated or dynamic strings) so we never synthesize garbage.
        if !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            return Some(name.to_string());
        }
    }
    None
}

/// Extract the parameter list and return-type hint of the closure/arrow-fn
/// argument to `macro()`.
fn closure_signature<'ast, 'arena>(
    expr: &'ast Expression<'arena>,
) -> Option<(
    &'ast FunctionLikeParameterList<'arena>,
    Option<&'ast FunctionLikeReturnTypeHint<'arena>>,
)> {
    match expr {
        Expression::Closure(c) => Some((&c.parameter_list, c.return_type_hint.as_ref())),
        Expression::ArrowFunction(a) => Some((&a.parameter_list, a.return_type_hint.as_ref())),
        _ => None,
    }
}

#[cfg(test)]
#[path = "macros_tests.rs"]
mod tests;
