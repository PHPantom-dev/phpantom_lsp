//! Call-site variable inference for Blade templates.
//!
//! For templates without a declared signature (`@bladestan-signature`
//! or plain `@var` docblocks), infer the variables a template receives
//! from the call sites that reference it: `view()`/`View::make()` calls
//! (literal array keys, `compact()` arguments, `->with()` chains — see
//! [`extract_call_site_vars`]), the `@include`/`@each` family in the
//! templates that render it, and, for a component, the attributes each
//! `<x-…>` tag passes (see [`super::component_tags`]). The inferred set
//! is injected into the template's virtual-PHP prologue as `@var`
//! docblock declarations (see `preprocess_with_vars`), so every
//! consumer — completion, hover, go-to-definition, and the
//! undefined-variable diagnostic — sees them through the ordinary
//! resolution pipeline.
//!
//! This is deliberately the lowest-priority source: an in-template
//! `@var` annotation shadows an injected one (it sits closer to every
//! use site in the backward docblock scan), `@props`/`@aware`, a
//! component's backing class (see `super::backing_class`), and a
//! provider's shared and composed data (see `super::shared_vars`) win
//! over it per name, and templates that declare a signature are skipped
//! entirely. Types are "true for the callers we found": multiple call
//! sites union per variable, and dynamic view names contribute nothing.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;
use crate::php_type::PhpType;
use crate::symbol_map::{LaravelStringKind, SymbolKind, SymbolMap};
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::types::ClassInfo;
use crate::virtual_members::laravel::canonical_view_name;

pub(crate) use super::view_call_walker::string_literal_contents;
use super::view_call_walker::{
    BladeDirectiveCollectCtx, BladeDirectiveWalker, CollectCtx, SiteDraft, SiteEntry,
    ViewCallWalker, data_shape_entries, each_variable_type, qualify_class_names,
};

/// A variable passed to a template at one call site: the name (without
/// `$`) and the expression's resolved type.
type InferredVars = Vec<(String, PhpType)>;

/// A byte range `[start, end)` in a caller file.
pub(crate) type ByteRange = (u32, u32);

/// One variable a `view()` call site passes, with the ranges that let a
/// diagnostic point at the key and at the value independently.
pub(crate) struct PassedVar {
    pub(crate) name: String,
    pub(crate) ty: PhpType,
    /// The key that named the variable — an array key, a `compact()`
    /// argument, or the `->withName()` method name.
    pub(crate) key_range: ByteRange,
    /// The expression that produced the value.
    pub(crate) value_range: ByteRange,
    /// Whether Blade binds the variable itself rather than the call site
    /// naming it, as `@each` does with `$key`.  Its type is still the
    /// call's to answer for, but a template with no use for it is not
    /// being handed something unwanted.
    pub(crate) framework_bound: bool,
}

/// One resolved `view()` / `View::make()` / `@include` call site.
pub(crate) struct ResolvedViewCall {
    /// The view-name string's contents, matching the offsets the symbol
    /// map records for a Laravel view key.
    pub(crate) name_range: ByteRange,
    pub(crate) vars: Vec<PassedVar>,
    /// Whether every data source at the site was readable, so [`Self::vars`]
    /// is everything the caller hands the template. A `view($name, $data)`
    /// whose data is a variable passes an unknown set, and neither a
    /// missing nor an unwanted name can be concluded from it.
    pub(crate) complete: bool,
    /// Whether the render hands the template the scope it is written in on
    /// top of [`Self::vars`], as `@include` does. False for `@each`, whose
    /// partial sees only the item and the key however much the surrounding
    /// template holds.
    pub(crate) forwards_scope: bool,
}

/// The variables injected into one template's virtual-PHP prologue:
/// (name without `$`, docblock type string).
pub(crate) type InjectedVars = Vec<(String, String)>;

/// What a template's virtual PHP is seeded with beyond the template's own
/// source: the variables its prologue declares, and the class its `$this`
/// is bound to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BladeScope {
    /// Highest-priority source first — the prologue declares the first
    /// entry for a name and skips the rest.
    pub vars: InjectedVars,
    /// The fully qualified name of the component instance a Livewire view
    /// renders with, which the preprocessor wraps the body in a method of
    /// (see `preprocess_with_vars`).  `None` for every other template.
    pub this_class: Option<String>,
    /// The class behind each `<x-…>` / `<livewire:…>` tag the template
    /// renders, and the call its attributes fill, sorted by tag as
    /// written.  Carried here rather than looked up while preprocessing
    /// so that resolving a tag never puts a walk of the project's view
    /// roots on the edit path, and so that a tag that starts resolving
    /// (or a component whose signature changes) after the workspace index
    /// finishes re-parses the template that renders it.
    pub components: Vec<(String, crate::blade::preprocessor::ComponentTarget)>,
}

/// User files that render Blade views, with their symbol maps. Shared across
/// a whole refresh pass so the workspace is walked once, not once per
/// template. Headless analysis also carries a compact per-view candidate
/// index because it deliberately does not build the workspace reference
/// index used by the editor.
pub(crate) struct ViewCallerSnapshot {
    files: Vec<(String, Arc<SymbolMap>)>,
    local_candidates: Option<HashMap<String, Vec<usize>>>,
}

/// Every Blade file's raw source, snapshotted once per refresh pass.
///
/// Unlike [`ViewCallerSnapshot`], there is no pre-built index of `<x-…>`
/// tag usages to filter by first (component tags are HTML, not something
/// the symbol map extracts), so finding a component's callers means
/// scanning every Blade file's content. Sharing this list across a whole
/// refresh pass keeps that scan at O(templates) rather than
/// O(templates × templates).
pub(crate) type BladeCallerSnapshot = Vec<(String, Arc<String>)>;

/// The parameter names a tag's attributes fill, from the targets a
/// template's virtual PHP was built with.
///
/// `None` when the tag names no component the preprocessor could build,
/// so none of its attributes were arguments.
fn component_argument_names(
    components: &[(String, crate::blade::preprocessor::ComponentTarget)],
    tag: &str,
) -> Option<Vec<String>> {
    use crate::blade::preprocessor::ComponentBinding;

    let (_, target) = components.iter().find(|(known, _)| known == tag)?;
    match &target.binding {
        ComponentBinding::Construct(parameters) | ComponentBinding::Mount(parameters) => {
            Some(parameters.iter().map(|param| param.name.clone()).collect())
        }
        ComponentBinding::Declare => None,
    }
}

/// Append the entries whose names nothing has declared yet, so the
/// highest-priority source to carry a name is the one that keeps it.
fn push_undeclared(declared: &mut InjectedVars, vars: InjectedVars) {
    for (name, ty) in vars {
        if declared.iter().any(|(existing, _)| existing == &name) {
            continue;
        }
        declared.push((name, ty));
    }
}

/// Deduplicate and union one variable's types across every call site that
/// passed it. Sorts the deduplicated members by their rendered form first,
/// so the result does not depend on the order the call sites were visited
/// in (the snapshot they come from is built from a `HashMap`, whose
/// iteration order varies across runs).
fn join_call_site_types(types: Vec<PhpType>) -> PhpType {
    let mut unique: Vec<PhpType> = Vec::new();
    for ty in types {
        if !unique.iter().any(|u| u.equivalent(&ty)) {
            unique.push(ty);
        }
    }
    unique.sort_by_key(|a| a.to_string());
    if unique.len() == 1 {
        unique.pop().unwrap()
    } else {
        PhpType::union(unique)
    }
}

/// The canonical spelling of a template path, for comparing against a
/// canonical view root.
///
/// A template that has just been deleted has no canonical form of its
/// own, so its directory is canonicalized instead: the file still has to
/// resolve to the view name it had, or the callers that render it are
/// left holding a name nothing answers to.
fn canonical_path_for_comparison(path: &std::path::Path) -> std::path::PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    match (
        path.parent().and_then(|parent| parent.canonicalize().ok()),
        path.file_name(),
    ) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => path.to_path_buf(),
    }
}

impl Backend {
    /// Compute the variables to inject into a Blade template's virtual
    /// PHP: the members of the class backing a component view (see
    /// [`super::backing_class`]), what the layouts it `@extends` declare
    /// (see [`super::layout`]), the variables a service provider shares
    /// or composes into its scope (see [`super::shared_vars`]), then the
    /// variables its `view()` call sites and, for a component, its `<x-…>`
    /// tag call sites pass.
    ///
    /// Returns pairs of (variable name without `$`, docblock type
    /// string), highest-priority source first — the prologue declares
    /// the first entry for a name and skips the rest — alongside the
    /// class the template's `$this` is bound to.  Empty when the
    /// template has no backing class, extends nothing, and no call site
    /// references it, or when the template's view name cannot be derived
    /// from its path.
    pub(crate) fn compute_blade_injected_vars(
        &self,
        uri: &str,
        blade_content: &str,
        shared: Option<&ViewCallerSnapshot>,
        shared_blade: Option<&BladeCallerSnapshot>,
    ) -> BladeScope {
        // The tags this template renders resolve whatever its own view
        // name turns out to be — a template outside every view root still
        // renders components.
        let components = self.resolve_component_tags(blade_content);

        let view_names = self.view_names_for_blade_uri(uri);
        if view_names.is_empty() {
            return BladeScope {
                components,
                ..BladeScope::default()
            };
        }

        // The backing class is a *declared* source, so it stands whatever
        // else the template says; only the names its own signature
        // declares win over it (the preprocessor applies that).
        let (mut declared, this_class) = self.blade_backing_class_vars(&view_names);

        // The layout the template `@extends` is rendered from the same data
        // the template is, so what it declares the template receives too
        // (see [`super::layout`]).
        push_undeclared(&mut declared, self.blade_layout_vars(blade_content));

        // What a service provider shares or composes into this template's
        // scope: no template declares it and no caller passes it, but it is
        // still written down somewhere, so it beats inference (see
        // [`super::shared_vars`]).
        push_undeclared(&mut declared, self.blade_provider_vars(&view_names));

        // A template that declares a signature manages its own contract;
        // inferring on top would fight the declared types.
        if crate::blade::signature::has_declared_signature(blade_content) {
            return BladeScope {
                vars: declared,
                this_class,
                components,
            };
        }

        // Find every file whose symbol map contains a View string key
        // matching one of this template's names.
        let keys: Vec<crate::reference_index::ReferenceIndexKey> = view_names
            .iter()
            .map(
                |name| crate::reference_index::ReferenceIndexKey::LaravelString {
                    kind: LaravelStringKind::View,
                    key: name.clone(),
                },
            )
            .collect();
        let own_snapshot;
        let snapshot = match shared {
            Some(shared) => shared.files.as_slice(),
            None => {
                // Never trigger (or wait on) workspace indexing from here:
                // this runs while a Blade file is being opened or a
                // controller saved, and a keystroke must not pay for a
                // workspace walk.  Before the index is ready this scans
                // whatever is parsed; the post-index refresh pass picks up
                // call sites discovered later.
                own_snapshot = self.user_file_symbol_maps_for_reference_keys_nonblocking(&keys);
                own_snapshot.as_slice()
            }
        };
        // A shared snapshot holds every file in the workspace that renders
        // any view. Use its compact local index during headless analysis and
        // the workspace reference index in the editor, so each template only
        // reads callers that can name it. Before editor indexing completes,
        // `None` conservatively reads the parsed snapshot whole.
        let local_candidates = shared.and_then(|snapshot| snapshot.local_candidates.as_ref());
        let candidates = shared
            .filter(|_| local_candidates.is_none())
            .and_then(|_| self.reference_candidate_uris_for_keys(&keys));

        // Union the variables from every call site, per name.
        let mut merged: HashMap<String, Vec<PhpType>> = HashMap::new();
        for (snapshot_index, (file_uri, snapshot_map)) in snapshot.iter().enumerate() {
            // A template must not feed itself: a recursive `@include` names
            // the template the spans it would be read from belong to.
            if file_uri == uri {
                continue;
            }
            if let Some(local_candidates) = local_candidates
                && !view_names.iter().any(|name| {
                    local_candidates
                        .get(name)
                        .is_some_and(|indices| indices.binary_search(&snapshot_index).is_ok())
                })
            {
                continue;
            }
            if let Some(candidates) = &candidates
                && !candidates.contains(file_uri.as_str())
            {
                continue;
            }
            // A Blade caller's map indexes its virtual PHP, and the refresh
            // pass rewrites that as it re-infers templates, so the
            // snapshot's copy can describe a text that no longer exists.
            let is_blade = self.is_blade_file(file_uri);
            let symbol_map = match is_blade {
                true => match self.symbol_maps.read().get(file_uri) {
                    Some(map) => Arc::clone(map),
                    None => continue,
                },
                false => Arc::clone(snapshot_map),
            };
            // A render site whose receiver only a type settles is not in
            // the map, so ask for the file's confirmed extras — but only
            // when a candidate names one of *this* template's views, since
            // the loop below keeps no other key anyway.  Confirming a
            // candidate resolves its receiver's type, and a file that
            // spells many candidates it will never confirm (`$xw->text(…)`
            // on an `XMLWriter` reads as a mailable's `text()` until the
            // receiver is resolved) would otherwise pay for all of them
            // here, in the serial refresh pass, rather than in the
            // parallel diagnostic pass that has a warm scope cache.
            let has_candidate = symbol_map
                .view_receiver_sites
                .iter()
                .any(|site| view_names.contains(&site.key));
            let extra = if has_candidate {
                self.typed_receiver_view_spans_for(file_uri, &symbol_map)
            } else {
                Arc::new(Vec::new())
            };
            let offsets: Vec<u32> = symbol_map
                .spans
                .iter()
                .chain(extra.iter())
                .filter_map(|span| match &span.kind {
                    SymbolKind::LaravelStringKey {
                        kind: LaravelStringKind::View,
                        key,
                        ..
                    } if view_names.iter().any(|n| n == key) => Some(span.start),
                    _ => None,
                })
                .collect();
            if offsets.is_empty() {
                continue;
            }
            // Read only once the caller is known to name this template: a
            // Blade caller's text is its whole virtual PHP, which is far too
            // much to copy for every template in the project.
            let Some(content) = self.caller_source(file_uri, is_blade, &symbol_map) else {
                continue;
            };
            for site in self.extract_call_site_vars(file_uri, &content, &offsets) {
                for var in site.vars {
                    merged.entry(var.name).or_default().push(var.ty);
                }
            }
        }

        // The attributes each `<x-…>` tag passes, for a template
        // addressable as a component tag (`components.*`, a namespaced
        // view name, or a directory a provider registered a tag prefix
        // for — see `component_tags::component_tag_names`).
        let tag_names = crate::blade::component_tags::component_tag_names(
            &view_names,
            &self.anonymous_component_namespaces(),
        );
        if !tag_names.is_empty() {
            let own_blade_snapshot;
            let blade_snapshot = match shared_blade {
                Some(shared) => shared.as_slice(),
                None => {
                    own_blade_snapshot = self.blade_caller_snapshot();
                    own_blade_snapshot.as_slice()
                }
            };
            let needles = crate::blade::component_tags::component_tag_needles(&tag_names);
            for (file_uri, content) in blade_snapshot {
                if file_uri == uri {
                    continue;
                }
                if !crate::blade::component_tags::may_contain_component_tag(content, &needles) {
                    continue;
                }
                // The same partition the caller's own virtual PHP was
                // built with, so the scan agrees with it about which
                // attributes became arguments of the tag's call and are
                // therefore not `blade_bound_attr_directive` calls to count.
                let caller_components = self
                    .blade_injected_vars
                    .read()
                    .get(file_uri)
                    .map(|scope| scope.components.clone())
                    .unwrap_or_default();
                let occurrences = crate::blade::component_tags::scan_component_tag_calls(
                    content,
                    &tag_names,
                    &|tag| component_argument_names(&caller_components, tag),
                );
                if occurrences.is_empty() {
                    continue;
                }
                let Some(virtual_php) = self.blade_virtual_content.read().get(file_uri).cloned()
                else {
                    continue;
                };
                for vars in
                    self.extract_component_call_site_vars(file_uri, &virtual_php, occurrences)
                {
                    for (name, ty) in vars {
                        merged.entry(name).or_default().push(ty);
                    }
                }
            }
        }

        if merged.is_empty() {
            return BladeScope {
                vars: declared,
                this_class,
                components,
            };
        }

        // A name a declared source already carries needs no inference: what
        // the backing class holds, and what a provider writes into the view's
        // data, beat what one caller happened to pass.
        merged.retain(|name, _| !declared.iter().any(|(existing, _)| existing == name));

        let mut result: Vec<(String, String)> = merged
            .into_iter()
            .map(|(name, types)| (name, join_call_site_types(types).to_string()))
            .collect();
        // Deterministic prologue ordering so re-preprocessing an
        // unchanged template produces identical virtual PHP.
        result.sort_by(|a, b| a.0.cmp(&b.0));
        // The declared sources lead, so theirs are the declarations the
        // prologue emits for the names more than one source carries.
        let mut vars = declared;
        vars.extend(result);
        BladeScope {
            vars,
            this_class,
            components,
        }
    }

    /// Re-run call-site inference for already-preprocessed Blade
    /// templates and re-parse the ones whose inferred variable set
    /// changed.
    ///
    /// Parse order is arbitrary: a template preprocessed before its
    /// controllers were indexed saw no call sites.  Run this after a
    /// pass that parses many files (workspace indexing, the analyse
    /// CLI's parse phase) or after a controller edit, so templates pick
    /// up call sites discovered since they were preprocessed.  Cheap
    /// for templates whose inference is unchanged (no re-parse).
    pub(crate) fn refresh_blade_injected_vars(&self) {
        let blade_uris: Vec<String> = self.blade_virtual_content.read().keys().cloned().collect();
        if blade_uris.is_empty() {
            return;
        }
        // Snapshot the caller files once for the whole pass.  Letting each
        // template take its own snapshot walks every symbol map (and, for
        // component tags, every Blade file) in the workspace per
        // template, which is quadratic in a project with hundreds of
        // templates.
        let shared = self.view_caller_snapshot();
        let shared_blade = self.blade_caller_snapshot();
        for uri in self.blade_render_order(blade_uris) {
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            self.reinfer_and_reparse_blade_with(&uri, &content, Some(&shared), Some(&shared_blade));
        }
    }

    /// The templates of a refresh pass, ordered so that a template is
    /// re-inferred after every template that renders it.
    ///
    /// A partial's inferred types are read out of the rendering template's
    /// virtual PHP, so that template's own scope has to be settled first:
    /// an `@include('partials.row', ['row' => $row])` inside a
    /// `@foreach ($rows as $row)` only types `$row` once the rendering
    /// template knows what `$rows` holds.
    ///
    /// Templates that render each other have no such order.  Each is read
    /// against the other's scope as the previous pass left it, and the tie
    /// is broken by URI so that the pass is reproducible rather than
    /// oscillating between two answers.
    fn blade_render_order(&self, mut uris: Vec<String>) -> Vec<String> {
        // The snapshot comes from a `HashMap`, so the tie-break is only a
        // tie-break once the input itself is in a fixed order.
        uris.sort_unstable();

        let mut rendered_by_name: HashMap<String, usize> = HashMap::new();
        for (index, uri) in uris.iter().enumerate() {
            for name in self.view_names_for_blade_uri(uri) {
                rendered_by_name.insert(canonical_view_name(&name).into_owned(), index);
            }
        }

        let anonymous = self.anonymous_component_namespaces();
        let mut renders: Vec<Vec<usize>> = vec![Vec::new(); uris.len()];
        let mut renderers: Vec<usize> = vec![0; uris.len()];
        for (index, uri) in uris.iter().enumerate() {
            for name in self.blade_rendered_view_names(uri, &anonymous) {
                let Some(&target) = rendered_by_name.get(canonical_view_name(&name).as_ref())
                else {
                    continue;
                };
                if target == index || renders[index].contains(&target) {
                    continue;
                }
                renders[index].push(target);
                renderers[target] += 1;
            }
        }

        let mut order: Vec<usize> = Vec::with_capacity(uris.len());
        let mut ready: VecDeque<usize> = (0..uris.len())
            .filter(|index| renderers[*index] == 0)
            .collect();
        while let Some(index) = ready.pop_front() {
            order.push(index);
            for target in std::mem::take(&mut renders[index]) {
                renderers[target] -= 1;
                if renderers[target] == 0 {
                    ready.push_back(target);
                }
            }
        }
        // A template rendered from a cycle never runs out of renderers, so
        // whatever the walk did not reach follows it in URI order.
        let placed: HashSet<usize> = order.iter().copied().collect();
        order.extend((0..uris.len()).filter(|index| !placed.contains(index)));

        order
            .into_iter()
            .map(|index| std::mem::take(&mut uris[index]))
            .collect()
    }

    /// The view names one Blade template renders: the ones its compiled
    /// `@include` / `@each` / `@extends` family names, and the ones the
    /// component each `<x-…>` tag addresses is addressable by.
    fn blade_rendered_view_names(
        &self,
        uri: &str,
        anonymous: &[crate::blade::component_tags::AnonymousNamespace],
    ) -> Vec<String> {
        let mut names: Vec<String> = self
            .symbol_maps
            .read()
            .get(uri)
            .map(|map| {
                map.spans
                    .iter()
                    .filter_map(|span| match &span.kind {
                        SymbolKind::LaravelStringKey {
                            kind: LaravelStringKind::View,
                            key,
                            ..
                        } => Some(key.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(content) = self.get_file_content_arc(uri) {
            for tag in crate::blade::component_tags::referenced_component_tags(&content) {
                names.extend(crate::blade::component_tags::view_names_for_component_tag(
                    &tag, anonymous,
                ));
            }
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    /// The text one caller file's symbol map offsets index: a Blade
    /// template's virtual PHP, and any other file's own source.
    ///
    /// `None` for a Blade caller whose map and virtual PHP disagree, which
    /// drops the caller rather than reading its offsets against the wrong
    /// buffer: re-inferring a template rewrites its prologue, so the map a
    /// reader holds may have been built for a shorter text than the one the
    /// pass has since written.
    fn caller_source(&self, uri: &str, is_blade: bool, map: &SymbolMap) -> Option<String> {
        if !is_blade {
            return self.get_file_content(uri);
        }
        let content = self.blade_virtual_content.read().get(uri).cloned()?;
        map.matches_source(&content).then_some(content)
    }

    /// Every parsed user file that renders at least one Blade view, with
    /// its symbol map.
    ///
    /// Templates count as callers: an `@include` is a render site like any
    /// other, and the offsets its spans carry index the template's virtual
    /// PHP, which is what [`Self::caller_source`] hands back for one.
    fn view_caller_snapshot(&self) -> ViewCallerSnapshot {
        let vendor_prefixes = self.workspace.vendor_uri_prefixes.lock().clone();
        let maps = self.symbol_maps.read();
        let mut files: Vec<(String, Arc<SymbolMap>)> = maps
            .iter()
            .filter(|(uri, map)| {
                !uri.starts_with("phpantom-stub://")
                    && !uri.starts_with("phpantom-stub-fn://")
                    && !vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
                    && (!map.view_receiver_sites.is_empty()
                        || map.spans.iter().any(|span| {
                            matches!(
                                &span.kind,
                                SymbolKind::LaravelStringKey {
                                    kind: LaravelStringKind::View,
                                    ..
                                }
                            )
                        }))
            })
            .map(|(uri, map)| (uri.clone(), Arc::clone(map)))
            .collect();
        // Deterministic order so the whole inference pass (and its
        // per-name type unions) is reproducible across runs, not just
        // the caller's `HashMap` iteration order.
        files.sort_by(|(a, _), (b, _)| a.cmp(b));

        let local_candidates = self.skip_reference_index.then(|| {
            let mut candidates: HashMap<String, Vec<usize>> = HashMap::new();
            for (index, (_, map)) in files.iter().enumerate() {
                let mut names: Vec<&str> = map
                    .spans
                    .iter()
                    .filter_map(|span| match &span.kind {
                        SymbolKind::LaravelStringKey {
                            kind: LaravelStringKind::View,
                            key,
                            ..
                        } => Some(key.as_str()),
                        _ => None,
                    })
                    .chain(map.view_receiver_sites.iter().map(|site| site.key.as_str()))
                    .collect();
                names.sort_unstable();
                names.dedup();
                for name in names {
                    candidates.entry(name.to_string()).or_default().push(index);
                }
            }
            candidates
        });

        ViewCallerSnapshot {
            files,
            local_candidates,
        }
    }

    /// Every known Blade file's raw source, for the component-tag scan in
    /// [`Self::compute_blade_injected_vars`]. Unlike [`Self::view_caller_snapshot`]
    /// this cannot pre-filter by symbol-map spans (component tags are HTML,
    /// not something the symbol map extracts), so it just snapshots every
    /// Blade file once per refresh pass.
    fn blade_caller_snapshot(&self) -> BladeCallerSnapshot {
        let vendor_prefixes = self.workspace.vendor_uri_prefixes.lock().clone();
        let uris: Vec<String> = self.blade_virtual_content.read().keys().cloned().collect();
        uris.into_iter()
            .filter(|uri| !vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str())))
            .filter_map(|uri| {
                let content = self.get_file_content_arc(&uri)?;
                Some((uri, content))
            })
            .collect()
    }

    /// Re-infer one template on its own, taking its own caller snapshot.
    /// For the single-template triggers (opening a Blade file, saving a
    /// controller) rather than a bulk refresh pass.
    pub(crate) fn reinfer_and_reparse_blade(&self, uri: &str, content: &str) -> bool {
        self.reinfer_and_reparse_blade_with(uri, content, None, None)
    }

    /// Re-infer one template and pass what it holds on to the templates it
    /// renders, for the open of a Blade file.
    ///
    /// The renders matter as much as the template itself here: a partial
    /// preprocessed before the page that `@include`s it was ever parsed read
    /// its own scope off a page that did not exist yet, and opening the page
    /// is the point that answer becomes available.
    pub(crate) fn reinfer_blade_and_its_renders(&self, uri: &str, content: &str) {
        if self.reinfer_and_reparse_blade(uri, content) {
            self.schedule_diagnostics(uri.to_string());
        }
        self.refresh_blade_render_targets(vec![uri.to_string()]);
    }

    /// Recompute one template's inferred variable set; when it differs
    /// from the cached set, overwrite the cache and re-parse the
    /// template (`update_ast` reads the cache, so it must be written
    /// first).  A missing cache entry counts as empty, matching what
    /// `update_ast` injects on a cache miss.
    fn reinfer_and_reparse_blade_with(
        &self,
        uri: &str,
        content: &str,
        shared: Option<&ViewCallerSnapshot>,
        shared_blade: Option<&BladeCallerSnapshot>,
    ) -> bool {
        let fresh = self.compute_blade_injected_vars(uri, content, shared, shared_blade);
        let unchanged = match self.blade_injected_vars.read().get(uri) {
            Some(prev) => *prev == fresh,
            None => fresh == BladeScope::default(),
        };
        if unchanged {
            return false;
        }
        self.blade_injected_vars
            .write()
            .insert(uri.to_string(), fresh);
        self.update_ast(uri, content);
        true
    }

    /// Re-run call-site inference for the templates referenced by one
    /// caller file (after it was edited or re-indexed), so an updated
    /// `view()` call is reflected in the template without waiting for
    /// the template's own next parse.
    ///
    /// Only templates that are already preprocessed are refreshed; a
    /// template parsed for the first time later runs inference itself.
    pub(crate) fn refresh_blade_inference_for_caller(&self, caller_uri: &str) {
        if self.is_blade_file(caller_uri) {
            self.refresh_blade_render_targets(vec![caller_uri.to_string()]);
            self.refresh_blade_layout_children(caller_uri);
            return;
        }
        let Some(map) = self.symbol_maps.read().get(caller_uri).cloned() else {
            return;
        };
        let extra = self.typed_receiver_view_spans_for(caller_uri, &map);
        let mut names: Vec<&str> = map
            .spans
            .iter()
            .chain(extra.iter())
            .filter_map(|span| match &span.kind {
                SymbolKind::LaravelStringKey {
                    kind: LaravelStringKind::View,
                    key,
                    ..
                } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        if names.is_empty() {
            return;
        }
        names.sort_unstable();
        names.dedup();

        let mut changed: Vec<String> = Vec::new();
        for name in names {
            for location in crate::virtual_members::laravel::resolve_laravel_string_key(
                self,
                &LaravelStringKind::View,
                name,
                caller_uri,
            ) {
                let template_uri = location.uri.to_string();
                if !self
                    .blade_virtual_content
                    .read()
                    .contains_key(&template_uri)
                {
                    continue;
                }
                let Some(content) = self.get_file_content(&template_uri) else {
                    continue;
                };
                if self.reinfer_and_reparse_blade(&template_uri, &content) {
                    self.schedule_diagnostics(template_uri.clone());
                    changed.push(template_uri);
                }
            }
        }
        // A template whose own scope moved hands different data to the
        // partials it renders, so the edit follows the renders down.
        self.refresh_blade_render_targets(changed);
    }

    /// The Blade-caller equivalent of [`Self::refresh_blade_inference_for_caller`]:
    /// re-run inference for the templates a *Blade* file renders — the
    /// partials its `@include` family names and the components its `<x-…>`
    /// tags address — so an updated attribute or `@include` array reaches
    /// them without waiting for their own next parse.
    ///
    /// The walk follows whatever changed: a partial handed a new type passes
    /// different data on to the partials *it* renders.  Each template is
    /// visited once, which is what keeps two templates that render each
    /// other from handing the work back and forth.
    fn refresh_blade_render_targets(&self, from: Vec<String>) {
        if from.is_empty() {
            return;
        }
        let anonymous = self.anonymous_component_namespaces();
        let mut visited: HashSet<String> = from.iter().cloned().collect();
        let mut pending = from;
        while let Some(caller_uri) = pending.pop() {
            for name in self.blade_rendered_view_names(&caller_uri, &anonymous) {
                for location in crate::virtual_members::laravel::resolve_laravel_string_key(
                    self,
                    &LaravelStringKind::View,
                    &name,
                    &caller_uri,
                ) {
                    let template_uri = location.uri.to_string();
                    if !visited.insert(template_uri.clone()) {
                        continue;
                    }
                    if !self
                        .blade_virtual_content
                        .read()
                        .contains_key(&template_uri)
                    {
                        continue;
                    }
                    let Some(template_content) = self.get_file_content(&template_uri) else {
                        continue;
                    };
                    if self.reinfer_and_reparse_blade(&template_uri, &template_content) {
                        self.schedule_diagnostics(template_uri.clone());
                        pending.push(template_uri);
                    }
                }
            }
        }
    }

    /// Re-run inference for the templates whose layout chain runs through
    /// a Blade file, after it was edited or saved, so a `@var` added to a
    /// layout reaches its children without waiting for each child's own
    /// next parse.
    ///
    /// The walk goes *down* the chain rather than reading every template's
    /// ancestors: each template's own `@extends` target is read once, then
    /// the set of affected view names grows a level per round until no
    /// template joins it. A template that extends a template that extends
    /// the edited layout inherits from it too.
    fn refresh_blade_layout_children(&self, layout_uri: &str) {
        let mut frontier = self.view_names_for_blade_uri(layout_uri);
        if frontier.is_empty() {
            return;
        }
        let mut pending: Vec<(String, Arc<String>, Vec<String>)> = self
            .blade_caller_snapshot()
            .into_iter()
            .filter(|(uri, _)| uri != layout_uri)
            .filter_map(|(uri, content)| {
                let extends = crate::blade::signature::extract_extends(&content);
                (!extends.is_empty()).then_some((uri, content, extends))
            })
            .collect();

        while !frontier.is_empty() && !pending.is_empty() {
            let (children, rest): (Vec<_>, Vec<_>) =
                pending.into_iter().partition(|(_, _, extends)| {
                    extends
                        .iter()
                        .any(|extends| frontier.iter().any(|name| name == extends))
                });
            pending = rest;
            frontier = Vec::new();
            for (uri, content, _) in children {
                frontier.extend(self.view_names_for_blade_uri(&uri));
                if self.reinfer_and_reparse_blade(&uri, &content) {
                    self.schedule_diagnostics(uri);
                }
            }
        }
    }

    /// Derive the view names a Blade file is addressable by: one per
    /// configured view root that contains it, in dot notation, plus
    /// `namespace::name` forms for provider-registered directories.
    pub(crate) fn view_names_for_blade_uri(&self, uri: &str) -> Vec<String> {
        let Ok(url) = tower_lsp::lsp_types::Url::parse(uri) else {
            return Vec::new();
        };
        let Ok(path) = url.to_file_path() else {
            return Vec::new();
        };

        let mut names = Vec::new();
        let mut push_name = |rel: &std::path::Path, namespace: &str| {
            let rel_str = rel.to_string_lossy();
            let stripped = rel_str
                .strip_suffix(".blade.php")
                .or_else(|| rel_str.strip_suffix(".php"));
            if let Some(stem) = stripped {
                let name = stem.replace(['/', '\\'], ".");
                if namespace.is_empty() {
                    names.push(name);
                } else {
                    names.push(format!("{namespace}::{name}"));
                }
            }
        };

        // Each root is tried against the raw spelling first, so the common
        // case costs no filesystem calls, and only falls back to canonical
        // spellings when the raw ones do not line up.  Canonicalizing the
        // template instead of trying it raw would lose one that is itself
        // a symlink into a shared directory, since that resolves out of
        // the view root it sits under.
        let canonical = std::cell::OnceCell::new();
        let mut match_root = |root: &std::path::Path, namespace: &str| {
            if let Ok(rel) = path.strip_prefix(root) {
                push_name(rel, namespace);
                return;
            }
            // A view root can be relative when the workspace root was
            // given relative (the analyse CLI passes `--project-root`
            // through as-is), while `path` came from a file URI and is
            // always absolute.
            let Ok(root) = root.canonicalize() else {
                return;
            };
            if let Ok(rel) = path.strip_prefix(&root) {
                push_name(rel, namespace);
                return;
            }
            // The workspace itself can be reached under an alias: macOS
            // exposes the same directory through both `/var` and
            // `/private/var`, so a canonical root and a raw template path
            // describe the same tree under two names.
            let canonical = canonical.get_or_init(|| canonical_path_for_comparison(&path));
            if let Ok(rel) = canonical.strip_prefix(&root) {
                push_name(rel, namespace);
            }
        };

        for root in self.laravel_view_roots() {
            match_root(&root, "");
        }
        for res in &self.laravel_provider_resources.read().view_dirs {
            match_root(&res.path, &res.namespace);
        }
        names
    }

    /// Parse one caller file and extract the variables passed to the
    /// template at each `view('name', …)` span offset.
    ///
    /// `offsets` are the byte offsets of the view-name string contents
    /// (as recorded in the symbol map); a call site matches when the
    /// span of one of its string arguments starts at one of them.
    ///
    /// Everything a single site passes lands in one [`ResolvedViewCall`],
    /// including the entries a chained `->with(…)` adds, so a caller that
    /// builds its data over several calls is still judged as one.
    pub(crate) fn extract_call_site_vars(
        &self,
        uri: &str,
        content: &str,
        offsets: &[u32],
    ) -> Vec<ResolvedViewCall> {
        let file_ctx = self.file_context(uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        let function_loader_cl = |name: &str, offset: u32| function_loader(name, offset);

        with_parsed_program(content, "blade_call_site_inference", |program, content| {
            let default_class = ClassInfo::default();

            // Collect the matching call expressions first, then resolve
            // types — both inside the closure so AST references never
            // outlive the arena.
            let mut collected: Vec<SiteDraft<'_, '_>> = Vec::new();
            let walker = ViewCallWalker { offsets };
            let mut ctx = CollectCtx {
                sites: &mut collected,
            };
            for stmt in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&walker, stmt, &mut ctx);
            }

            let mut result = Vec::new();
            for site in collected {
                let enclosing =
                    crate::class_lookup::find_class_at_offset(&file_ctx.classes, site.offset);
                let current_class = enclosing.unwrap_or(&default_class);
                let loaders = Loaders::with_function(Some(&function_loader_cl));
                let var_ctx = VarResolutionCtx {
                    var_name: "",
                    top_level_scope: None,
                    current_class,
                    all_classes: &file_ctx.classes,
                    content,
                    cursor_offset: site.offset,
                    class_loader: &class_loader,
                    backend: Some(self),
                    loaders,
                    resolved_class_cache: Some(&self.resolved_class_cache),
                    enclosing_return_type: None,
                    branch_aware: false,
                    match_arm_narrowing: HashMap::new(),
                    scope_var_resolver: None,
                    scope_proofs: None,
                };

                let mut vars: Vec<PassedVar> = Vec::new();
                // Only what a shape argument turns out to hold can lower
                // the completeness the walker settled syntactically.
                let mut complete = site.complete;
                for entry in site.entries {
                    let framework_bound = entry.framework_bound();
                    let (name, key_range, value_range, ty) = match entry {
                        SiteEntry::Expr {
                            name,
                            key_range,
                            expr,
                        } => {
                            let span = expr.span();
                            // A bare variable needs the public variable
                            // resolver's `@var` precedence. The generic RHS
                            // resolver deliberately skips that outer layer
                            // because it is also used inside the forward walk.
                            let ty = match expr {
                                Expression::Variable(Variable::Direct(dv)) => crate::type_engine::variable::resolution::resolve_variable_php_type(
                                    bytes_to_str(dv.name),
                                    content,
                                    site.offset,
                                    Some(current_class),
                                    &file_ctx.classes,
                                    &class_loader,
                                    Some(self),
                                    Loaders::with_function(Some(&function_loader_cl)),
                                ),
                                _ => crate::type_engine::variable::foreach_resolution::resolve_expression_type(
                                    expr, &var_ctx,
                                ),
                            }
                            .unwrap_or_else(PhpType::mixed);
                            (name, key_range, (span.start.offset, span.end.offset), ty)
                        }
                        SiteEntry::Variable { name, key_range } => {
                            let loaders = Loaders::with_function(Some(&function_loader_cl));
                            let ty = crate::type_engine::variable::resolution::resolve_variable_php_type(
                                &name,
                                content,
                                site.offset,
                                Some(current_class),
                                &file_ctx.classes,
                                &class_loader,
                                Some(self),
                                loaders,
                            )
                            .unwrap_or_else(PhpType::mixed);
                            (name, key_range, key_range, ty)
                        }
                        SiteEntry::Iteration {
                            name,
                            key_range,
                            collection,
                            part,
                        } => {
                            let span = collection.span();
                            let ty = each_variable_type(collection, part, &var_ctx);
                            (name, key_range, (span.start.offset, span.end.offset), ty)
                        }
                        SiteEntry::Shape { expr } => {
                            // Nothing at the call site spells the names out,
                            // so the argument as a whole is what a diagnostic
                            // about any one of them points at.
                            let span = expr.span();
                            let range = (span.start.offset, span.end.offset);
                            match data_shape_entries(expr, &var_ctx) {
                                Some(entries) => {
                                    vars.extend(entries.into_iter().map(|(name, ty)| PassedVar {
                                        name,
                                        ty: qualify_class_names(ty, &class_loader),
                                        key_range: range,
                                        value_range: range,
                                        framework_bound: false,
                                    }))
                                }
                                None => complete = false,
                            }
                            continue;
                        }
                    };
                    vars.push(PassedVar {
                        name,
                        ty: qualify_class_names(ty, &class_loader),
                        key_range,
                        value_range,
                        framework_bound,
                    });
                }
                result.push(ResolvedViewCall {
                    name_range: site.name_range,
                    vars,
                    complete,
                    forwards_scope: site.forwards_scope,
                });
            }
            result
        })
    }

    /// The type the file at `uri` holds under `name` at `offset`.
    ///
    /// `blade_rendering_scope` answers which names a template forwards to
    /// the views it renders; this is the other half of that answer — what
    /// it holds under one of them *where the render is written*, so a
    /// `@foreach` binding reads as the loop binds it and a `@php`
    /// assignment as the last write before the include leaves it.
    ///
    /// `content` is the template's virtual PHP, which is where every source
    /// of a Blade scope lands: its own `@var` docblocks, the prologue the
    /// backing class and the providers are injected into, and the
    /// statements the body compiles to. So the ordinary variable
    /// resolution answers it, at the offset of the render itself.
    pub(crate) fn blade_scope_var_type(
        &self,
        file_ctx: &crate::types::FileContext,
        content: &str,
        offset: u32,
        name: &str,
    ) -> Option<PhpType> {
        let class_loader = self.class_loader(file_ctx);
        let function_loader = self.function_loader(file_ctx);
        let function_loader_cl = |name: &str, offset: u32| function_loader(name, offset);
        let ty = crate::type_engine::variable::resolution::resolve_variable_php_type(
            name,
            content,
            offset,
            crate::class_lookup::find_class_at_offset(&file_ctx.classes, offset),
            &file_ctx.classes,
            &class_loader,
            Some(self),
            Loaders::with_function(Some(&function_loader_cl)),
        )?;
        Some(qualify_class_names(ty, &class_loader))
    }

    /// Extract the variables one Blade caller passes to component tags,
    /// given the tag occurrences [`super::component_tags::scan_component_tag_calls`]
    /// already found in its raw source.
    ///
    /// `virtual_php` is the caller's own preprocessed content: a bound
    /// attribute on *any* HTML tag compiles down to a
    /// `blade_bound_attr_directive(EXPR)` call, in document order, so an
    /// occurrence's [`super::component_tags::ComponentTagCall::bound`]
    /// indices index directly into that call sequence — no Blade-to-PHP
    /// offset translation needed. That marker is exclusive to bound
    /// attributes, unlike the generic `blade_directive` shared by `@class`,
    /// `@json`, and other directives, so none of those can shift the
    /// sequence out of sync with `scan_component_tag_calls`'s count.
    fn extract_component_call_site_vars(
        &self,
        uri: &str,
        virtual_php: &str,
        occurrences: Vec<crate::blade::component_tags::ComponentTagCall>,
    ) -> Vec<InferredVars> {
        let file_ctx = self.file_context(uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        let function_loader_cl = |name: &str, offset: u32| function_loader(name, offset);

        with_parsed_program(
            virtual_php,
            "blade_component_call_site",
            |program, content| {
                let default_class = ClassInfo::default();

                let mut ctx = BladeDirectiveCollectCtx { calls: Vec::new() };
                let walker = BladeDirectiveWalker;
                for stmt in program.statements.iter() {
                    mago_syntax::walker::Walker::walk_statement(&walker, stmt, &mut ctx);
                }
                let calls = ctx.calls;

                let mut result = Vec::new();
                for occurrence in occurrences {
                    let mut vars: InferredVars = occurrence.literal;
                    for (name, index) in occurrence.bound {
                        let Some(expr) = calls.get(index).copied() else {
                            continue;
                        };
                        let offset = expr.span().start.offset;
                        let enclosing =
                            crate::class_lookup::find_class_at_offset(&file_ctx.classes, offset);
                        let current_class = enclosing.unwrap_or(&default_class);
                        let loaders = Loaders::with_function(Some(&function_loader_cl));
                        let var_ctx = VarResolutionCtx {
                            var_name: "",
                            top_level_scope: None,
                            current_class,
                            all_classes: &file_ctx.classes,
                            content,
                            cursor_offset: offset,
                            class_loader: &class_loader,
                            backend: Some(self),
                            loaders,
                            resolved_class_cache: Some(&self.resolved_class_cache),
                            enclosing_return_type: None,
                            branch_aware: false,
                            match_arm_narrowing: HashMap::new(),
                            scope_var_resolver: None,
                            scope_proofs: None,
                        };
                        let ty = crate::type_engine::variable::foreach_resolution::resolve_expression_type(
                        expr, &var_ctx,
                    )
                    .unwrap_or_else(PhpType::mixed);
                        vars.push((name, qualify_class_names(ty, &class_loader)));
                    }
                    if !vars.is_empty() {
                        result.push(vars);
                    }
                }
                result
            },
        )
    }
}

#[cfg(test)]
#[path = "call_site_inference_tests.rs"]
mod tests;
