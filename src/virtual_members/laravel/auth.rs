//! Static resolution of the authenticated-user model type from
//! `config/auth.php`.
//!
//! Laravel's auth methods (`auth()->user()`, `Auth::user()`,
//! `$request->user()`) declare only the `Authenticatable` contract as their
//! return type, which carries almost no members.  The concrete model is
//! declared in `config/auth.php`, reachable by a three-hop traversal:
//!
//! ```text
//! auth.defaults.guard  →  auth.guards.<guard>.provider  →  auth.providers.<provider>.model
//! ```
//!
//! This module performs that traversal over a parsed [`ConfigNode`] tree and
//! returns a best-guess model type, following three deliberate principles:
//!
//! * **Never execute runtime meaning.** We read only the literal *default
//!   argument* of `env('KEY', <default>)`, never the actual environment
//!   variable — the developer's or CI's `.env` is not production.
//! * **Fan out when uncertain.** Any hop that a runtime value could override
//!   (or that we cannot read) enumerates *every* candidate at that hop, so
//!   the resulting union honestly reflects "it could be any of these."
//! * **Raise the floor to concrete implementors.** Whenever a branch is
//!   dynamic, the "some Authenticatable we cannot pin down" floor is expanded
//!   to the project's own classes that implement the contract, rather than the
//!   abstract interface.  In a single-model app this collapses the union to
//!   just that model; in a multi-model app it offers each.  The abstract
//!   `Authenticatable` is used only as a last resort when no implementor is
//!   known.  We never widen to `mixed` and never invent a contract Laravel did
//!   not itself guarantee.

use std::sync::Arc;

use tower_lsp::lsp_types::Url;

use crate::Backend;
use crate::php_type::PhpType;
use crate::types::ClassInfo;

use super::config_values::{ConfigNode, parse_config_tree};

/// FQN of the auth user contract every configured model must satisfy.
pub(crate) const AUTHENTICATABLE_FQN: &str = "Illuminate\\Contracts\\Auth\\Authenticatable";

/// FQN of the guard contract whose `user()` resolves the default-guard model.
pub(crate) const GUARD_FQN: &str = "Illuminate\\Contracts\\Auth\\Guard";

/// FQN of the HTTP request whose `user()` resolves the default-guard model.
pub(crate) const REQUEST_FQN: &str = "Illuminate\\Http\\Request";

/// Refine `Guard::user()` / `Request::user()` to return the configured auth
/// user model for the **default** guard, preserving the method's nullability.
///
/// This mirrors the DB and Cache facade patches in [`patches`](super::patches):
/// a vendor method whose declared return type is only the `?Authenticatable`
/// contract is refined once, so every consumer (completion, hover,
/// diagnostics, the forward walker) sees the concrete model without any
/// request-context plumbing.  Guard-argument-aware forms (`auth('admin')`)
/// resolve through these same methods and therefore currently receive the
/// default-guard model.
///
/// Returns `loaded` unchanged when the class is not one of the two auth entry
/// points, when `config/auth.php` yields no concrete model, or when there is
/// no `user()` method to refine.
pub(crate) fn patch_auth_user_class(backend: &Backend, loaded: Arc<ClassInfo>) -> Arc<ClassInfo> {
    let fqn = loaded.fqn();
    if fqn.as_str() != GUARD_FQN && fqn.as_str() != REQUEST_FQN {
        return loaded;
    }

    // Context-free FQN loader for the compatibility filter: configured models
    // and the `Authenticatable` contract are fully-qualified here.
    let loader = |name: &str| backend.find_or_load_class(name);
    let Some(model_type) = resolve_auth_user_type(backend, None, &loader) else {
        return loaded;
    };

    let mut patched = (*loaded).clone();
    let mut changed = false;
    for method in patched.methods.make_mut().iter_mut() {
        if method.name.as_str() != "user" {
            continue;
        }
        let method = Arc::make_mut(method);
        // `user()` is declared `?Authenticatable`; keep the result nullable.
        let nullable = method
            .return_type
            .as_ref()
            .is_none_or(|rt| rt.accepts_null());
        method.return_type = Some(if nullable {
            PhpType::Nullable(Box::new(model_type.clone()))
        } else {
            model_type.clone()
        });
        changed = true;
    }

    if changed { Arc::new(patched) } else { loaded }
}

/// Resolve (and memoize) the authenticated-user model type for a guard.
///
/// Reads `config/auth.php` statically and traverses guard → provider → model.
/// Results are cached per guard on the [`Backend`] and invalidated when files
/// are re-parsed, so the (cheap) config read happens at most once per guard
/// between edits.  Returns `None` when no concrete model can be pinned down,
/// leaving the caller's declared `?Authenticatable` return type intact.
pub(crate) fn resolve_auth_user_type(
    backend: &Backend,
    guard: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    let key = guard.unwrap_or("").to_string();
    if let Some(cached) = backend.auth_user_type_cache.read().get(&key) {
        return cached.clone();
    }
    // Break re-entry: resolving the floor scans the class index and re-loads
    // the auth entry points, which would re-invoke this patch.  Seed the cache
    // with `None` so a re-entrant load resolves to the raw (unpatched) class —
    // exactly what the implementor scan needs — then overwrite with the real
    // result once computed.
    backend
        .auth_user_type_cache
        .write()
        .insert(key.clone(), None);
    let result = compute_auth_user_type(backend, guard, class_loader);
    backend
        .auth_user_type_cache
        .write()
        .insert(key, result.clone());
    result
}

fn compute_auth_user_type(
    backend: &Backend,
    guard: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    let content = read_auth_config(backend)?;
    let tree = parse_config_tree(&content)?;
    let resolve_model = |name: &str| resolve_model_fqn(name, class_loader);
    let floor_implementors = || project_auth_implementors(backend, class_loader);
    resolve_auth_user_model(&tree, guard, &resolve_model, &floor_implementors)
}

/// The project's own concrete classes implementing `Authenticatable`.
///
/// Vendor implementors (the framework base `Foundation\Auth\User`,
/// `Auth\GenericUser`, …) are excluded: they are never the model a developer
/// configures, so widening the floor to them would only add framework noise.
fn project_auth_implementors(
    backend: &Backend,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<String> {
    // Resolve implementors first so its internal index reads are released
    // before we take the `fqn_uri_index` read guard below.  `project_only`
    // keeps the scan from loading and parsing every class in the vendor
    // tree: the only implementors we keep are project-local ones (the
    // `/vendor/` filter below), and vendor classes are already parsed
    // during indexing, so restricting the scan up front avoids a
    // multi-second full-vendor parse on large Laravel apps.
    let implementors = backend.find_implementors(
        "Authenticatable",
        AUTHENTICATABLE_FQN,
        class_loader,
        false,
        false,
        true,
    );
    let uri_index = backend.fqn_uri_index.read();
    implementors
        .into_iter()
        .map(|cls| cls.fqn().to_string())
        .filter(|fqn| match uri_index.get(fqn.as_str()) {
            // Keep project-local classes; drop vendor implementors.  The
            // `/vendor/` segment is Composer's convention and is checked
            // identically by the LSP and the `analyze` CLI (unlike
            // `vendor_uri_prefixes`, which only the LSP populates).  A class
            // with no indexed URI is treated as project-local.
            Some(uri) => !uri.contains("/vendor/"),
            None => true,
        })
        .collect()
}

/// Read the project's `config/auth.php`, preferring an open editor buffer over
/// the on-disk copy.
fn read_auth_config(backend: &Backend) -> Option<String> {
    let root = backend.workspace_root.read().clone()?;
    let path = root.join("config").join("auth.php");
    if !path.is_file() {
        return None;
    }
    // Prefer an open editor buffer (keyed by an absolute file URI); fall back
    // to disk.  `Url::from_file_path` requires an absolute path, so relative
    // workspace roots (e.g. `analyze --project-root`) go straight to disk.
    if let Ok(uri) = Url::from_file_path(&path)
        && let Some(content) = backend.get_file_content(uri.as_ref())
    {
        return Some(content);
    }
    std::fs::read_to_string(&path).ok()
}

/// Resolve a configured model name to its canonical FQN, keeping it only if it
/// is a subtype of [`AUTHENTICATABLE_FQN`].  This both filters misreads and
/// canonicalizes short names written against a `use` statement.
fn resolve_model_fqn(
    name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    let normalized = name.trim_start_matches('\\');
    let class_info = class_loader(normalized)?;
    if crate::util::is_subtype_of(&class_info, AUTHENTICATABLE_FQN, class_loader) {
        Some(class_info.fqn().to_string())
    } else {
        None
    }
}

/// Resolve the authenticated-user model type from a parsed `config/auth.php`
/// tree.
///
/// * `guard` — an explicit guard name (from `auth('admin')` /
///   `Auth::guard('admin')` / `$request->user('admin')`), or `None` for the
///   default guard.
/// * `resolve_model` — maps a class name *as written in the config* to its
///   fully-qualified name **iff** it is a subtype of [`AUTHENTICATABLE_FQN`].
///   Returning `None` both filters out misreads (a value we mistook for a
///   model) and drives the fan-out towards the floor.
/// * `floor_implementors` — the concrete classes that implement the
///   `Authenticatable` contract in the project.  When a branch is uncertain,
///   the floor is *raised* from the abstract contract to this concrete set:
///   "some Authenticatable we cannot pin down" honestly becomes "one of the
///   classes that actually implement it."  In the common single-model project
///   this collapses the union to just that model; the abstract contract is
///   only used as a last resort when no implementor is known.
///
/// Returns `None` when no concrete model can be pinned down — in that case the
/// caller keeps the method's existing `?Authenticatable` return type, so there
/// is never a regression.  Otherwise returns the union of every candidate
/// model (config-derived models first, as the best guess).
pub(crate) fn resolve_auth_user_model(
    tree: &ConfigNode,
    guard: Option<&str>,
    resolve_model: &dyn Fn(&str) -> Option<String>,
    floor_implementors: &dyn Fn() -> Vec<String>,
) -> Option<PhpType> {
    let (mut models, uncertain) = collect_models(tree, guard, resolve_model);

    if uncertain {
        let implementors = floor_implementors();
        if implementors.is_empty() {
            // No concrete implementor is known.  Keep the abstract contract as
            // the floor when we at least have a best guess; otherwise leave
            // resolution untouched.
            if models.is_empty() {
                return None;
            }
            push_unique(&mut models, AUTHENTICATABLE_FQN.to_string());
        } else {
            // Raise the floor: every concrete implementor is a possible runtime
            // type.  Config-derived models are already first, so they rank
            // ahead of the rest as the best guess.
            for fqn in implementors {
                push_unique(&mut models, fqn);
            }
        }
    }

    if models.is_empty() {
        return None;
    }

    let mut members = models.into_iter().map(PhpType::Named);
    let first = members.next()?;
    match members.next() {
        None => Some(first),
        Some(second) => {
            let mut all = vec![first, second];
            all.extend(members);
            Some(PhpType::Union(all))
        }
    }
}

/// Append `value` to `out` if not already present (order-preserving dedup).
fn push_unique(out: &mut Vec<String>, value: String) {
    if !out.iter().any(|existing| existing == &value) {
        out.push(value);
    }
}

/// Walk the guard → provider → model chain, collecting every candidate model
/// FQN and whether any branch was runtime-dynamic or unreadable.
fn collect_models(
    tree: &ConfigNode,
    guard: Option<&str>,
    resolve_model: &dyn Fn(&str) -> Option<String>,
) -> (Vec<String>, bool) {
    let mut models: Vec<String> = Vec::new();
    let mut uncertain = false;

    let guard_names = resolve_guards(tree, guard, &mut uncertain);
    for guard_name in &guard_names {
        let providers = resolve_providers(tree, guard_name, &mut uncertain);
        for provider in &providers {
            resolve_provider_models(tree, provider, resolve_model, &mut models, &mut uncertain);
        }
    }

    (models, uncertain)
}

/// The set of guards to consider: the explicit argument, the (possibly
/// env-overridable) default, or every configured guard when the choice is
/// uncertain.
fn resolve_guards(tree: &ConfigNode, guard: Option<&str>, uncertain: &mut bool) -> Vec<String> {
    if let Some(explicit) = guard {
        return vec![explicit.to_string()];
    }
    match tree.value_at(&["defaults", "guard"]) {
        Some(value) => {
            let (names, dynamic) = value.as_strings();
            if dynamic || names.is_empty() {
                *uncertain = true;
                fan_out(tree, &["guards"], names)
            } else {
                names
            }
        }
        None => {
            *uncertain = true;
            all_child_keys(tree, &["guards"])
        }
    }
}

/// The set of providers backing a guard: the (possibly env-overridable)
/// declared provider, or every configured provider when uncertain.
fn resolve_providers(tree: &ConfigNode, guard_name: &str, uncertain: &mut bool) -> Vec<String> {
    match tree.value_at(&["guards", guard_name, "provider"]) {
        Some(value) => {
            let (names, dynamic) = value.as_strings();
            if dynamic || names.is_empty() {
                *uncertain = true;
                fan_out(tree, &["providers"], names)
            } else {
                names
            }
        }
        None => {
            // A guard with no readable provider tells us nothing concrete.
            *uncertain = true;
            Vec::new()
        }
    }
}

/// Resolve and compatibility-filter every model a provider may map to.
fn resolve_provider_models(
    tree: &ConfigNode,
    provider: &str,
    resolve_model: &dyn Fn(&str) -> Option<String>,
    models: &mut Vec<String>,
    uncertain: &mut bool,
) {
    let Some(value) = tree.value_at(&["providers", provider, "model"]) else {
        *uncertain = true;
        return;
    };
    let (classes, dynamic) = value.as_classes();
    if dynamic || classes.is_empty() {
        *uncertain = true;
    }
    for name in classes {
        match resolve_model(&name) {
            Some(fqn) => {
                if !models.iter().any(|existing| existing == &fqn) {
                    models.push(fqn);
                }
            }
            // A configured value that is not an Authenticatable subtype means
            // we misread the config; drop it and widen to the contract.
            None => *uncertain = true,
        }
    }
}

/// When an intermediate hop is uncertain, fan out to every configured child
/// key (guard or provider).  Falls back to whatever literal names we did
/// resolve if the parent key is absent.
fn fan_out(tree: &ConfigNode, path: &[&str], fallback: Vec<String>) -> Vec<String> {
    let all = all_child_keys(tree, path);
    if all.is_empty() { fallback } else { all }
}

fn all_child_keys(tree: &ConfigNode, path: &[&str]) -> Vec<String> {
    tree.get(path)
        .map(|node| node.child_keys())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
