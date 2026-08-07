//! Static scan of Laravel's authorization gate: `Gate::define()` abilities
//! and the model → policy map.
//!
//! Laravel resolves an authorization string (`Gate::allows('update-post')`,
//! `$user->can('update', $post)`, `@can('view', $post)`) at runtime against a
//! `Gate` instance the application boots.  Two things populate it, and both
//! are literal facts in source:
//!
//! * **Gate definitions** — `Gate::define('name', …)` in a service provider's
//!   `boot()`, plus the `Gate::resource()` shorthand that expands to one
//!   ability per CRUD verb.
//! * **The policy map** — `Gate::policy(Model::class, Policy::class)`, the
//!   `protected $policies = […]` array an `AuthServiceProvider` declares, the
//!   `#[UsePolicy]` attribute on the model, and (when none of those name a
//!   policy) Laravel's discovery convention.  Every public method of the
//!   resolved policy is an ability valid *for that model*.
//!
//! This module recovers the first group with a source scan of the shape the
//! macro and morph-map scanners use ([`scan_gate_registrations`], stored in a
//! [`LaravelGateIndex`]), and the second with the registration map plus a
//! lazy, class-index-driven lookup ([`policy_class_for_model`]) that consumers
//! call when they know which model an ability was checked against.
//!
//! Registrations whose ability name is not a literal string are skipped: a
//! partial set is still useful, and an unrecoverable name simply keeps
//! resolving to nothing rather than to a guess.

use std::collections::HashMap;
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;

use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;
use crate::types::{ClassInfo, MethodInfo};

/// FQN of the gate contract the `Gate` facade proxies.
const GATE_FACADE_FQN: &str = "Illuminate\\Support\\Facades\\Gate";

/// The abilities `Gate::resource()` registers when no explicit list is given,
/// matching `Illuminate\Auth\Access\Gate::resourceAbilityMap()`.
const RESOURCE_ABILITIES: &[&str] = &["viewAny", "view", "create", "update", "delete"];

/// Policy methods that are framework hooks rather than abilities.
///
/// `before`/`after` run around *every* check, and the rest are the response
/// helpers `HandlesAuthorization` mixes in.  None of them is a name a
/// `Gate::allows(…)` call may legitimately use.
const NON_ABILITY_POLICY_METHODS: &[&str] = &[
    "before",
    "after",
    "allow",
    "deny",
    "denyWithStatus",
    "denyAsNotFound",
];

/// One `Gate::define('name', …)` registration recovered from source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GateDefinition {
    /// The ability name, e.g. `update-post`.
    pub name: String,
    /// Byte offset of the ability string literal's content (just inside the
    /// opening quote) in the file the registration was found in.
    pub offset: u32,
    /// The callback's parameter list as written, e.g.
    /// `User $user, Post $post`.  Shown in hover so the ability documents
    /// what it expects.  `None` when the callback is not a literal
    /// closure/arrow function.
    pub signature: Option<String>,
}

/// One model → policy binding recovered from source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PolicyRegistration {
    /// FQN of the model the policy governs.
    pub model_fqn: String,
    /// FQN of the policy class.
    pub policy_fqn: String,
    /// Byte offset of the registration (the model expression).
    pub offset: u32,
}

/// Everything one file contributes to the gate index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GateScan {
    pub definitions: Vec<GateDefinition>,
    pub policies: Vec<PolicyRegistration>,
}

impl GateScan {
    /// Whether the file contributes nothing at all to the index.
    pub(crate) fn is_empty(&self) -> bool {
        self.definitions.is_empty() && self.policies.is_empty()
    }
}

/// Extract every literal gate definition and policy registration from a
/// file's source.
///
/// Returns an empty scan when the file mentions neither `Gate` nor a
/// `$policies` property, so the parse is only paid for candidate files.
pub(crate) fn scan_gate_registrations(content: &str) -> GateScan {
    let bytes = content.as_bytes();
    if memchr::memmem::find(bytes, b"Gate").is_none()
        && memchr::memmem::find(bytes, b"$policies").is_none()
    {
        return GateScan::default();
    }

    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, bytes);
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);

    let mut scan = GateScan::default();
    collect_gate_nodes(Node::Program(program), &owned, content, &mut scan);
    scan
}

/// Recursively collect `Gate::…` calls and `$policies` property arrays.
fn collect_gate_nodes(
    node: Node<'_, '_>,
    resolved: &OwnedResolvedNames,
    content: &str,
    scan: &mut GateScan,
) {
    match node {
        Node::StaticMethodCall(smc) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &smc.method
                && subject_is_gate(smc.class, resolved)
            {
                collect_gate_call(
                    bytes_to_str(ident.value),
                    &smc.argument_list,
                    resolved,
                    content,
                    scan,
                );
            }
        }
        Node::PlainProperty(plain) => {
            for item in plain.items.iter() {
                let PropertyItem::Concrete(concrete) = item else {
                    continue;
                };
                if concrete.variable.name.strip_prefix(b"$").unwrap_or(b"") != b"policies" {
                    continue;
                }
                collect_policy_map(concrete.value, resolved, scan);
            }
        }
        _ => {}
    }
    node.visit_children(|child| collect_gate_nodes(child, resolved, content, scan));
}

/// Read one `Gate::<method>(…)` call into the scan.
fn collect_gate_call(
    method: &str,
    arguments: &ArgumentList<'_>,
    resolved: &OwnedResolvedNames,
    content: &str,
    scan: &mut GateScan,
) {
    let mut args = arguments.arguments.iter();
    let (Some(first), second) = (args.next(), args.next()) else {
        return;
    };

    if method.eq_ignore_ascii_case("define") {
        if let Some((name, offset)) = string_literal_content(first.value(), content) {
            scan.definitions.push(GateDefinition {
                name: name.to_string(),
                offset,
                signature: second.and_then(|arg| callback_signature(arg.value(), content)),
            });
        }
        return;
    }

    if method.eq_ignore_ascii_case("policy") {
        let Some(second) = second else {
            return;
        };
        let (Some(model_fqn), Some(policy_fqn)) = (
            class_constant_fqn(first.value(), resolved),
            class_constant_fqn(second.value(), resolved),
        ) else {
            return;
        };
        scan.policies.push(PolicyRegistration {
            model_fqn,
            policy_fqn,
            offset: first.value().span().start.offset,
        });
        return;
    }

    // `Gate::resource('photos', PhotoPolicy::class)` registers one ability
    // per CRUD verb, named `<resource>.<verb>`.  A third argument replaces
    // the default verb list with an explicit `ability => method` map.
    if method.eq_ignore_ascii_case("resource")
        && let Some((prefix, offset)) = string_literal_content(first.value(), content)
    {
        let explicit: Vec<String> = args
            .next()
            .map(|arg| array_keys_or_values(arg.value(), content))
            .unwrap_or_default();
        let abilities: Vec<String> = if explicit.is_empty() {
            RESOURCE_ABILITIES.iter().map(|a| a.to_string()).collect()
        } else {
            explicit
        };
        for ability in abilities {
            scan.definitions.push(GateDefinition {
                name: format!("{prefix}.{ability}"),
                offset,
                signature: None,
            });
        }
    }
}

/// Read a `[Model::class => Policy::class, …]` literal into the scan.
fn collect_policy_map(expr: &Expression<'_>, resolved: &OwnedResolvedNames, scan: &mut GateScan) {
    let elements = match expr {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return,
    };
    for element in elements.iter() {
        let ArrayElement::KeyValue(kv) = element else {
            continue;
        };
        let (Some(model_fqn), Some(policy_fqn)) = (
            class_constant_fqn(kv.key, resolved),
            class_constant_fqn(kv.value, resolved),
        ) else {
            continue;
        };
        scan.policies.push(PolicyRegistration {
            model_fqn,
            policy_fqn,
            offset: kv.key.span().start.offset,
        });
    }
}

/// The keys of an `['ability' => 'method']` map, or the values of a plain
/// `['ability', …]` list — the two shapes `Gate::resource()` accepts for its
/// ability argument.
fn array_keys_or_values(expr: &Expression<'_>, content: &str) -> Vec<String> {
    let elements = match expr {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return Vec::new(),
    };
    elements
        .iter()
        .filter_map(|element| match element {
            ArrayElement::KeyValue(kv) => string_literal_content(kv.key, content),
            ArrayElement::Value(value) => string_literal_content(value.value, content),
            _ => None,
        })
        .map(|(text, _)| text.to_string())
        .collect()
}

/// Whether the class written before `::define` resolves to Laravel's `Gate`.
///
/// Both the facade and the `Illuminate\Contracts\Auth\Access\Gate` contract
/// (which a provider may reference directly) count; requiring the resolved
/// FQN keeps an unrelated `Gate` class in the current namespace from matching
/// while an aliased import still does.
fn subject_is_gate(class: &Expression<'_>, resolved: &OwnedResolvedNames) -> bool {
    let Expression::Identifier(ident) = class else {
        return false;
    };
    let raw = bytes_to_str(ident.value());
    let fqn = resolved
        .get(ident.span().start.offset)
        .map(|fqn| fqn.trim_start_matches('\\').to_string())
        .unwrap_or_else(|| raw.trim_start_matches('\\').to_string());
    fqn.eq_ignore_ascii_case(GATE_FACADE_FQN)
        || fqn.eq_ignore_ascii_case("Illuminate\\Contracts\\Auth\\Access\\Gate")
        || fqn.eq_ignore_ascii_case("Illuminate\\Auth\\Access\\Gate")
}

/// The parameter list of a closure or arrow function, as written in source.
fn callback_signature(expr: &Expression<'_>, content: &str) -> Option<String> {
    let span = match expr {
        Expression::Closure(closure) => closure.parameter_list.span(),
        Expression::ArrowFunction(arrow) => arrow.parameter_list.span(),
        _ => return None,
    };
    let text = content.get(span.start.offset as usize..span.end.offset as usize)?;
    let inner = text
        .trim()
        .strip_prefix('(')?
        .strip_suffix(')')?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!inner.is_empty()).then_some(inner)
}

/// The FQN behind a `Model::class` expression, resolved via the file's `use`
/// statements.  A plain string literal is accepted too, since
/// `['App\Models\Post' => …]` is a legal (if unidiomatic) spelling.
fn class_constant_fqn(expr: &Expression<'_>, resolved: &OwnedResolvedNames) -> Option<String> {
    match expr {
        Expression::Access(Access::ClassConstant(access))
            if matches!(
                &access.constant,
                ClassLikeConstantSelector::Identifier(constant)
                    if bytes_to_str(constant.value).eq_ignore_ascii_case("class")
            ) =>
        {
            let Expression::Identifier(ident) = access.class else {
                return None;
            };
            let raw = bytes_to_str(ident.value());
            if matches!(
                raw.to_ascii_lowercase().as_str(),
                "self" | "static" | "parent"
            ) {
                return None;
            }
            resolved
                .get(ident.span().start.offset)
                .map(|fqn| fqn.trim_start_matches('\\').to_string())
                .or_else(|| (!raw.is_empty()).then(|| raw.trim_start_matches('\\').to_string()))
        }
        Expression::Literal(Literal::String(s)) => {
            let value = s.value?;
            let fqn = bytes_to_str(value).replace("\\\\", "\\");
            let fqn = fqn.trim_start_matches('\\');
            (!fqn.is_empty() && fqn.contains('\\')).then(|| fqn.to_string())
        }
        _ => None,
    }
}

/// The content of a non-interpolated string literal plus the byte offset of
/// that content (just inside the opening quote).
fn string_literal_content<'c>(expr: &Expression<'_>, content: &'c str) -> Option<(&'c str, u32)> {
    let Expression::Literal(Literal::String(s)) = expr else {
        return None;
    };
    let start = s.span.start.offset + 1;
    let end = s.span.end.offset - 1;
    if start >= end || end as usize > content.len() {
        return None;
    }
    let text = &content[start as usize..end as usize];
    (!text.is_empty()).then_some((text, start))
}

// ─── Index ───────────────────────────────────────────────────────────────────

/// Where a gate ability was defined, and with what callback signature.
#[derive(Clone, Debug)]
pub(crate) struct GateAbilityTarget {
    /// URI of the file holding the `Gate::define()` call.
    pub uri: String,
    /// Byte offset of the ability literal within that file.
    pub offset: u32,
    /// The callback's parameter list, when it was a literal closure.
    pub signature: Option<String>,
}

/// Project-wide index of Laravel gate definitions and policy registrations.
///
/// Stored on [`Backend`](crate::Backend) and built for Laravel projects after
/// indexing.  `by_uri` is the source of truth (one entry per contributing
/// file, so an edit can replace just that file's registrations); the rest are
/// derived lookup maps.
#[derive(Default)]
pub(crate) struct LaravelGateIndex {
    by_uri: HashMap<String, GateScan>,
    /// Ability name → where `Gate::define()` declared it.
    abilities: HashMap<String, GateAbilityTarget>,
    /// Model FQN → the FQN of the policy registered for it.
    policies: HashMap<String, String>,
}

impl LaravelGateIndex {
    /// Replace the registrations contributed by `uri`.  Passing an empty scan
    /// removes the file's contributions.  Call [`Self::rebuild`] afterwards
    /// (deferred so a bulk build rebuilds once rather than per file).
    pub(crate) fn set_file(&mut self, uri: String, scan: GateScan) {
        if scan.is_empty() {
            self.by_uri.remove(&uri);
        } else {
            self.by_uri.insert(uri, scan);
        }
    }

    /// Rebuild the derived lookup maps from the per-file scans.
    ///
    /// A duplicate keeps the first registration seen: scan order across files
    /// is not the runtime boot order, so first-wins is the stable choice
    /// (matching the macro and morph-map indexes).
    pub(crate) fn rebuild(&mut self) {
        let mut abilities: HashMap<String, GateAbilityTarget> = HashMap::new();
        let mut policies: HashMap<String, String> = HashMap::new();

        for (uri, scan) in self.by_uri.iter() {
            for definition in &scan.definitions {
                abilities
                    .entry(definition.name.clone())
                    .or_insert_with(|| GateAbilityTarget {
                        uri: uri.clone(),
                        offset: definition.offset,
                        signature: definition.signature.clone(),
                    });
            }
            for registration in &scan.policies {
                policies
                    .entry(registration.model_fqn.clone())
                    .or_insert_with(|| registration.policy_fqn.clone());
            }
        }

        self.abilities = abilities;
        self.policies = policies;
    }

    /// Whether `uri` currently contributes any registrations.
    pub(crate) fn has_uri(&self, uri: &str) -> bool {
        self.by_uri.contains_key(uri)
    }

    /// Where `Gate::define()` declared an ability, if it did.
    pub(crate) fn definition(&self, ability: &str) -> Option<&GateAbilityTarget> {
        self.abilities.get(ability)
    }

    /// Every ability name a `Gate::define()` call registered.
    pub(crate) fn definition_names(&self) -> Vec<String> {
        self.abilities.keys().cloned().collect()
    }

    /// The FQN of the policy explicitly registered for a model, if any.
    pub(crate) fn policy_for(&self, model_fqn: &str) -> Option<&str> {
        self.policies
            .get(model_fqn.trim_start_matches('\\'))
            .map(String::as_str)
    }

    /// Every explicitly registered policy class.
    pub(crate) fn registered_policy_fqns(&self) -> Vec<String> {
        let mut fqns: Vec<String> = self.policies.values().cloned().collect();
        fqns.sort();
        fqns.dedup();
        fqns
    }
}

// ─── Policy resolution ───────────────────────────────────────────────────────

/// The policy class governing a model, resolved the way Laravel's `Gate`
/// does.
///
/// Checks, in Laravel's own precedence order:
///
/// 1. An explicit `Gate::policy()` / `$policies` registration.
/// 2. The `#[UsePolicy(FooPolicy::class)]` attribute on the model.
/// 3. The discovery convention — `App\Models\Post` looks for
///    `App\Models\Policies\PostPolicy` and then `App\Policies\PostPolicy`.
///
/// Returns `None` when no policy class can be loaded, which leaves the
/// ability judged against the global `Gate::define()` set alone.
pub(crate) fn policy_class_for_model(
    backend: &crate::Backend,
    model_fqn: &str,
) -> Option<Arc<ClassInfo>> {
    let model_fqn = model_fqn.trim_start_matches('\\');

    let registered = backend
        .laravel_gates
        .read()
        .policy_for(model_fqn)
        .map(str::to_string);
    if let Some(policy_fqn) = registered
        && let Some(class) = load_policy(backend, &policy_fqn)
    {
        return Some(class);
    }

    if let Some(class) = backend.find_or_load_class(model_fqn)
        && let Some(policy) = class.laravel().and_then(|l| l.policy_class.clone())
        && let Some(policy_class) = load_policy(backend, &policy)
    {
        return Some(policy_class);
    }

    guessed_policy_names(model_fqn)
        .into_iter()
        .find_map(|candidate| load_policy(backend, &candidate))
}

/// Load a policy class with its parent chain and traits merged in, so that a
/// policy extending a shared base contributes the base's abilities too.
fn load_policy(backend: &crate::Backend, fqn: &str) -> Option<Arc<ClassInfo>> {
    let class = backend.find_or_load_class(fqn.trim_start_matches('\\'))?;
    let class_loader = |name: &str| backend.find_or_load_class(name);
    Some(crate::virtual_members::resolve_class_fully_cached(
        &class,
        &class_loader,
        &backend.resolved_class_cache,
    ))
}

/// The policy class names Laravel's `Gate::guessPolicyName()` tries, in the
/// order it tries them.
///
/// The framework builds one candidate per prefix of the model's namespace
/// (`App\Policies\PostPolicy`, `App\Models\Policies\PostPolicy`), appends the
/// two `\Models\` → `\Policies\` rewrites when the namespace contains a
/// `Models` segment, then takes the *last* candidate that exists — so the
/// list is reversed here and the caller picks the first that loads.
pub(crate) fn guessed_policy_names(model_fqn: &str) -> Vec<String> {
    let model_fqn = model_fqn.trim_start_matches('\\');
    let Some((namespace, short)) = model_fqn.rsplit_once('\\') else {
        return vec![format!("Policies\\{model_fqn}Policy")];
    };

    let segments: Vec<&str> = namespace.split('\\').collect();
    let mut candidates: Vec<String> = (1..=segments.len())
        .map(|index| format!("{}\\Policies\\{short}Policy", segments[..index].join("\\")))
        .collect();

    // Laravel only applies the rewrites when a `Models` segment has something
    // after it, so `App\Models` itself is left to the prefix candidates.
    if namespace.contains("\\Models\\") {
        candidates.push(format!(
            "{}\\{short}Policy",
            namespace.replacen("\\Models\\", "\\Policies\\", 1)
        ));
        candidates.push(format!(
            "{}\\{short}Policy",
            namespace.replacen("\\Models\\", "\\Models\\Policies\\", 1)
        ));
    }

    candidates.reverse();
    candidates.dedup();
    candidates
}

/// The abilities a policy class declares: its public, non-static instance
/// methods, minus the framework hooks and response helpers.
pub(crate) fn policy_abilities(policy: &ClassInfo) -> Vec<&Arc<MethodInfo>> {
    policy
        .methods
        .iter()
        .filter(|method| {
            method.visibility == crate::types::Visibility::Public
                && !method.is_static
                && !method.name.starts_with("__")
                && !NON_ABILITY_POLICY_METHODS
                    .iter()
                    .any(|hook| method.name.eq_ignore_ascii_case(hook))
        })
        .collect()
}

/// Every ability valid for a model: the methods of its policy.
///
/// Returns `None` when the model has no resolvable policy — a caller must not
/// read that as "no abilities", but as "nothing is known about this model".
pub(crate) fn model_policy_abilities(
    backend: &crate::Backend,
    model_fqn: &str,
) -> Option<(Arc<ClassInfo>, Vec<String>)> {
    let policy = policy_class_for_model(backend, model_fqn)?;
    let names = policy_abilities(&policy)
        .into_iter()
        .map(|method| method.name.to_string())
        .collect();
    Some((policy, names))
}

/// Every ability name the project knows about: `Gate::define()` registrations
/// plus the methods of every policy class the workspace declares.
///
/// Policy classes are found from the explicit registrations and from the
/// naming convention (a class whose short name ends in `Policy`), which is
/// how Laravel's own discovery finds them.  The union is what completion
/// offers and what the unknown-ability diagnostic judges an ability against
/// when the call names no model.
pub(crate) fn enumerate_gate_abilities(backend: &crate::Backend) -> Vec<String> {
    let mut names = backend.laravel_gates.read().definition_names();

    for policy in project_policy_classes(backend) {
        names.extend(
            policy_abilities(&policy)
                .into_iter()
                .map(|method| method.name.to_string()),
        );
    }

    names.sort();
    names.dedup();
    names
}

/// Every policy class the project declares, loaded and fully resolved.
pub(crate) fn project_policy_classes(backend: &crate::Backend) -> Vec<Arc<ClassInfo>> {
    policy_class_fqns(backend)
        .into_iter()
        .filter_map(|fqn| load_policy(backend, &fqn))
        .collect()
}

/// Every policy method named `ability`, paired with the class that *declares*
/// it, ordered by that class's FQN so the result is stable between requests.
///
/// Two policies extending a shared base both offer an ability the base
/// declares; the declaring class is reported once, so a consumer navigating to
/// the ability lands on the one method that implements it rather than on every
/// policy that inherits it.
pub(crate) fn policy_methods_named(
    backend: &crate::Backend,
    ability: &str,
) -> Vec<(Arc<ClassInfo>, Arc<MethodInfo>)> {
    let mut found: Vec<(Arc<ClassInfo>, Arc<MethodInfo>)> = Vec::new();
    for policy in project_policy_classes(backend) {
        let Some(method) = policy_abilities(&policy)
            .into_iter()
            .find(|method| method.name.eq_ignore_ascii_case(ability))
            .cloned()
        else {
            continue;
        };
        // The resolved policy carries inherited methods, whose `name_offset`
        // indexes the file that declares them — so walk back to that class.
        let (owner, method) =
            declaring_policy_class(backend, &policy, ability).unwrap_or((policy, method));
        if !found
            .iter()
            .any(|(existing, _)| existing.fqn() == owner.fqn())
        {
            found.push((owner, method));
        }
    }
    found.sort_by_key(|(policy, _)| policy.fqn());
    found
}

/// Walk a policy's own methods and then its parent chain for the class that
/// declares `ability`.
///
/// Returns `None` when no class in the chain declares it as its own member —
/// which happens when the ability comes from a trait, or a parent cannot be
/// loaded — leaving the caller with the resolved policy it started from.
fn declaring_policy_class(
    backend: &crate::Backend,
    policy: &ClassInfo,
    ability: &str,
) -> Option<(Arc<ClassInfo>, Arc<MethodInfo>)> {
    let mut current = backend.find_or_load_class(policy.fqn().as_str())?;
    loop {
        let own = current
            .methods
            .iter()
            .find(|method| method.name.eq_ignore_ascii_case(ability) && method.name_offset != 0);
        if let Some(method) = own {
            let method = method.clone();
            return Some((current, method));
        }
        let parent = current.parent_class.as_ref()?;
        current = backend.find_or_load_class(parent)?;
    }
}

/// The FQNs of every policy class in the project: those explicitly registered
/// plus those the `*Policy` naming convention identifies.
///
/// Vendor classes are excluded — a package's own policies govern its models,
/// not the application's, and loading every `*Policy` in `vendor/` would cost
/// far more than the abilities are worth.
fn policy_class_fqns(backend: &crate::Backend) -> Vec<String> {
    let mut fqns = backend.laravel_gates.read().registered_policy_fqns();
    {
        let index = backend.symbols.fqn_uri_index.read();
        for (fqn, uri) in index.iter() {
            if uri.contains("/vendor/") {
                continue;
            }
            let short = fqn.rsplit('\\').next().unwrap_or(fqn);
            if short.len() > "Policy".len() && short.ends_with("Policy") {
                fqns.push(fqn.to_string());
            }
        }
    }
    fqns.sort();
    fqns.dedup();
    fqns
}

#[cfg(test)]
#[path = "gates_tests.rs"]
mod tests;
