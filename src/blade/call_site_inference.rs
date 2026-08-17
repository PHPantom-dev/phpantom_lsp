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
use mago_syntax::cst::literal::{Literal, LiteralString};
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{bytes_to_str, literal_bytes_to_str};
use crate::parser::with_parsed_program;
use crate::php_type::{PhpType, TypeKind};
use crate::symbol_map::{LaravelStringKind, SymbolKind, SymbolMap};
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::types::ClassInfo;
use crate::virtual_members::laravel::canonical_view_name;

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
                // therefore not `blade_directive` calls to count.
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
        let Ok(mut path) = url.to_file_path() else {
            return Vec::new();
        };

        // Roots are canonicalized below, so normalize the file side once as
        // well. macOS exposes the same temporary directory through `/var`
        // and `/private/var`; comparing only one canonical side makes every
        // template under that alias appear to sit outside its view root.
        path = path.canonicalize().unwrap_or_else(|_| {
            path.parent()
                .and_then(|parent| parent.canonicalize().ok())
                .and_then(|parent| path.file_name().map(|name| parent.join(name)))
                .unwrap_or(path)
        });

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

        // `path` came from a file URI and is absolute; a view root can
        // be relative when the workspace root was given relative (the
        // analyse CLI passes `--project-root` through as-is), so
        // canonicalize each root before comparing.
        for root in self.laravel_view_roots() {
            let root = root.canonicalize().unwrap_or(root);
            if let Ok(rel) = path.strip_prefix(&root) {
                push_name(rel, "");
            }
        }
        for res in &self.laravel_provider_resources.read().view_dirs {
            let res_path = res.path.canonicalize().unwrap_or_else(|_| res.path.clone());
            if let Ok(rel) = path.strip_prefix(&res_path) {
                push_name(rel, &res.namespace);
            }
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
    /// attribute on *any* HTML tag compiles down to a `blade_directive(EXPR)`
    /// call, in document order, so an occurrence's
    /// [`super::component_tags::ComponentTagCall::bound`] indices index
    /// directly into that call sequence — no Blade-to-PHP offset
    /// translation needed.
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

// ─── AST walking ────────────────────────────────────────────────────────────

/// Collects every `blade_directive(EXPR)` call in a Blade file's virtual
/// PHP, in document order. The preprocessor emits exactly one such call
/// per bound HTML attribute (see `super::preprocessor`), so this order
/// matches the order `super::component_tags::scan_component_tag_calls`
/// counts bound attributes in.
struct BladeDirectiveCollectCtx<'ast, 'arena> {
    calls: Vec<&'ast Expression<'arena>>,
}

struct BladeDirectiveWalker;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, BladeDirectiveCollectCtx<'ast, 'arena>>
    for BladeDirectiveWalker
{
    fn walk_in_function_call(
        &self,
        node: &'ast FunctionCall<'arena>,
        ctx: &mut BladeDirectiveCollectCtx<'ast, 'arena>,
    ) {
        let Expression::Identifier(ident) = node.function else {
            return;
        };
        if bytes_to_str(ident.value()) != "blade_directive" {
            return;
        }
        if let Some(arg) = node.argument_list.arguments.iter().next() {
            ctx.calls.push(arg.value());
        }
    }
}

/// Which of the two variables an `@each` binds an entry describes.
#[derive(Clone, Copy, PartialEq)]
enum IterationPart {
    Item,
    Key,
}

/// One variable passed at a call site: the value expression (array entry
/// / `->with()` value), the same-named variable to resolve at the
/// call-site offset (for `compact('name')`), one of the two variables
/// `@each` derives from the collection it iterates, or a whole data
/// argument whose entries only its type names.
#[derive(Clone)]
enum SiteEntry<'ast, 'arena> {
    Expr {
        name: String,
        key_range: ByteRange,
        expr: &'ast Expression<'arena>,
    },
    Variable {
        name: String,
        key_range: ByteRange,
    },
    Iteration {
        name: String,
        key_range: ByteRange,
        collection: &'ast Expression<'arena>,
        part: IterationPart,
    },
    /// A data argument written as anything but an array literal or a
    /// `compact()` call — `view('page', $data)`, `->with($extra)`,
    /// `array_merge($a, $b)`, a method call returning an array. It names
    /// its variables only in its type, so it stands for however many
    /// entries [`data_shape_entries`] reads off it.
    Shape {
        expr: &'ast Expression<'arena>,
    },
}

impl SiteEntry<'_, '_> {
    /// Whether Blade names the variable itself.  Only `@each`'s `$key`
    /// does: its name comes from the directive rather than from anything
    /// the call site writes.
    fn framework_bound(&self) -> bool {
        matches!(
            self,
            SiteEntry::Iteration {
                part: IterationPart::Key,
                ..
            }
        )
    }
}

/// One call site as the walker finds it, before its entries are resolved.
struct SiteDraft<'ast, 'arena> {
    /// The offset of the `view()` call itself, which every chained
    /// `->with(…)` is folded into and which types resolve at.
    offset: u32,
    name_range: ByteRange,
    entries: Vec<SiteEntry<'ast, 'arena>>,
    complete: bool,
    forwards_scope: bool,
}

struct CollectCtx<'w, 'ast, 'arena> {
    sites: &'w mut Vec<SiteDraft<'ast, 'arena>>,
}

impl<'ast, 'arena> CollectCtx<'_, 'ast, 'arena> {
    /// The draft for the view call at `offset` that names the template at
    /// `name_range`, created if the walker has not reached it yet.
    ///
    /// A chained `->with(…)` is walked *before* the `view()` call it hangs
    /// off (the method call is the outer node), so either end of the chain
    /// may be the first to open the site.
    ///
    /// The name is part of the key, not just the offset: an
    /// `@includeFirst(['custom.header', 'partials.header'])` renders whichever
    /// of its candidates exists, so one call is a site of each template it
    /// names, and each is judged against its own contract.
    fn site(&mut self, offset: u32, name_range: ByteRange) -> &mut SiteDraft<'ast, 'arena> {
        if let Some(index) = self
            .sites
            .iter()
            .position(|site| site.offset == offset && site.name_range == name_range)
        {
            return &mut self.sites[index];
        }
        self.sites.push(SiteDraft {
            offset,
            name_range,
            entries: Vec::new(),
            complete: true,
            forwards_scope: true,
        });
        self.sites.last_mut().expect("just pushed")
    }

    /// Record the same data against every template one call names.
    fn record(
        &mut self,
        offset: u32,
        name_ranges: &[ByteRange],
        entries: Vec<SiteEntry<'ast, 'arena>>,
        complete: bool,
        forwards_scope: bool,
    ) {
        for name_range in name_ranges {
            let site = self.site(offset, *name_range);
            site.entries.extend(entries.iter().cloned());
            site.complete &= complete;
            site.forwards_scope &= forwards_scope;
        }
    }
}

/// Walker that finds `view('name', …)` / `View::make('name', …)` calls
/// whose view-name string contents sit at one of the requested offsets,
/// and collects the data entries they pass: the array literal /
/// `compact()` call that follows the name, plus any `->with(…)` chained
/// onto the call.
struct ViewCallWalker<'a> {
    offsets: &'a [u32],
}

impl ViewCallWalker<'_> {
    /// The ranges of the view-name strings in an argument list, along with
    /// the index of the argument holding them, when the list names one of
    /// the views asked about.
    ///
    /// The name is looked for at any position rather than only the first:
    /// `Route::view('/about', 'pages.about', …)` puts the URI first, and
    /// the data argument is always the one after the name whatever the
    /// helper's shape.
    ///
    /// One argument can name several templates: a `…First` directive and the
    /// factory's `first()` take a list of candidates and render whichever
    /// exists, so every entry of the array is one the call reaches.
    fn matches(&self, argument_list: &ArgumentList<'_>) -> Option<(usize, Vec<ByteRange>)> {
        argument_list
            .arguments
            .iter()
            .enumerate()
            .find_map(|(index, argument)| {
                let ranges = self.matching_ranges(argument.value());
                (!ranges.is_empty()).then_some((index, ranges))
            })
    }

    /// The view-name ranges one argument holds: the string itself, or the
    /// entries of the candidate array a `…First` directive names.
    fn matching_ranges(&self, expr: &Expression<'_>) -> Vec<ByteRange> {
        let mut ranges = Vec::new();
        match expr {
            Expression::Literal(Literal::String(s)) => {
                let inner = (s.span.start.offset + 1, s.span.end.offset - 1);
                if self.offsets.contains(&inner.0) {
                    ranges.push(inner);
                }
            }
            Expression::Array(array) => {
                for element in array.elements.iter() {
                    if let ArrayElement::Value(value) = element {
                        ranges.extend(self.matching_ranges(value.value));
                    }
                }
            }
            Expression::LegacyArray(array) => {
                for element in array.elements.iter() {
                    if let ArrayElement::Value(value) = element {
                        ranges.extend(self.matching_ranges(value.value));
                    }
                }
            }
            _ => {}
        }
        ranges
    }
}

impl<'ast, 'arena, 'w> mago_syntax::walker::Walker<'ast, 'arena, CollectCtx<'w, 'ast, 'arena>>
    for ViewCallWalker<'_>
{
    fn walk_in_function_call(
        &self,
        node: &'ast FunctionCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let Expression::Identifier(ident) = node.function else {
            return;
        };
        let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
        if !is_view_render_function(name) {
            return;
        }
        let Some((index, name_ranges)) = self.matches(&node.argument_list) else {
            return;
        };
        let mut entries = Vec::new();
        // `@each` names its partial first and then a collection and an item
        // name, so the argument after the view name is not a data array and
        // the two variables the partial receives are derived rather than
        // listed.  A match on any later argument (the optional empty-view
        // name) says nothing about what the partial is handed.
        let complete = if is_each_render_function(name) {
            index == 0 && collect_each_arguments(&node.argument_list, &mut entries)
        } else {
            collect_data_argument(&node.argument_list, index + 1, &mut entries)
        };
        ctx.record(
            node.span().start.offset,
            &name_ranges,
            entries,
            complete,
            !is_each_render_function(name),
        );
    }

    fn walk_in_static_method_call(
        &self,
        node: &'ast StaticMethodCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let ClassLikeMemberSelector::Identifier(method) = &node.method else {
            return;
        };
        let method_name = bytes_to_str(method.value);
        if is_view_facade(node.class) && is_render_each_method(method_name) {
            collect_render_each_sites(node.span().start.offset, &node.argument_list, self, ctx);
            return;
        }
        if !is_view_render_static_call(node.class, method_name) {
            return;
        }
        let Some((index, name_ranges)) = self.matches(&node.argument_list) else {
            return;
        };
        let mut entries = Vec::new();
        let complete = collect_data_argument(&node.argument_list, index + 1, &mut entries);
        ctx.record(
            node.span().start.offset,
            &name_ranges,
            entries,
            complete,
            true,
        );
    }

    /// `new Content(view: 'emails.orders.shipped', with: […])`, the value a
    /// mailable's `content()` returns.
    fn walk_in_instantiation(
        &self,
        node: &'ast Instantiation<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let Some(argument_list) = &node.argument_list else {
            return;
        };
        let Some((_, name_ranges)) = self.matches(argument_list) else {
            return;
        };
        let mut entries = Vec::new();
        let complete = collect_content_data_argument(argument_list, &mut entries);
        ctx.record(
            node.span().start.offset,
            &name_ranges,
            entries,
            complete,
            true,
        );
    }

    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let ClassLikeMemberSelector::Identifier(method) = &node.method else {
            return;
        };
        let method_name = bytes_to_str(method.value);

        // `renderEach()` is the PHP spelling of `@each`, down to the
        // argument order, so both templates it names are read the way the
        // directive's are.
        if is_render_each_method(method_name) {
            collect_render_each_sites(node.span().start.offset, &node.argument_list, self, ctx);
            return;
        }

        // A render the receiver's type decides: a mailable's
        // `$this->view('name', […])` and the view factory's own `make()` /
        // `first()` / `renderWhen()`.  Which of those the receiver actually
        // is was settled when the name was indexed, so a matching name at
        // the position the method takes one is the whole test here.
        if let Some(name_index) = view_render_method_name_index(method_name)
            && let Some((index, name_ranges)) = self.matches(&node.argument_list)
            && index == name_index
        {
            let mut entries = Vec::new();
            let complete = collect_data_argument(&node.argument_list, index + 1, &mut entries);
            ctx.record(
                node.span().start.offset,
                &name_ranges,
                entries,
                complete,
                true,
            );
            return;
        }

        // `->with('key', $value)` / `->with(['key' => $value])` /
        // `->withKey($value)` chained onto a matching `view()` call.  The
        // receiver chain may pass through other builder methods
        // (`->layout(…)`), so scan the whole spine for the matching view
        // call.
        if !method_name.starts_with("with") && !method_name.starts_with("With") {
            return;
        }
        let Some((offset, name_ranges)) = matching_view_call_in_chain(node.object, self) else {
            return;
        };

        let mut entries = Vec::new();
        let mut complete = true;
        // `->withUser($user)` is Laravel's magic setter for `$user`; the
        // name is the method's own tail, so the method identifier is what
        // a diagnostic points at.
        if let Some(magic) = magic_with_name(method_name) {
            match node.argument_list.arguments.iter().next() {
                Some(value) => entries.push(SiteEntry::Expr {
                    name: magic,
                    key_range: (method.span.start.offset, method.span.end.offset),
                    expr: value.value(),
                }),
                None => complete = false,
            }
        } else {
            let mut args = node.argument_list.arguments.iter();
            match (args.next(), args.next()) {
                (Some(key_arg), Some(value_arg)) => {
                    // ->with('key', $value)
                    match key_arg.value() {
                        Expression::Literal(Literal::String(s)) => {
                            match string_literal_contents(s) {
                                Some(name) => entries.push(SiteEntry::Expr {
                                    name,
                                    key_range: (s.span.start.offset + 1, s.span.end.offset - 1),
                                    expr: value_arg.value(),
                                }),
                                None => complete = false,
                            }
                        }
                        _ => complete = false,
                    }
                }
                (Some(single), None) => {
                    // ->with(['key' => $value, …]) or ->with(compact('key'))
                    complete = collect_from_data_expr(single.value(), &mut entries);
                }
                _ => complete = false,
            }
        }

        ctx.record(offset, &name_ranges, entries, complete, true);
    }
}

/// The type one of `@each`'s two variables has, derived from the
/// collection the directive iterates.
///
/// The derivation is the same one `foreach ($collection as $key => $item)`
/// uses, so a `Collection<int, User>`, an `array<string, Row>`, and a
/// class whose `@implements IteratorAggregate` says what it holds all
/// answer the way they do in a loop.  A collection that says nothing about
/// its entries leaves the item `mixed` and the key `int|string`, matching
/// what PHP guarantees about iterating anything at all.
fn each_variable_type(
    collection: &Expression<'_>,
    part: IterationPart,
    var_ctx: &VarResolutionCtx<'_>,
) -> PhpType {
    use crate::type_engine::variable::foreach_resolution as iter;

    let derived = iter::resolve_expression_type(collection, var_ctx).and_then(|collection_ty| {
        let iter_ctx = iter::IterableCtx::from_var_ctx(var_ctx);
        match part {
            IterationPart::Item => iter::iteration_value_type(&collection_ty, &iter_ctx),
            IterationPart::Key => iter::iteration_key_type(&collection_ty, &iter_ctx),
        }
    });
    derived.unwrap_or_else(|| match part {
        IterationPart::Item => PhpType::mixed(),
        IterationPart::Key => PhpType::union(vec![PhpType::int(), PhpType::string()]),
    })
}

/// Render the class names a resolved type mentions as FQNs.
///
/// A call site's types are resolved in the caller's import context, while
/// the template that receives them has neither those imports nor a
/// namespace: an injected `@var User $user` in a template's prologue means
/// the global `\User`, and a template's contract is qualified the same way
/// before a call site is judged against it.
fn qualify_class_names(
    ty: PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    ty.resolve_names(&|name: &str| match class_loader(name) {
        Some(cls) => format!("\\{}", cls.fqn()),
        None => name.to_string(),
    })
}

/// The interface Laravel's view factory accepts in place of a data array,
/// converting it with `toArray()` before rendering.
const ARRAYABLE: &str = "Illuminate\\Contracts\\Support\\Arrayable";

/// The variables a data argument hands the template when only its type
/// names them.
///
/// The argument is resolved through the shared pipeline and read as a
/// single constant array shape, which is what Bladestan reads off
/// `$scope->getType()`. Optional keys (`array{user?: User}`) are dropped:
/// the caller may or may not pass them, so they stay reportable as missing
/// while the guaranteed keys are still type-checked.
///
/// `None` when the type is not one shape, which leaves the call site
/// incomplete and stands the missing and unknown checks down as before.
fn data_shape_entries(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<(String, PhpType)>> {
    let ty = crate::type_engine::variable::foreach_resolution::resolve_expression_type(expr, ctx)?;
    array_shape_entries(&ty).or_else(|| arrayable_shape_entries(&ty, ctx))
}

/// The guaranteed entries of an array shape, under the names Blade's
/// `extract()` would bind them to.
///
/// A positional entry, or one keyed by anything that is not a variable
/// name, is dropped rather than counted against the shape: `extract()`
/// skips it too, so it hides nothing the caller passes.
fn array_shape_entries(ty: &PhpType) -> Option<Vec<(String, PhpType)>> {
    let TypeKind::ArrayShape(entries) = ty.kind() else {
        return None;
    };
    Some(
        entries
            .iter()
            .filter(|entry| !entry.optional)
            .filter_map(|entry| {
                let key = entry.key.as_deref().filter(|key| is_variable_name(key))?;
                Some((key.to_string(), entry.value_type.clone()))
            })
            .collect(),
    )
}

/// The entries an `Arrayable` hands over: the factory calls `toArray()` on
/// one before rendering, so its return type describes the data exactly as
/// an array argument's own type does.
fn arrayable_shape_entries(
    ty: &PhpType,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<(String, PhpType)>> {
    let class = (ctx.class_loader)(ty.base_name()?)?;
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        &class,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    if !crate::class_lookup::is_subtype_of(&merged, ARRAYABLE, ctx.class_loader) {
        return None;
    }
    array_shape_entries(merged.get_method_ci("toArray")?.return_type.as_ref()?)
}

/// Whether a helper function renders a view named by one of its string
/// arguments.
///
/// `blade_view_directive` is what the preprocessor compiles Blade's own
/// `@include` family, `@extends`, and `@component` into, so a template
/// rendering another template is judged by the same rules a controller
/// is.  `@each` compiles to [`is_each_render_function`]'s marker instead,
/// because its arguments do not describe a data array.
fn is_view_render_function(name: &str) -> bool {
    name.eq_ignore_ascii_case("view")
        || name.eq_ignore_ascii_case("blade_view_directive")
        || is_each_render_function(name)
}

/// Whether the call is the preprocessor's compilation of `@each`.
fn is_each_render_function(name: &str) -> bool {
    name.eq_ignore_ascii_case("blade_each_directive")
}

/// Whether a static call renders a view: the `View` facade's factory
/// methods, the `Route::view()` shorthand that binds a URI straight to a
/// template, or `Response::view()`.
///
/// `View::exists()` is left out on purpose. It names a template without
/// rendering it, so it hands it nothing, and reading it as a render would
/// report every variable the template declares as missing.
fn is_view_render_static_call(class: &Expression<'_>, method: &str) -> bool {
    let Expression::Identifier(ident) = class else {
        return false;
    };
    let subject = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
    let is_facade = |short: &str, fqn: &str| {
        subject.eq_ignore_ascii_case(short) || subject.eq_ignore_ascii_case(fqn)
    };
    (is_view_facade(class) && view_render_method_name_index(method).is_some())
        || (is_facade("Route", "Illuminate\\Support\\Facades\\Route")
            && method.eq_ignore_ascii_case("view"))
        || (is_facade("Response", "Illuminate\\Support\\Facades\\Response")
            && method.eq_ignore_ascii_case("view"))
}

/// Whether a static call's subject is the `View` facade, which proxies the
/// view factory.
fn is_view_facade(class: &Expression<'_>) -> bool {
    let Expression::Identifier(ident) = class else {
        return false;
    };
    let subject = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
    subject.eq_ignore_ascii_case("View")
        || subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\View")
}

/// Whether a method is the view factory's `renderEach()`.
fn is_render_each_method(method: &str) -> bool {
    method.eq_ignore_ascii_case("renderEach")
}

/// Record the sites a `renderEach($view, $data, $iterator, $empty)` call is.
///
/// It renders `$view` once per entry of `$data`, with the entry under the
/// name `$iterator` spells and the key beside it, and `$empty` once with
/// nothing at all when the collection is empty. Neither partial sees the
/// scope the call is written in, so neither forwards it.
fn collect_render_each_sites<'ast, 'arena>(
    offset: u32,
    argument_list: &'ast ArgumentList<'arena>,
    walker: &ViewCallWalker<'_>,
    ctx: &mut CollectCtx<'_, 'ast, 'arena>,
) {
    let mut arguments = argument_list.arguments.iter();
    if let Some(view) = arguments.next() {
        let ranges = walker.matching_ranges(view.value());
        if !ranges.is_empty() {
            let mut entries = Vec::new();
            let complete = collect_each_arguments(argument_list, &mut entries);
            ctx.record(offset, &ranges, entries, complete, false);
        }
    }
    if let Some(empty) = arguments.nth(2) {
        let ranges = walker.matching_ranges(empty.value());
        if !ranges.is_empty() {
            ctx.record(offset, &ranges, Vec::new(), true, false);
        }
    }
}

/// The argument index at which a view-rendering *method* names its
/// template, or `None` when the method names none.
///
/// Covers both families the symbol map indexes by receiver type: a
/// mailable's `view()` / `text()` / `markdown()`, and the view factory's
/// `make()` / `first()` / `renderWhen()` / `renderUnless()`. Which of the
/// two a receiver is was decided when the name was indexed, so the two sets
/// need not be told apart again here.
fn view_render_method_name_index(method: &str) -> Option<usize> {
    match method.to_ascii_lowercase().as_str() {
        "view" | "text" | "markdown" | "make" | "first" => Some(0),
        "renderwhen" | "renderunless" => Some(1),
        _ => None,
    }
}

/// The variable a `->withSomething()` magic setter names, following
/// Laravel's `View::__call()` (`with` plus the camel-cased tail).
///
/// Plain `->with(…)` is not a magic setter, and neither is a method whose
/// tail does not start a new word (`->within(…)`).
fn magic_with_name(method: &str) -> Option<String> {
    let rest = method
        .strip_prefix("with")
        .or_else(|| method.strip_prefix("With"))?;
    let mut chars = rest.chars();
    let first = chars.next().filter(|ch| ch.is_uppercase())?;
    Some(first.to_lowercase().chain(chars).collect())
}

/// The offset and view-name ranges of the `view()` / `View::make()` /
/// `$this->view()` call a method call's receiver spine ends in, when it
/// names one of the views asked about.
///
/// Walks through chained method calls (`view(…)->with(…)->with(…)`) but
/// not through variables — a `$view = view(…); $view->with(…)` split is
/// out of scope.
fn matching_view_call_in_chain(
    mut expr: &Expression<'_>,
    walker: &ViewCallWalker<'_>,
) -> Option<(u32, Vec<ByteRange>)> {
    loop {
        match expr {
            Expression::Call(Call::Function(fc)) => {
                let Expression::Identifier(ident) = fc.function else {
                    return None;
                };
                let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
                if !is_view_render_function(name) {
                    return None;
                }
                let (_, ranges) = walker.matches(&fc.argument_list)?;
                return Some((fc.span().start.offset, ranges));
            }
            Expression::Call(Call::StaticMethod(sc)) => {
                let ClassLikeMemberSelector::Identifier(method) = &sc.method else {
                    return None;
                };
                if !is_view_render_static_call(sc.class, bytes_to_str(method.value)) {
                    return None;
                }
                let (_, ranges) = walker.matches(&sc.argument_list)?;
                return Some((sc.span().start.offset, ranges));
            }
            Expression::Call(Call::Method(mc)) => {
                // `$this->view('emails.shipped')->with([…])` in a mailable
                // renders from the link the spine passes through, not from
                // the one it ends at.
                if let ClassLikeMemberSelector::Identifier(method) = &mc.method
                    && let Some(name_index) =
                        view_render_method_name_index(bytes_to_str(method.value))
                    && let Some((index, ranges)) = walker.matches(&mc.argument_list)
                    && index == name_index
                {
                    return Some((mc.span().start.offset, ranges));
                }
                expr = mc.object;
            }
            Expression::Parenthesized(p) => {
                expr = p.expression;
            }
            _ => return None,
        }
    }
}

/// Collect variable entries from the data argument at `index` of a
/// `view()` / `View::make()` argument list.
///
/// Returns whether the argument was readable in full: an absent one
/// passes nothing (readable), while one built from a variable or a
/// non-literal key hides names the caller does pass.
fn collect_data_argument<'ast, 'arena>(
    argument_list: &'ast ArgumentList<'arena>,
    index: usize,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) -> bool {
    match argument_list.arguments.iter().nth(index) {
        Some(arg) => collect_from_data_expr(arg.value(), entries),
        None => true,
    }
}

/// The position `Illuminate\Mail\Mailables\Content` takes its data at when
/// its constructor is called positionally, after `view`, `html`, `text`,
/// and `markdown`.
const CONTENT_WITH_INDEX: usize = 4;

/// Collect variable entries from the data a `new Content(…)` passes.
///
/// A `Content` names its data `with:` rather than putting it after the view
/// name, so the pairing is by argument name; the constructor still accepts
/// it positionally, at [`CONTENT_WITH_INDEX`].
///
/// A mailable also hands its view every public property it declares, which
/// no argument here names — see `Backend::component_render_scope_names`.
fn collect_content_data_argument<'ast, 'arena>(
    argument_list: &'ast ArgumentList<'arena>,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) -> bool {
    for argument in argument_list.arguments.iter() {
        if let Argument::Named(named) = argument
            && bytes_to_str(named.name.value).eq_ignore_ascii_case("with")
        {
            return collect_from_data_expr(named.value, entries);
        }
    }
    match argument_list.arguments.iter().nth(CONTENT_WITH_INDEX) {
        Some(Argument::Positional(positional)) => collect_from_data_expr(positional.value, entries),
        _ => true,
    }
}

/// Collect the two variables an `@each` binds: the entry, under the name
/// the third argument spells, and `$key`.
///
/// `@each('partials.row', $rows, 'row')` renders `partials.row` once per
/// entry of `$rows` with `$row` and `$key` in scope and nothing else, so
/// both are derived from the collection rather than read off a data array.
///
/// Returns whether the pair was readable: an `@each` short of arguments,
/// or one whose item name is not a plain string literal, binds a name that
/// cannot be known.
fn collect_each_arguments<'ast, 'arena>(
    argument_list: &'ast ArgumentList<'arena>,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) -> bool {
    let mut args = argument_list.arguments.iter().skip(1);
    let (Some(collection), Some(item)) = (args.next(), args.next()) else {
        return false;
    };
    let Expression::Literal(Literal::String(s)) = item.value() else {
        return false;
    };
    let Some(name) = string_literal_contents(s) else {
        return false;
    };
    // The item name is the only text at the call site that names either
    // variable, so it is where a diagnostic about the pair points.
    let key_range = (s.span.start.offset + 1, s.span.end.offset - 1);
    let collection = collection.value();
    entries.push(SiteEntry::Iteration {
        name,
        key_range,
        collection,
        part: IterationPart::Item,
    });
    entries.push(SiteEntry::Iteration {
        name: "key".to_string(),
        key_range,
        collection,
        part: IterationPart::Key,
    });
    true
}

/// Collect entries from a data expression: an array literal with
/// string keys, a `compact('a', 'b')` call (whose values are the
/// same-named variables at the call site), or anything else, whose
/// entries are read off its resolved type instead (see
/// [`data_shape_entries`]).
///
/// Returns whether every entry the expression writes down was readable.
/// An expression that writes none is readable here and answered for when
/// its type is resolved.
fn collect_from_data_expr<'ast, 'arena>(
    expr: &'ast Expression<'arena>,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) -> bool {
    let mut collect_array_elements =
        |elements: &'ast TokenSeparatedSequence<'arena, ArrayElement<'arena>>| {
            let mut complete = true;
            for element in elements.iter() {
                let ArrayElement::KeyValue(kv) = element else {
                    // A spread, or a positional entry Blade's `extract()`
                    // would drop: either way the key set is not the one
                    // written here.
                    complete = false;
                    continue;
                };
                let Expression::Literal(Literal::String(s)) = kv.key else {
                    complete = false;
                    continue;
                };
                match string_literal_contents(s) {
                    Some(name) => entries.push(SiteEntry::Expr {
                        name,
                        key_range: (s.span.start.offset + 1, s.span.end.offset - 1),
                        expr: kv.value,
                    }),
                    None => complete = false,
                }
            }
            complete
        };
    match expr {
        Expression::Array(array) => collect_array_elements(&array.elements),
        Expression::LegacyArray(array) => collect_array_elements(&array.elements),
        Expression::Call(Call::Function(fc)) if is_compact_call(fc) => {
            let mut complete = true;
            for arg in fc.argument_list.arguments.iter() {
                match arg.value() {
                    Expression::Literal(Literal::String(s)) => match string_literal_contents(s) {
                        Some(name) => entries.push(SiteEntry::Variable {
                            name,
                            key_range: (s.span.start.offset + 1, s.span.end.offset - 1),
                        }),
                        None => complete = false,
                    },
                    _ => complete = false,
                }
            }
            complete
        }
        _ => {
            entries.push(SiteEntry::Shape { expr });
            true
        }
    }
}

/// Whether a function call is `compact(…)`, whose arguments name the
/// variables it copies out of the calling scope.
fn is_compact_call(call: &FunctionCall<'_>) -> bool {
    let Expression::Identifier(ident) = call.function else {
        return false;
    };
    crate::util::strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case("compact")
}

/// The contents of a single- or double-quoted string literal, when it
/// is a plain identifier-safe name.
pub(crate) fn string_literal_contents(s: &LiteralString<'_>) -> Option<String> {
    let value = s.value.and_then(literal_bytes_to_str)?;
    is_variable_name(value).then(|| value.to_string())
}

/// Whether a key can name a PHP variable, and so survive the `extract()`
/// Blade hands a template's data through.
fn is_variable_name(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !value.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::join_call_site_types;
    use crate::Backend;
    use crate::atom::atom;
    use crate::php_type::PhpType;
    use tower_lsp::lsp_types::Url;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    /// The joined union's member order must not depend on the order the
    /// call sites were visited in, since that order comes from a
    /// `HashMap` snapshot and varies across runs.
    #[test]
    fn union_order_is_independent_of_visit_order() {
        let a = PhpType::named(atom("App\\Item"));
        let b = PhpType::literal_string_raw("fallback");

        let forward = join_call_site_types(vec![a.clone(), b.clone()]).to_string();
        let backward = join_call_site_types(vec![b, a]).to_string();

        assert_eq!(
            forward, backward,
            "joining the same types in reverse order must produce the same union string"
        );
    }

    #[test]
    fn headless_view_snapshot_builds_local_candidates() {
        let mut backend = Backend::new_test();
        backend.skip_reference_index = true;
        let uri = "file:///project/app/Controller.php";
        backend.update_ast(uri, "<?php\nview('shop', ['item' => new Item()]);\n");

        let snapshot = backend.view_caller_snapshot();
        let candidates = snapshot
            .local_candidates
            .as_ref()
            .and_then(|by_view| by_view.get("shop"))
            .expect("headless refresh should index the view caller locally");

        assert_eq!(candidates.len(), 1);
        assert_eq!(snapshot.files[candidates[0]].0, uri);
    }

    #[test]
    fn headless_inference_skips_non_candidate_callers() {
        let dir = tempfile::tempdir().expect("failed to create test workspace");
        let root = dir
            .path()
            .canonicalize()
            .expect("test workspace should canonicalize");
        let views = root.join("resources/views");
        let app = root.join("app");
        std::fs::create_dir_all(&views).expect("failed to create view directory");
        std::fs::create_dir_all(&app).expect("failed to create app directory");

        let target = views.join("shop.blade.php");
        let matching = app.join("MatchingController.php");
        let unrelated = app.join("UnrelatedController.php");
        let matching_source = "<?php\nview('shop', ['kept' => 1]);\n";
        let unrelated_source = "<?php\nview('other', ['excluded' => 2]);\n";
        std::fs::write(&target, "").expect("failed to write target view");
        std::fs::write(&matching, matching_source).expect("failed to write matching caller");
        std::fs::write(&unrelated, unrelated_source).expect("failed to write unrelated caller");

        let mut backend = Backend::new_test_with_workspace(root, Vec::new());
        backend.skip_reference_index = true;
        for (path, source) in [(&matching, matching_source), (&unrelated, unrelated_source)] {
            let uri = Url::from_file_path(path).expect("caller path should become a file URI");
            backend.update_ast(uri.as_str(), source);
        }

        let snapshot = backend.view_caller_snapshot();
        let by_view = snapshot
            .local_candidates
            .as_ref()
            .expect("headless snapshot should carry local candidates");
        assert_eq!(snapshot.files.len(), 2);
        assert_eq!(by_view.get("shop").map(Vec::len), Some(1));
        assert_eq!(by_view.get("other").map(Vec::len), Some(1));

        let target_uri = Url::from_file_path(target).expect("target path should become a file URI");
        assert_eq!(
            backend.view_names_for_blade_uri(target_uri.as_str()),
            vec!["shop"]
        );
        let scope =
            backend.compute_blade_injected_vars(target_uri.as_str(), "", Some(&snapshot), None);

        assert!(
            scope.vars.iter().any(|(name, _)| name == "kept"),
            "matching caller should contribute its data: {scope:?}"
        );
        assert!(
            scope.vars.iter().all(|(name, _)| name != "excluded"),
            "non-candidate caller must be skipped: {scope:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn view_name_resolution_normalizes_aliased_file_paths() {
        let dir = tempfile::tempdir().expect("failed to create test workspace");
        let real_root = dir.path().join("real-project");
        let linked_root = dir.path().join("linked-project");
        let views = real_root.join("resources/views");
        std::fs::create_dir_all(&views).expect("failed to create view directory");
        symlink(&real_root, &linked_root).expect("failed to create workspace alias");

        let real_template = views.join("shop.blade.php");
        std::fs::write(&real_template, "").expect("failed to write view");
        let linked_template = linked_root.join("resources/views/shop.blade.php");
        let linked_uri =
            Url::from_file_path(&linked_template).expect("view path should become a file URI");
        let backend = Backend::new_test_with_workspace(linked_root, Vec::new());

        assert_eq!(
            backend.view_names_for_blade_uri(linked_uri.as_str()),
            vec!["shop"]
        );

        std::fs::remove_file(real_template).expect("failed to remove view");
        assert_eq!(
            backend.view_names_for_blade_uri(linked_uri.as_str()),
            vec!["shop"]
        );
    }
}
