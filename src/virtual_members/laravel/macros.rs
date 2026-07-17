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

use std::collections::{HashMap, HashSet};
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
#[derive(Clone)]
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
    /// Byte offset of the macro-name string literal (the `'name'` argument) in
    /// the file the registration was found in.  Go-to-definition on a macro
    /// call jumps here, since the synthesized method has no declaration in the
    /// target class's own file.
    pub name_offset: u32,
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
    collect_instance_macro_registrations(
        Node::Program(program),
        &owned,
        content,
        php_version,
        &mut out,
    );
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
    /// Source location of each macro's `::macro('name', ...)` registration,
    /// keyed by `(target FQN, macro name)`.  Powers go-to-definition, which
    /// jumps to the registration call site rather than the target class's own
    /// file (where the macro has no declaration).
    locations: HashMap<(String, String), (String, u32)>,
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

    /// The source location (file URI + byte offset of the name literal) of the
    /// `::macro('name', ...)` registration for `name` on `fqn`, if known.
    pub(crate) fn definition(&self, fqn: &str, name: &str) -> Option<(&str, u32)> {
        self.locations
            .get(&(fqn.to_string(), name.to_string()))
            .map(|(uri, offset)| (uri.as_str(), *offset))
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
        let mut locations: HashMap<(String, String), (String, u32)> = HashMap::new();
        for (uri, regs) in self.by_uri.iter() {
            for reg in regs {
                let bucket = merged.entry(reg.target.clone()).or_default();
                let mut added = false;
                for is_static in [false, true] {
                    let exists = bucket
                        .iter()
                        .any(|m| m.name == reg.method.name && m.is_static == is_static);
                    if exists {
                        continue;
                    }
                    added = true;
                    let mut method = reg.method.clone();
                    method.is_static = is_static;
                    bucket.push(Arc::new(method));
                }
                // First registration for a (target, name) wins its location so
                // it stays consistent with the first-wins merge above.
                if added {
                    locations
                        .entry((reg.target.clone(), reg.method.name.to_string()))
                        .or_insert_with(|| (uri.clone(), reg.name_offset));
                }
            }
        }
        self.merged = merged;
        self.locations = locations;
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

fn collect_instance_macro_registrations(
    node: Node<'_, '_>,
    resolved: &OwnedResolvedNames,
    content: &str,
    php_version: Option<PhpVersion>,
    out: &mut Vec<MacroRegistration>,
) {
    use mago_syntax::ast::class_like::member::ClassLikeMember;
    use mago_syntax::ast::class_like::method::MethodBody;

    match node {
        Node::Program(program) => {
            for statement in program.statements.iter() {
                collect_instance_macro_registrations(
                    Node::Statement(statement),
                    resolved,
                    content,
                    php_version,
                    out,
                );
            }
        }
        Node::Statement(Statement::Namespace(namespace)) => {
            for statement in namespace.statements().iter() {
                collect_instance_macro_registrations(
                    Node::Statement(statement),
                    resolved,
                    content,
                    php_version,
                    out,
                );
            }
        }
        Node::Statement(Statement::Function(function)) => collect_instance_macros_in_body(
            Node::Block(&function.body),
            &typed_parameter_targets(&function.parameter_list, resolved),
            content,
            php_version,
            out,
        ),
        Node::Class(class) => {
            for member in class.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(body) = &method.body
                {
                    collect_instance_macros_in_body(
                        Node::Block(body),
                        &typed_parameter_targets(&method.parameter_list, resolved),
                        content,
                        php_version,
                        out,
                    );
                }
            }
        }
        Node::Trait(trait_) => {
            for member in trait_.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(body) = &method.body
                {
                    collect_instance_macros_in_body(
                        Node::Block(body),
                        &typed_parameter_targets(&method.parameter_list, resolved),
                        content,
                        php_version,
                        out,
                    );
                }
            }
        }
        Node::Enum(enum_) => {
            for member in enum_.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(body) = &method.body
                {
                    collect_instance_macros_in_body(
                        Node::Block(body),
                        &typed_parameter_targets(&method.parameter_list, resolved),
                        content,
                        php_version,
                        out,
                    );
                }
            }
        }
        _ => node.visit_children(|child| {
            collect_instance_macro_registrations(child, resolved, content, php_version, out)
        }),
    }
}

fn collect_instance_macros_in_body(
    node: Node<'_, '_>,
    typed_targets: &HashMap<String, String>,
    content: &str,
    php_version: Option<PhpVersion>,
    out: &mut Vec<MacroRegistration>,
) {
    if let Node::MethodCall(call) = node
        && let ClassLikeMemberSelector::Identifier(ident) = &call.method
        && bytes_to_str(ident.value).eq_ignore_ascii_case("macro")
        && let Expression::Variable(Variable::Direct(dv)) = call.object
        && let Some(target) = typed_targets.get(bytes_to_str(dv.name))
        && let Some(reg) = build_instance_registration(call, target, content, php_version)
    {
        out.push(reg);
    }
    node.visit_children(|child| {
        collect_instance_macros_in_body(child, typed_targets, content, php_version, out)
    });
}

fn typed_parameter_targets(
    params: &FunctionLikeParameterList<'_>,
    resolved: &OwnedResolvedNames,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for param in params.parameters.iter() {
        let Some(hint) = param.hint.as_ref() else {
            continue;
        };
        let Some(target) = resolve_hint_target_fqn(hint, resolved) else {
            continue;
        };
        out.insert(bytes_to_str(param.variable.name).to_string(), target);
    }
    out
}

fn resolve_hint_target_fqn(hint: &Hint<'_>, resolved: &OwnedResolvedNames) -> Option<String> {
    match hint {
        Hint::Identifier(ident) => {
            let raw = bytes_to_str(ident.value());
            if matches!(
                raw.to_ascii_lowercase().as_str(),
                "self" | "static" | "parent"
            ) {
                return None;
            }
            let offset = ident.span().start.offset;
            resolved
                .get(offset)
                .map(|fqn| fqn.trim_start_matches('\\').to_string())
                .or_else(|| (!raw.is_empty()).then(|| raw.trim_start_matches('\\').to_string()))
        }
        Hint::Nullable(nullable) => resolve_hint_target_fqn(nullable.hint, resolved),
        Hint::Parenthesized(paren) => resolve_hint_target_fqn(paren.hint, resolved),
        _ => None,
    }
}

fn build_instance_registration(
    mc: &MethodCall<'_>,
    target: &str,
    content: &str,
    php_version: Option<PhpVersion>,
) -> Option<MacroRegistration> {
    let mut args = mc.argument_list.arguments.iter();
    let name_arg = args.next()?.value();
    let name = macro_name(name_arg)?;
    let name_offset = name_arg.span().start.offset;
    let (parameter_list, return_type_hint) = closure_signature(args.next()?.value())?;

    let parameters =
        crate::parser::extract_parameters(parameter_list, Some(content), php_version, None);
    let return_type = return_type_hint.map(|rth| crate::parser::extract_hint_type(&rth.hint));

    let mut method = MethodInfo::virtual_method_typed(&name, return_type.as_ref());
    method.parameters = parameters;
    method.native_return_type = return_type;

    Some(MacroRegistration {
        target: target.to_string(),
        method,
        name_offset,
    })
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
    let name_arg = args.next()?.value();
    let name = macro_name(name_arg)?;
    let name_offset = name_arg.span().start.offset;
    let (parameter_list, return_type_hint) = closure_signature(args.next()?.value())?;

    let parameters =
        crate::parser::extract_parameters(parameter_list, Some(content), php_version, None);
    let return_type = return_type_hint.map(|rth| crate::parser::extract_hint_type(&rth.hint));

    let mut method = MethodInfo::virtual_method_typed(&name, return_type.as_ref());
    method.parameters = parameters;
    method.native_return_type = return_type;

    Some(MacroRegistration {
        target,
        method,
        name_offset,
    })
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

/// Collect the Laravel service-provider FQNs that installed vendor packages
/// register via `extra.laravel.providers` in `vendor/composer/installed.json`.
///
/// These are the classes Laravel's package auto-discovery boots, and the
/// precise, bounded set of vendor files where `::macro()` calls live.  Scanning
/// these (rather than the whole vendor tree) keeps macro discovery cheap.
pub(crate) fn parse_installed_providers(installed_json: &str) -> Vec<String> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(installed_json) else {
        return Vec::new();
    };
    // installed.json is either a top-level array (Composer 1) or
    // `{ "packages": [...] }` (Composer 2).
    let packages = json
        .as_array()
        .or_else(|| json.get("packages").and_then(|p| p.as_array()));
    let Some(packages) = packages else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for package in packages {
        let Some(providers) = package
            .pointer("/extra/laravel/providers")
            .and_then(|v| v.as_array())
        else {
            continue;
        };
        for provider in providers {
            if let Some(fqn) = provider.as_str() {
                out.push(fqn.trim_start_matches('\\').to_string());
            }
        }
    }
    out
}

/// Collect service-provider FQNs registered in a PHP provider-list file.
///
/// Handles both `bootstrap/providers.php` (Laravel 11+, a bare
/// `return [Foo::class, ...];`) and `config/app.php` (Laravel ≤10, a
/// `'providers' => [...]` entry, possibly built via
/// `ServiceProvider::defaultProviders()->merge([...])`).  When a `providers`
/// array key is present its `::class` entries are collected; otherwise every
/// `::class` in the file is collected.
pub(crate) fn parse_provider_class_list(content: &str) -> Vec<String> {
    if memchr::memmem::find(content.as_bytes(), b"::class").is_none() {
        return Vec::new();
    }

    let arena = Bump::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);

    let mut out = Vec::new();
    if let Some(providers_value) = find_return_array_entry(program, "providers") {
        collect_class_consts(Node::Expression(providers_value), &owned, &mut out);
    } else {
        collect_class_consts(Node::Program(program), &owned, &mut out);
    }
    out
}

pub(crate) fn parse_provider_referenced_classes(content: &str) -> Vec<String> {
    if !content.contains("::") && !content.contains("new ") {
        return Vec::new();
    }

    let arena = Bump::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    collect_provider_method_refs(Node::Program(program), &owned, &mut seen, &mut out);
    out
}

/// The value expression of a top-level `return [ 'key' => … ]` array entry.
fn find_return_array_entry<'ast, 'arena>(
    program: &'ast Program<'arena>,
    key: &str,
) -> Option<&'ast Expression<'arena>> {
    for stmt in program.statements.iter() {
        if let Statement::Return(ret) = stmt
            && let Some(Expression::Array(arr)) = ret.value
        {
            for (k, v) in arr.elements.iter().filter_map(|e| match e {
                ArrayElement::KeyValue(kv) => Some((kv.key, kv.value)),
                _ => None,
            }) {
                if matches!(
                    k,
                    Expression::Literal(Literal::String(s))
                        if s.value.is_some_and(|val| bytes_to_str(val) == key)
                ) {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn collect_provider_method_refs(
    node: Node<'_, '_>,
    resolved: &OwnedResolvedNames,
    seen: &mut HashSet<String>,
    out: &mut Vec<String>,
) {
    use mago_syntax::ast::class_like::member::ClassLikeMember;
    use mago_syntax::ast::class_like::method::MethodBody;

    match node {
        Node::Class(class) => {
            for member in class.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(body) = &method.body
                {
                    collect_class_refs(Node::Block(body), resolved, seen, out);
                }
            }
        }
        Node::AnonymousClass(class) => {
            for member in class.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(body) = &method.body
                {
                    collect_class_refs(Node::Block(body), resolved, seen, out);
                }
            }
        }
        Node::Trait(trait_) => {
            for member in trait_.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(body) = &method.body
                {
                    collect_class_refs(Node::Block(body), resolved, seen, out);
                }
            }
        }
        Node::Enum(enum_) => {
            for member in enum_.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(body) = &method.body
                {
                    collect_class_refs(Node::Block(body), resolved, seen, out);
                }
            }
        }
        Node::Interface(_) => {}
        _ => node.visit_children(|child| collect_provider_method_refs(child, resolved, seen, out)),
    }
}

fn collect_class_refs(
    node: Node<'_, '_>,
    resolved: &OwnedResolvedNames,
    seen: &mut HashSet<String>,
    out: &mut Vec<String>,
) {
    match node {
        Node::StaticMethodCall(call) => push_resolved_expr_fqn(call.class, resolved, seen, out),
        Node::ClassConstantAccess(access)
            if matches!(
                &access.constant,
                ClassLikeConstantSelector::Identifier(id)
                    if bytes_to_str(id.value).eq_ignore_ascii_case("class")
            ) =>
        {
            push_resolved_expr_fqn(access.class, resolved, seen, out)
        }
        _ => node.visit_children(|child| collect_class_refs(child, resolved, seen, out)),
    }
}

fn push_resolved_expr_fqn(
    expr: &Expression<'_>,
    resolved: &OwnedResolvedNames,
    seen: &mut HashSet<String>,
    out: &mut Vec<String>,
) {
    let Expression::Identifier(ident) = expr else {
        return;
    };
    let raw = bytes_to_str(ident.value());
    if matches!(
        raw.to_ascii_lowercase().as_str(),
        "self" | "static" | "parent"
    ) {
        return;
    }
    let Some(fqn) = resolved.get(ident.span().start.offset) else {
        if raw.is_empty() {
            return;
        }
        let raw = raw.trim_start_matches('\\').to_string();
        if seen.insert(raw.clone()) {
            out.push(raw);
        }
        return;
    };
    let fqn = fqn.trim_start_matches('\\').to_string();
    if seen.insert(fqn.clone()) {
        out.push(fqn);
    }
}

/// Recursively collect the FQN of every `Something::class` constant reachable
/// from `node`, resolving short names via the file's `use` statements.
/// `self`/`static`/`parent` are skipped (no concrete FQN).
fn collect_class_consts(node: Node<'_, '_>, resolved: &OwnedResolvedNames, out: &mut Vec<String>) {
    if let Node::ClassConstantAccess(cca) = node
        && let ClassLikeConstantSelector::Identifier(id) = &cca.constant
        && bytes_to_str(id.value).eq_ignore_ascii_case("class")
        && let Expression::Identifier(ident) = cca.class
    {
        let raw = bytes_to_str(ident.value());
        if !matches!(
            raw.to_ascii_lowercase().as_str(),
            "self" | "static" | "parent"
        ) {
            let offset = ident.span().start.offset;
            let fqn = resolved
                .get(offset)
                .map(|f| f.trim_start_matches('\\').to_string())
                .or_else(|| (!raw.is_empty()).then(|| raw.trim_start_matches('\\').to_string()));
            if let Some(fqn) = fqn {
                out.push(fqn);
            }
        }
    }
    node.visit_children(|child| collect_class_consts(child, resolved, out));
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
