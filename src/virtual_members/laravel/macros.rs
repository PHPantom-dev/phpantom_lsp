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

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
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
    /// Raw source text of the closure / arrow-function argument.
    ///
    /// Kept so the backend can infer a return type from the body when the
    /// registration has no explicit `: ReturnType` annotation.
    pub closure_text: Option<String>,
    /// Optional override for the go-to-definition location, as a file URI.
    ///
    /// A plain `Target::macro('name', ...)` registration has its definition in
    /// the same file it was found in, so this stays `None` and the index keys
    /// the location under the contributing file's URI. A `mixin()`-derived
    /// registration instead points `name_offset` at the mixin method's own
    /// declaration, which lives in a *different* file; this holds that file's
    /// URI so go-to-definition jumps to the mixin method rather than the
    /// `::mixin(...)` call site.
    pub definition_uri: Option<String>,
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

    let arena = mago_allocator::LocalArena::new();
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

/// Return the resolved target of a `Target::macro('name', closure)` call when
/// `cursor_offset` is inside the closure body.
pub(crate) fn macro_closure_this_target(content: &str, cursor_offset: u32) -> Option<String> {
    memchr::memmem::find(content.as_bytes(), b"macro(")?;

    let arena = mago_allocator::LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);

    let mut calls: Vec<&StaticMethodCall<'_>> = Vec::new();
    collect_macro_calls(Node::Program(program), &mut calls);

    calls.into_iter().find_map(|call| {
        let target = resolve_target_fqn(call.class, &owned)?;
        let closure_expr = call.argument_list.arguments.iter().nth(1)?.value();
        cursor_inside_closure_body(closure_expr, cursor_offset).then_some(target)
    })
}

/// A `Target::mixin(new X)` / `Target::mixin(X::class)` registration recovered
/// from source.
///
/// Unlike a `macro()` call, the macro signatures a mixin contributes live in
/// the mixin class `X`'s own file, which the single-file scanner cannot read.
/// This records only the resolved target and mixin class FQNs; the caller loads
/// `X`'s source and expands it via [`synthesize_mixin_macros`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MixinRegistration {
    /// FQN of the class written before `::mixin`, resolved via the file's `use`
    /// statements.  A `Macroable` class (the macros attach to it directly) or a
    /// facade (the caller resolves it to the concrete class before injecting).
    pub target: String,
    /// FQN of the mixin class passed as the sole `mixin()` argument, resolved
    /// from a literal `new X` / `X::class` via the file's `use` statements.
    pub mixin_fqn: String,
}

/// Extract every literal `Target::mixin(new X)` / `Target::mixin(X::class)`
/// registration from a file's source.
///
/// Returns an empty vector when the file contains no `mixin(` substring (a
/// cheap byte pre-filter).  Non-literal arguments (a variable or computed
/// value) are skipped, matching the scope of the `macro()` scanner.
pub(crate) fn extract_mixin_registrations(content: &str) -> Vec<MixinRegistration> {
    // Byte pre-filter: every registration contains the `mixin(` call token.
    if memchr::memmem::find(content.as_bytes(), b"mixin(").is_none() {
        return Vec::new();
    }

    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);

    let mut out = Vec::new();
    collect_mixin_calls(Node::Program(program), &owned, &mut out);
    out
}

/// Recursively collect every `X::mixin(new Y)` / `X::mixin(Y::class)` call
/// whose target and mixin argument both resolve to a class FQN.
fn collect_mixin_calls(
    node: Node<'_, '_>,
    resolved: &OwnedResolvedNames,
    out: &mut Vec<MixinRegistration>,
) {
    if let Node::StaticMethodCall(smc) = node
        && let ClassLikeMemberSelector::Identifier(ident) = &smc.method
        && bytes_to_str(ident.value).eq_ignore_ascii_case("mixin")
        && let Some(target) = resolve_target_fqn(smc.class, resolved)
        && let Some(arg) = smc.argument_list.arguments.first()
        && let Some(mixin_fqn) = resolve_mixin_argument_fqn(arg.value(), resolved)
    {
        out.push(MixinRegistration { target, mixin_fqn });
    }
    node.visit_children(|child| collect_mixin_calls(child, resolved, out));
}

/// Resolve the mixin-class FQN from a `mixin()` argument, accepting only the
/// two literal shapes `new X` / `new X(...)` and `X::class`.
fn resolve_mixin_argument_fqn(
    expr: &Expression<'_>,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    match expr {
        Expression::Instantiation(inst) => resolve_target_fqn(inst.class, resolved),
        Expression::Access(Access::ClassConstant(access))
            if matches!(
                &access.constant,
                ClassLikeConstantSelector::Identifier(constant)
                    if bytes_to_str(constant.value).eq_ignore_ascii_case("class")
            ) =>
        {
            resolve_target_fqn(access.class, resolved)
        }
        _ => None,
    }
}

/// Synthesize the macros a `Target::mixin(X)` registration contributes by
/// parsing the mixin class `X`'s source.
///
/// Each public/protected, non-static, concrete, non-magic method of `mixin_fqn`
/// whose body returns a closure/arrow-function becomes a macro on `target_fqn`
/// named after the method, taking the *returned closure's* parameters and
/// return type (mirroring Laravel's runtime `mixin()`, which invokes each
/// method to obtain the closure it registers).  `mixin_uri` is recorded as the
/// go-to-definition target so a macro call jumps to the mixin method's own
/// declaration rather than the `::mixin(...)` call site.
pub(crate) fn synthesize_mixin_macros(
    mixin_source: &str,
    mixin_fqn: &str,
    mixin_uri: &str,
    target_fqn: &str,
    php_version: Option<PhpVersion>,
) -> Vec<MacroRegistration> {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, mixin_source.as_bytes());

    let mut out = Vec::new();
    for statement in program.statements.iter() {
        collect_mixin_class_methods(
            statement,
            None,
            mixin_fqn,
            mixin_uri,
            target_fqn,
            mixin_source,
            php_version,
            &mut out,
        );
    }
    out
}

/// Walk a statement (descending through `namespace` blocks) for the class whose
/// FQN matches `mixin_fqn`, appending a macro for each of its qualifying
/// methods.
#[allow(clippy::too_many_arguments)]
fn collect_mixin_class_methods(
    statement: &Statement<'_>,
    namespace: Option<&str>,
    mixin_fqn: &str,
    mixin_uri: &str,
    target_fqn: &str,
    content: &str,
    php_version: Option<PhpVersion>,
    out: &mut Vec<MacroRegistration>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;

    match statement {
        Statement::Namespace(ns) => {
            let ns_name = ns
                .name
                .as_ref()
                .map(|n| bytes_to_str(n.value()).trim_matches('\\').to_string());
            for inner in ns.statements().iter() {
                collect_mixin_class_methods(
                    inner,
                    ns_name.as_deref(),
                    mixin_fqn,
                    mixin_uri,
                    target_fqn,
                    content,
                    php_version,
                    out,
                );
            }
        }
        Statement::Class(class) => {
            let class_name = bytes_to_str(class.name.value);
            let fqn = match namespace {
                Some(ns) => format!("{ns}\\{class_name}"),
                None => class_name.to_string(),
            };
            if !fqn.eq_ignore_ascii_case(mixin_fqn.trim_start_matches('\\')) {
                return;
            }
            for member in class.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let Some(reg) =
                        build_mixin_macro(method, target_fqn, mixin_uri, content, php_version)
                {
                    out.push(reg);
                }
            }
        }
        Statement::Trait(trait_def) => {
            let trait_name = bytes_to_str(trait_def.name.value);
            let fqn = match namespace {
                Some(ns) => format!("{ns}\\{trait_name}"),
                None => trait_name.to_string(),
            };
            if !fqn.eq_ignore_ascii_case(mixin_fqn.trim_start_matches('\\')) {
                return;
            }
            for member in trait_def.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let Some(reg) = build_direct_mixin_macro(
                        method,
                        target_fqn,
                        mixin_uri,
                        content,
                        php_version,
                    )
                {
                    out.push(reg);
                }
            }
        }
        _ => {}
    }
}

/// Build the macro a single mixin method contributes, or `None` when the method
/// is not eligible (private/static/abstract/magic) or its body does not return
/// a closure whose signature we can recover.
fn build_mixin_macro(
    method: &Method<'_>,
    target_fqn: &str,
    mixin_uri: &str,
    content: &str,
    php_version: Option<PhpVersion>,
) -> Option<MacroRegistration> {
    use mago_syntax::cst::class_like::method::MethodBody;

    // Laravel's `mixin()` copies public and protected methods; a private,
    // static, or abstract method is never registered, and a magic method
    // (`__construct`, `__call`, …) is not a macro factory.
    if method
        .modifiers
        .iter()
        .any(|m| m.is_private() || m.is_static() || m.is_abstract())
    {
        return None;
    }
    let MethodBody::Concrete(body) = &method.body else {
        return None;
    };
    let name = bytes_to_str(method.name.value);
    if name.starts_with("__") {
        return None;
    }

    // The macro's signature comes from the closure the method returns.
    let closure_expr = returned_closure(body)?;
    let (parameter_list, return_type_hint) = closure_signature(closure_expr)?;
    let closure_text = expr_source_text(Some(closure_expr), content);

    let parameters =
        crate::parser::extract_parameters(parameter_list, Some(content), php_version, None);
    let return_type = return_type_hint.map(|rth| crate::parser::extract_hint_type(&rth.hint));

    let mut synthesized = MethodInfo::virtual_method_typed(name, return_type.as_ref());
    synthesized.parameters = parameters;
    synthesized.native_return_type = return_type;
    synthesized.is_macro = true;

    Some(MacroRegistration {
        target: target_fqn.to_string(),
        method: synthesized,
        name_offset: method.name.span.start.offset,
        closure_text,
        definition_uri: Some(mixin_uri.to_string()),
    })
}

/// Build a macro from a trait mixin method whose own signature is used directly
/// (Carbon's trait-based `mixin()` pattern, where the method's parameters and
/// return type become the macro's signature, rather than the Laravel class-based
/// pattern where each method returns a closure).
fn build_direct_mixin_macro(
    method: &Method<'_>,
    target_fqn: &str,
    mixin_uri: &str,
    content: &str,
    php_version: Option<PhpVersion>,
) -> Option<MacroRegistration> {
    use mago_syntax::cst::class_like::method::MethodBody;

    if method
        .modifiers
        .iter()
        .any(|m| m.is_private() || m.is_static() || m.is_abstract())
    {
        return None;
    }
    let MethodBody::Concrete(_) = &method.body else {
        return None;
    };
    let name = bytes_to_str(method.name.value);
    if name.starts_with("__") {
        return None;
    }

    let parameters =
        crate::parser::extract_parameters(&method.parameter_list, Some(content), php_version, None);
    let return_type = method
        .return_type_hint
        .as_ref()
        .map(|rth| crate::parser::extract_hint_type(&rth.hint));

    let mut synthesized = MethodInfo::virtual_method_typed(name, return_type.as_ref());
    synthesized.parameters = parameters;
    synthesized.native_return_type = return_type;
    synthesized.is_macro = true;

    Some(MacroRegistration {
        target: target_fqn.to_string(),
        method: synthesized,
        name_offset: method.name.span.start.offset,
        closure_text: None,
        definition_uri: Some(mixin_uri.to_string()),
    })
}

/// The closure / arrow-function returned by a mixin method body, if the body's
/// top-level statements contain a `return <closure>;`.
fn returned_closure<'ast, 'arena>(body: &'ast Block<'arena>) -> Option<&'ast Expression<'arena>> {
    for statement in body.statements.iter() {
        if let Statement::Return(ret) = statement
            && let Some(value) = ret.value
            && matches!(value, Expression::Closure(_) | Expression::ArrowFunction(_))
        {
            return Some(value);
        }
    }
    None
}

/// Extract the concrete date class selected by `Date::use()` or
/// `Date::useClass()`, through either the facade or date factory.
pub(crate) fn extract_date_factory_class(content: &str) -> Option<String> {
    if !content.contains("::use") {
        return None;
    }

    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);
    let mut configured = None;

    fn collect(node: Node<'_, '_>, resolved: &OwnedResolvedNames, configured: &mut Option<String>) {
        if let Node::StaticMethodCall(call) = node
            && let ClassLikeMemberSelector::Identifier(method) = &call.method
            && matches!(
                bytes_to_str(method.value).to_ascii_lowercase().as_str(),
                "use" | "useclass"
            )
            && resolve_target_fqn(call.class, resolved).is_some_and(|target| {
                matches!(
                    target.as_str(),
                    "Illuminate\\Support\\Facades\\Date" | "Illuminate\\Support\\DateFactory"
                )
            })
            && let Some(arg) = call.argument_list.arguments.first()
            && let Expression::Access(Access::ClassConstant(access)) = arg.value()
            && matches!(
                &access.constant,
                ClassLikeConstantSelector::Identifier(constant)
                    if bytes_to_str(constant.value).eq_ignore_ascii_case("class")
            )
            && let Some(class) = resolve_target_fqn(access.class, resolved)
        {
            *configured = Some(class);
        }
        node.visit_children(|child| collect(child, resolved, configured));
    }

    collect(Node::Program(program), &owned, &mut configured);
    configured
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
    /// Reverse lookup from a registration string location back to the target
    /// FQN(s) and macro name it contributes.
    reverse_locations: HashMap<(String, u32), Vec<(String, String)>>,
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

    /// All macro targets contributed by the registration string at `uri` +
    /// `offset`, filtered to the given `name`.
    pub(crate) fn targets_at(&self, uri: &str, offset: u32, name: &str) -> Vec<String> {
        [offset, offset.saturating_sub(1)]
            .into_iter()
            .find_map(|candidate| {
                self.reverse_locations
                    .get(&(uri.to_string(), candidate))
                    .map(|entries| {
                        entries
                            .iter()
                            .filter(|(_, entry_name)| entry_name == name)
                            .map(|(target, _)| target.clone())
                            .collect::<Vec<_>>()
                    })
            })
            .unwrap_or_default()
    }

    /// Whether `fqn` has a recognized macro registration named `name`.
    pub(crate) fn has_macro(&self, fqn: &str, name: &str) -> bool {
        self.locations
            .contains_key(&(fqn.to_string(), name.to_string()))
    }

    /// The sole registration location for `name`, if exactly one target in the
    /// index contributes that macro name.
    pub(crate) fn unique_definition_for_name(&self, name: &str) -> Option<(&str, u32)> {
        let mut matches = self
            .locations
            .iter()
            .filter_map(|((_, macro_name), (uri, offset))| {
                (macro_name == name).then_some((uri.as_str(), *offset))
            });
        let first = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some(first)
    }

    /// Every class FQN that has at least one macro (used to evict stale
    /// resolved-class cache entries when the index changes).
    pub(crate) fn target_fqns(&self) -> Vec<String> {
        self.merged.keys().cloned().collect()
    }

    /// Return the closure source text of every macro registered on
    /// `Illuminate\Database\Schema\Blueprint`.  The migration scanner uses
    /// this to expand custom Blueprint macros when processing migrations.
    pub(crate) fn blueprint_macro_closures(&self) -> HashMap<String, String> {
        const BLUEPRINT_FQNS: &[&str] = &[
            "Illuminate\\Database\\Schema\\Blueprint",
            "Illuminate\\Database\\Schema\\Builder",
        ];
        let mut map = HashMap::new();
        for regs in self.by_uri.values() {
            for reg in regs {
                if BLUEPRINT_FQNS
                    .iter()
                    .any(|fqn| reg.target.eq_ignore_ascii_case(fqn))
                    && let Some(text) = &reg.closure_text
                {
                    map.entry(reg.method.name.to_string())
                        .or_insert_with(|| text.clone());
                }
            }
        }
        map
    }

    /// Rebuild `merged` from `by_uri`.  For each registration the macro is
    /// added as both a static and an instance method; duplicates
    /// (same name + staticness on the same target) keep the first seen.
    fn rebuild_merged(&mut self) {
        let mut merged: HashMap<String, Vec<Arc<MethodInfo>>> = HashMap::new();
        let mut locations: HashMap<(String, String), (String, u32)> = HashMap::new();
        let mut reverse_locations: HashMap<(String, u32), Vec<(String, String)>> = HashMap::new();
        for (uri, regs) in self.by_uri.iter() {
            for reg in regs {
                // A `mixin()`-derived registration points its location at the
                // mixin method's own file; a plain macro's location is the file
                // it was found in.
                let location_uri = reg.definition_uri.as_deref().unwrap_or(uri.as_str());
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
                        .or_insert_with(|| (location_uri.to_string(), reg.name_offset));
                }
                reverse_locations
                    .entry((location_uri.to_string(), reg.name_offset))
                    .or_default()
                    .push((reg.target.clone(), reg.method.name.to_string()));
            }
        }
        self.merged = merged;
        self.locations = locations;
        self.reverse_locations = reverse_locations;
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
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

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
            resolved,
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
                        resolved,
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
                        resolved,
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
                        resolved,
                        content,
                        php_version,
                        out,
                    );
                }
            }
        }
        // A closure or arrow function in top-level code opens a variable
        // scope of its own; the body walker computes it from the empty
        // enclosing scope.
        Node::Closure(_) | Node::ArrowFunction(_) => collect_instance_macros_in_body(
            node,
            &HashMap::new(),
            resolved,
            content,
            php_version,
            out,
        ),
        _ => node.visit_children(|child| {
            collect_instance_macro_registrations(child, resolved, content, php_version, out)
        }),
    }
}

fn collect_instance_macros_in_body(
    node: Node<'_, '_>,
    typed_targets: &HashMap<String, String>,
    resolved: &OwnedResolvedNames,
    content: &str,
    php_version: Option<PhpVersion>,
    out: &mut Vec<MacroRegistration>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    match node {
        // A closure sees only its `use (...)` captures plus its own
        // parameters; a typed parameter overrides everything else.
        Node::Closure(closure) => {
            let mut scope: HashMap<String, String> = HashMap::new();
            if let Some(use_clause) = &closure.use_clause {
                for capture in use_clause.variables.iter() {
                    let name = bytes_to_str(capture.variable.name);
                    if let Some(target) = typed_targets.get(name) {
                        scope.insert(name.to_string(), target.clone());
                    }
                }
            }
            for param in closure.parameter_list.parameters.iter() {
                scope.remove(bytes_to_str(param.variable.name));
            }
            scope.extend(typed_parameter_targets(&closure.parameter_list, resolved));
            collect_instance_macros_in_body(
                Node::Block(&closure.body),
                &scope,
                resolved,
                content,
                php_version,
                out,
            );
        }
        // An arrow function captures the enclosing scope automatically; its
        // own parameters shadow captured names.
        Node::ArrowFunction(arrow) => {
            let mut scope = typed_targets.clone();
            for param in arrow.parameter_list.parameters.iter() {
                scope.remove(bytes_to_str(param.variable.name));
            }
            scope.extend(typed_parameter_targets(&arrow.parameter_list, resolved));
            collect_instance_macros_in_body(
                Node::Expression(arrow.expression),
                &scope,
                resolved,
                content,
                php_version,
                out,
            );
        }
        // A nested named function starts a fresh variable scope.
        Node::Function(function) => collect_instance_macros_in_body(
            Node::Block(&function.body),
            &typed_parameter_targets(&function.parameter_list, resolved),
            resolved,
            content,
            php_version,
            out,
        ),
        // So does each method of an anonymous class.
        Node::AnonymousClass(class) => {
            for member in class.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(body) = &method.body
                {
                    collect_instance_macros_in_body(
                        Node::Block(body),
                        &typed_parameter_targets(&method.parameter_list, resolved),
                        resolved,
                        content,
                        php_version,
                        out,
                    );
                }
            }
        }
        _ => {
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
                collect_instance_macros_in_body(
                    child,
                    typed_targets,
                    resolved,
                    content,
                    php_version,
                    out,
                )
            });
        }
    }
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
    let closure_expr = args.next()?.value();
    let (parameter_list, return_type_hint) = closure_signature(closure_expr)?;
    let closure_text = expr_source_text(Some(closure_expr), content);

    let parameters =
        crate::parser::extract_parameters(parameter_list, Some(content), php_version, None);
    let return_type = return_type_hint.map(|rth| crate::parser::extract_hint_type(&rth.hint));

    let mut method = MethodInfo::virtual_method_typed(&name, return_type.as_ref());
    method.parameters = parameters;
    method.native_return_type = return_type;
    method.is_macro = true;

    Some(MacroRegistration {
        target: target.to_string(),
        method,
        name_offset,
        closure_text,
        definition_uri: None,
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
    let closure_expr = args.next()?.value();
    let (parameter_list, return_type_hint) = closure_signature(closure_expr)?;
    let closure_text = expr_source_text(Some(closure_expr), content);

    let parameters =
        crate::parser::extract_parameters(parameter_list, Some(content), php_version, None);
    let return_type = return_type_hint.map(|rth| crate::parser::extract_hint_type(&rth.hint));

    let mut method = MethodInfo::virtual_method_typed(&name, return_type.as_ref());
    method.parameters = parameters;
    method.native_return_type = return_type;
    method.is_macro = true;

    Some(MacroRegistration {
        target,
        method,
        name_offset,
        closure_text,
        definition_uri: None,
    })
}

fn expr_source_text(expr: Option<&Expression<'_>>, content: &str) -> Option<String> {
    let expr = expr?;
    let span = expr.span();
    let start = span.start.offset as usize;
    let end = span.end.offset as usize;
    (start < end && end <= content.len()).then(|| content[start..end].to_string())
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

    let arena = mago_allocator::LocalArena::new();
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

    let arena = LocalArena::new();
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
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

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
        Node::Instantiation(instantiation) => {
            push_resolved_expr_fqn(instantiation.class, resolved, seen, out)
        }
        Node::ClassConstantAccess(access)
            if matches!(
                &access.constant,
                ClassLikeConstantSelector::Identifier(id)
                    if bytes_to_str(id.value).eq_ignore_ascii_case("class")
            ) =>
        {
            push_resolved_expr_fqn(access.class, resolved, seen, out)
        }
        _ => {}
    }
    // Always descend: a static call's or instantiation's arguments can
    // themselves reference further helper classes.
    node.visit_children(|child| collect_class_refs(child, resolved, seen, out));
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

fn cursor_inside_closure_body(expr: &Expression<'_>, cursor_offset: u32) -> bool {
    match expr {
        Expression::Closure(closure) => {
            cursor_offset >= closure.body.left_brace.start.offset
                && cursor_offset <= closure.body.right_brace.end.offset
        }
        Expression::ArrowFunction(arrow) => {
            let body = arrow.expression.span();
            cursor_offset >= arrow.arrow.start.offset && cursor_offset <= body.end.offset
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "macros_tests.rs"]
mod tests;
