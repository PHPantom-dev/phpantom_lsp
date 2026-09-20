//! Static resolution of `Storage::disk()` / `FilesystemManager::disk()` (and
//! the sibling `drive()`/`cloud()`/`build()`) return types from
//! `config/filesystems.php` and the `Storage::extend()` registrations a
//! project's service providers make.
//!
//! `FilesystemManager` declares all four as returning the
//! `Illuminate\Contracts\Filesystem\Filesystem` (or, for `cloud()`, `Cloud`)
//! contract, but every driver the framework ships builds a concrete
//! `Illuminate\Filesystem\FilesystemAdapter` (or a subclass of one). Since
//! `FilesystemAdapter` itself implements the `Cloud` contract, every built-in
//! driver's result satisfies the same concrete type regardless of which one
//! is configured. Assertion helpers (`assertExists()`, `assertMissing()`, …)
//! and adapter-only methods (`download()`, …) live only on the concrete
//! adapter, so member access on the declared contract falsely reports them as
//! missing.
//!
//! A project that calls `Storage::extend('name', …)` binds a disk to whatever
//! the registered closure builds, which need not be a `FilesystemAdapter`. So
//! the disk type is the union of what each configured disk resolves to: the
//! adapter for a driver the framework ships, and the closure's own return type
//! for one a `Storage::extend()` registration supplies. In the common case a
//! custom driver's closure returns a `FilesystemAdapter` too (it is how the
//! framework's own drivers, and every example in the Laravel documentation,
//! wrap a Flysystem adapter), so the union collapses back to the single
//! concrete type. A disk whose driver cannot be read as a single literal, or
//! whose custom driver has no discoverable registration, leaves the declared
//! contract untouched for every method.

use std::collections::HashMap;
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;

use crate::Backend;
use crate::atom::{atom, bytes_to_str};
use crate::names::OwnedResolvedNames;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MethodInfo};

use super::config_values::ConfigNode;
use super::macros::{
    closure_signature, expr_source_text, resolve_hint_target_fqn, resolve_target_fqn,
    string_literal_value,
};
use super::patches::{FILESYSTEM_ADAPTER_FQN, FILESYSTEM_CONTRACT_FQN, STORAGE_FACADE_FQN};

/// FQN of the concrete manager class `Storage::disk()` and friends forward
/// to via `__callStatic`.
pub(crate) const FILESYSTEM_MANAGER_FQN: &str = "Illuminate\\Filesystem\\FilesystemManager";

/// FQN of the `Cloud` contract that `cloud()` declares.
///
/// `Illuminate\Filesystem\FilesystemAdapter` implements this contract
/// directly, so every built-in driver's concrete result already satisfies
/// it; no separate cloud-specific concrete type is needed.
const CLOUD_CONTRACT_FQN: &str = "Illuminate\\Contracts\\Filesystem\\Cloud";

/// Driver names every supported Laravel version ships a
/// `FilesystemAdapter`-based implementation for. `scoped` delegates to
/// `build()`, which itself resolves through this same set, so it is sound to
/// include without following the delegation.
const BUILTIN_DRIVERS: &[&str] = &["local", "ftp", "sftp", "s3", "scoped"];

/// Public methods whose declared return type is the bare `Filesystem` /
/// `Cloud` contract but which hand back one of the configured disks.
const DISK_RETURNING_METHODS: &[&str] = &["drive", "disk", "cloud", "build"];

/// Class names a `Target::extend('driver', closure)` registration can be
/// written against: the facade and the manager it forwards to.
const STORAGE_EXTEND_TARGETS: &[&str] = &[STORAGE_FACADE_FQN, FILESYSTEM_MANAGER_FQN];

/// Refine `FilesystemManager`/`Storage`'s disk-returning methods from the
/// abstract `Filesystem`/`Cloud` contract to what the configured disks are
/// actually built from.
///
/// Mirrors [`super::patch_auth_user_class`]: a vendor method whose declared
/// return type is only the abstract contract is refined once, so every
/// consumer (completion, hover, diagnostics, the forward walker) sees the
/// concrete type without any request-context plumbing. Returns `loaded`
/// unchanged when the class is not one of the two entry points, or when a
/// disk cannot be resolved to a concrete type.
pub(crate) fn patch_storage_disk_type(backend: &Backend, loaded: Arc<ClassInfo>) -> Arc<ClassInfo> {
    let fqn = loaded.fqn();
    if fqn.as_str() != FILESYSTEM_MANAGER_FQN && fqn.as_str() != STORAGE_FACADE_FQN {
        return loaded;
    }
    let Some(adapter) = configured_disk_type(backend) else {
        return loaded;
    };

    let mut patched = (*loaded).clone();
    let mut changed = false;

    for method in patched.methods.make_mut().iter_mut() {
        if !DISK_RETURNING_METHODS.contains(&method.name.as_str()) {
            continue;
        }
        if refine_disk_return_type(Arc::make_mut(method), &adapter) {
            changed = true;
        }
    }

    // `Storage`'s disk-returning methods exist only as `@method` tags on the
    // facade's class docblock, not as real methods.
    if let Some(doc) = patched.doc_members.as_ref()
        && doc
            .methods
            .iter()
            .any(|m| DISK_RETURNING_METHODS.contains(&m.name.as_str()))
    {
        let mut doc = (**doc).clone();
        for method in &mut doc.methods {
            if !DISK_RETURNING_METHODS.contains(&method.name.as_str()) {
                continue;
            }
            if refine_disk_return_type(Arc::make_mut(method), &adapter) {
                changed = true;
            }
        }
        patched.doc_members = Some(Arc::new(doc));
    }

    if changed { Arc::new(patched) } else { loaded }
}

/// Replace a method's return type with the concrete disk type, but only when
/// it honestly declares the bare `Filesystem` or `Cloud` contract. A
/// hand-written override with a different type is left untouched.
fn refine_disk_return_type(method: &mut MethodInfo, adapter: &PhpType) -> bool {
    let returns_contract = method
        .return_type
        .as_ref()
        .and_then(|rt| rt.class_name())
        .is_some_and(|name| {
            let name = name.trim_start_matches('\\');
            name == FILESYSTEM_CONTRACT_FQN || name == CLOUD_CONTRACT_FQN
        });
    if returns_contract {
        method.return_type = Some(adapter.clone());
    }
    returns_contract
}

/// The concrete type every configured disk resolves to, memoized on the
/// [`Backend`].
///
/// `None` means at least one disk could not be classified, so the declared
/// contract is left alone. The cache is cleared when a `config/` file or a
/// `Storage::extend()` registration changes (see
/// [`Backend::refresh_laravel_storage_drivers`]), so an edit takes effect
/// without a restart.
fn configured_disk_type(backend: &Backend) -> Option<PhpType> {
    if let Some(cached) = backend.storage_disk_type_cache.read().as_ref() {
        return cached.clone();
    }
    let trees = backend.cached_config_trees();
    let resolved = trees
        .iter()
        .find(|(prefix, _)| prefix == "filesystems")
        .and_then(|(_, tree)| disk_union(tree, &backend.laravel_storage_drivers.read()));
    *backend.storage_disk_type_cache.write() = Some(resolved.clone());
    resolved
}

/// The union of the types every disk under `filesystems.disks` is built from,
/// or `None` when any one of them cannot be classified.
///
/// Returns `None` when there are no disks to check, since that almost
/// certainly means the tree could not be read rather than a project that
/// genuinely configures none.
fn disk_union(tree: &ConfigNode, drivers: &LaravelStorageDriverIndex) -> Option<PhpType> {
    let names = tree.get(&["disks"])?.child_keys();
    if names.is_empty() {
        return None;
    }

    let mut types: Vec<PhpType> = Vec::new();
    for name in &names {
        let driver = tree.value_at(&["disks", name.as_str(), "driver"])?;
        let (values, dynamic) = driver.as_strings();
        if dynamic || values.len() != 1 {
            return None;
        }
        let ty = driver_type(&values[0], drivers)?;
        if !types.contains(&ty) {
            types.push(ty);
        }
    }

    match types.len() {
        0 | 1 => types.pop(),
        _ => Some(PhpType::union(types)),
    }
}

/// The concrete type a single driver name builds.
///
/// A `Storage::extend()` registration is consulted first because
/// `FilesystemManager::resolve()` checks `$this->customCreators` before the
/// framework's own `create*Driver()` methods, so a project may legitimately
/// replace a built-in driver.
fn driver_type(driver: &str, drivers: &LaravelStorageDriverIndex) -> Option<PhpType> {
    if let Some(ty) = drivers.get(driver) {
        return Some(ty.clone());
    }
    BUILTIN_DRIVERS
        .contains(&driver)
        .then(|| PhpType::named(atom(FILESYSTEM_ADAPTER_FQN)))
}

/// A single `Storage::extend('driver', closure)` registration recovered from
/// source.
#[derive(Clone, Debug)]
pub(crate) struct StorageDriverRegistration {
    /// The driver name the registration binds, exactly as written.
    /// `FilesystemManager::resolve()` looks the name up by identity, so no
    /// case folding is applied.
    pub driver: String,
    /// The type the registered closure builds, from its `: Type` annotation
    /// when it has one.  `None` until the backend infers it from the closure
    /// body, which is the usual shape (the documented `Storage::extend()`
    /// example returns an unannotated `new FilesystemAdapter(...)`).
    pub return_type: Option<PhpType>,
    /// Raw source text of the closure / arrow-function argument, kept so the
    /// backend can infer the return type from the body.
    pub closure_text: Option<String>,
    /// Byte offset of the closure argument in the file the registration was
    /// found in, so body inference resolves against real source positions.
    pub closure_offset: u32,
}

/// Extract every literal `Storage::extend('driver', closure)` registration
/// from a file's source.
///
/// Returns an empty vector unless the file mentions both `extend(` and one of
/// the two class names such a call can be written against (a cheap byte
/// pre-filter), so the parse is only paid for candidate files. `extend()` is
/// common enough on its own — `Validator::extend()`, the container's own
/// `extend()` — that the class name has to be part of the filter; an aliased
/// import still spells it out in its `use` statement.
///
/// Registrations whose driver name or closure is not a literal are skipped;
/// a disk configured for such a driver then leaves the declared contract
/// untouched, which is the gracefully degraded behaviour.
pub(crate) fn extract_storage_driver_registrations(
    content: &str,
) -> Vec<StorageDriverRegistration> {
    if memchr::memmem::find(content.as_bytes(), b"extend(").is_none()
        || !STORAGE_EXTEND_TARGETS.iter().any(|target| {
            let short = target.rsplit('\\').next().unwrap_or(target);
            memchr::memmem::find(content.as_bytes(), short.as_bytes()).is_some()
        })
    {
        return Vec::new();
    }

    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);

    let mut out = Vec::new();
    collect_extend_calls(Node::Program(program), &owned, content, &mut out);
    out
}

/// Recursively collect every `Storage::extend('driver', closure)` call whose
/// target resolves to the facade or the manager.
fn collect_extend_calls(
    node: Node<'_, '_>,
    resolved: &OwnedResolvedNames,
    content: &str,
    out: &mut Vec<StorageDriverRegistration>,
) {
    if let Node::StaticMethodCall(smc) = node
        && let ClassLikeMemberSelector::Identifier(ident) = &smc.method
        && bytes_to_str(ident.value).eq_ignore_ascii_case("extend")
        && resolve_target_fqn(smc.class, resolved)
            .is_some_and(|target| STORAGE_EXTEND_TARGETS.contains(&target.as_str()))
        && let Some(reg) = build_driver_registration(smc, resolved, content)
    {
        out.push(reg);
    }
    node.visit_children(|child| collect_extend_calls(child, resolved, content, out));
}

/// Build a [`StorageDriverRegistration`] from an `extend('driver', closure)`
/// argument list, or `None` when it does not match the supported literal
/// shape.
///
/// A `: Type` annotation is resolved through the file's `use` statements
/// here rather than kept as written: the type ends up on `FilesystemManager`'s
/// own methods, where a short name would be resolved against the wrong file.
fn build_driver_registration(
    smc: &StaticMethodCall<'_>,
    resolved: &OwnedResolvedNames,
    content: &str,
) -> Option<StorageDriverRegistration> {
    let mut args = smc.argument_list.arguments.iter();
    let driver = string_literal_value(args.next()?.value())?;
    if driver.is_empty() {
        return None;
    }
    let closure_expr = args.next()?.value();
    let (_, return_type_hint) = closure_signature(closure_expr)?;

    Some(StorageDriverRegistration {
        driver: driver.to_string(),
        return_type: return_type_hint
            .and_then(|rth| resolve_hint_target_fqn(&rth.hint, resolved))
            .map(|fqn| PhpType::named(atom(&fqn))),
        closure_text: expr_source_text(Some(closure_expr), content),
        closure_offset: closure_expr.span().start.offset,
    })
}

/// Project-wide index of `Storage::extend()` driver registrations, keyed by
/// driver name.
///
/// Stored on [`Backend`] and built alongside the macro index (both are
/// recovered from the same service-provider scan). `by_uri` is the source of
/// truth, so an edit to one file replaces just that file's registrations;
/// `merged` is the derived lookup consulted when a disk's driver is
/// classified.
#[derive(Default)]
pub(crate) struct LaravelStorageDriverIndex {
    by_uri: HashMap<String, Vec<StorageDriverRegistration>>,
    merged: HashMap<String, PhpType>,
}

impl LaravelStorageDriverIndex {
    /// Replace the registrations contributed by `uri`.  Passing an empty
    /// vector removes the file's contributions.  Call [`Self::rebuild`]
    /// afterwards to refresh the merged lookup map (deferred so a bulk build
    /// rebuilds once rather than per file).
    pub(crate) fn set_file(&mut self, uri: String, regs: Vec<StorageDriverRegistration>) {
        if regs.is_empty() {
            self.by_uri.remove(&uri);
        } else {
            self.by_uri.insert(uri, regs);
        }
    }

    /// Whether `uri` currently contributes any registrations.
    pub(crate) fn has_uri(&self, uri: &str) -> bool {
        self.by_uri.contains_key(uri)
    }

    /// Whether the merged map holds no drivers at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.merged.is_empty()
    }

    /// The type the closure registered for `driver` builds, if known.
    fn get(&self, driver: &str) -> Option<&PhpType> {
        self.merged.get(driver)
    }

    /// Rebuild the merged lookup from the per-file registrations.
    ///
    /// A registration whose return type is not a class type contributes
    /// nothing: the disk it backs then stays unclassified and the declared
    /// contract survives, rather than a scalar or `mixed` standing in for a
    /// filesystem.  Two files registering the same driver keep the one from
    /// the first URI in sort order, so a rebuild never flips the disk type
    /// on hash-iteration order alone.
    pub(crate) fn rebuild(&mut self) {
        let mut uris: Vec<&String> = self.by_uri.keys().collect();
        uris.sort_unstable();

        let mut merged: HashMap<String, PhpType> = HashMap::new();
        for regs in uris.iter().filter_map(|uri| self.by_uri.get(*uri)) {
            for reg in regs {
                let Some(ty) = reg.return_type.as_ref() else {
                    continue;
                };
                if ty.class_name().is_none() {
                    continue;
                }
                merged
                    .entry(reg.driver.clone())
                    .or_insert_with(|| ty.clone());
            }
        }
        self.merged = merged;
    }
}

// ─── Storage facade name resolution ─────────────────────────────────────────

/// The local names Laravel's `Storage` facade answers to in a file.
///
/// Resolution is textual because the same question has to be answered for the
/// mid-edit buffer completion runs on and for the parsed file the symbol map
/// walks; an AST is only available on one of those. The result is a short list
/// (usually one name), so callers hold on to it for the file rather than
/// re-scanning per call site.
///
/// A file with no `namespace` declaration reaches the facade through Laravel's
/// global class alias, so the bare short name counts there unless another
/// import has taken it.
pub(crate) fn storage_facade_local_names(content: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut short_name_taken = false;

    crate::text_scan::for_each_class_import(content, &mut |imported, local| {
        if imported.eq_ignore_ascii_case(STORAGE_FACADE_FQN) {
            names.push(local.to_string());
        } else if local.eq_ignore_ascii_case("Storage") {
            short_name_taken = true;
        }
    });

    if !short_name_taken
        && !names
            .iter()
            .any(|name| name.eq_ignore_ascii_case("Storage"))
        && !crate::text_scan::source_declares_namespace(content)
    {
        names.push("Storage".to_string());
    }
    names
}

/// Whether a written class name is Laravel's `Storage` facade, given the
/// local names [`storage_facade_local_names`] found for the file.
pub(crate) fn is_storage_facade_name(class_name: &str, local_names: &[String]) -> bool {
    let is_root_qualified = class_name.starts_with('\\');
    let class_name = class_name.trim_start_matches('\\');
    if class_name.eq_ignore_ascii_case(STORAGE_FACADE_FQN) {
        return true;
    }
    if class_name.contains('\\') {
        return false;
    }
    if is_root_qualified {
        // `\Storage` names the global alias whatever the file imports.
        return class_name.eq_ignore_ascii_case("Storage");
    }
    local_names
        .iter()
        .any(|local| local.eq_ignore_ascii_case(class_name))
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
