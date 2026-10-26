//! Keeping call-site inference current: which templates to re-infer after
//! an edit or an indexing pass, in what order, and the re-parse of each
//! template whose inferred scope changed. The inference itself lives in
//! [`super::call_site_inference`].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::Backend;
use crate::symbol_map::{StringKeyKind, SymbolKind};
use crate::virtual_members::laravel::canonical_view_name;

use super::call_site_inference::{BladeCallerSnapshot, BladeScope, ViewCallerSnapshot};

impl Backend {
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
        let blade_uris = self.lowered_template_uris();
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
        // A controller that renders many templates is walked once per
        // template, and every method body it infers a return type from
        // would be walked again each time.  The other type-engine memos
        // stay scoped to one caller file: their keys include the address
        // of the content they walked, and this pass replaces templates'
        // virtual PHP as it goes, so a freed buffer reused at the same
        // address could serve stale entries.
        //
        // Each worker keeps its own memo: the memo is thread-local, and the
        // templates one worker picks up still share what it inferred.
        let (order, ready_after) = self.blade_render_order(blade_uris);
        crate::parallel::for_each_layered(
            "blade-refresh",
            &ready_after,
            crate::type_engine::call_resolution::activate_body_infer_memo,
            |index| {
                let uri = &order[index];
                let Some(content) = self.get_file_content(uri) else {
                    return;
                };
                self.reinfer_and_reparse_blade_with(
                    uri,
                    &content,
                    Some(&shared),
                    Some(&shared_blade),
                );
            },
        );
    }

    /// The templates of a refresh pass, ordered so that a template is
    /// re-inferred after every template that renders it, with how many of
    /// the templates before each one have to finish before it can start.
    ///
    /// A partial's inferred types are read out of the rendering template's
    /// virtual PHP, so that template's own scope has to be settled first:
    /// an `@include('partials.row', ['row' => $row])` inside a
    /// `@foreach ($rows as $row)` only types `$row` once the rendering
    /// template knows what `$rows` holds.
    ///
    /// The order comes in layers: a template whose renderers all sit in
    /// earlier layers reads nothing another template of its own layer
    /// writes, so a whole layer can be re-inferred at once.  The second
    /// vector holds, for each template, where its layer starts.
    ///
    /// Templates that render each other have no such order.  Each is read
    /// against the other's scope as the previous pass left it, and the tie
    /// is broken by URI so that the pass is reproducible rather than
    /// oscillating between two answers.  That needs them re-inferred one
    /// at a time, so each gets a layer of its own.
    fn blade_render_order(&self, mut uris: Vec<String>) -> (Vec<String>, Vec<usize>) {
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
        let mut ready_after: Vec<usize> = Vec::with_capacity(uris.len());
        let mut layer: Vec<usize> = (0..uris.len())
            .filter(|index| renderers[*index] == 0)
            .collect();
        while !layer.is_empty() {
            let layer_start = order.len();
            let mut next: Vec<usize> = Vec::new();
            for &index in &layer {
                for target in std::mem::take(&mut renders[index]) {
                    renderers[target] -= 1;
                    if renderers[target] == 0 {
                        next.push(target);
                    }
                }
            }
            ready_after.extend(std::iter::repeat_n(layer_start, layer.len()));
            order.append(&mut layer);
            next.sort_unstable();
            layer = next;
        }
        // A template rendered from a cycle never runs out of renderers, so
        // whatever the walk did not reach follows it in URI order.
        let placed: HashSet<usize> = order.iter().copied().collect();
        for index in (0..uris.len()).filter(|index| !placed.contains(index)) {
            ready_after.push(order.len());
            order.push(index);
        }

        let order = order
            .into_iter()
            .map(|index| std::mem::take(&mut uris[index]))
            .collect();
        (order, ready_after)
    }

    /// The view names one Blade template renders: the ones its compiled
    /// `@include` / `@each` / `@extends` family names, and the ones the
    /// component each `<x-…>` tag addresses is addressable by.
    fn blade_rendered_view_names(
        &self,
        uri: &str,
        anonymous: &[crate::blade::component_names::AnonymousNamespace],
    ) -> Vec<String> {
        let mut names: Vec<String> = self
            .symbol_map_for(uri)
            .map(|map| {
                map.spans
                    .iter()
                    .filter_map(|span| match &span.kind {
                        SymbolKind::StringKey {
                            kind: StringKeyKind::View,
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
                names.extend(crate::blade::component_names::view_names_for_component_tag(
                    &tag, anonymous,
                ));
            }
        }
        names.sort_unstable();
        names.dedup();
        names
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
        let unchanged = match self.blade.injected_vars.read().get(uri) {
            Some(prev) => *prev == fresh,
            None => fresh == BladeScope::default(),
        };
        if unchanged {
            return false;
        }
        self.blade
            .injected_vars
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
        let Some(map) = self.symbol_map_for(caller_uri) else {
            return;
        };
        let extra = self.typed_receiver_spans_for(caller_uri, &map);
        let mut names: Vec<&str> = map
            .spans
            .iter()
            .chain(extra.iter())
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::View,
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
                &StringKeyKind::View,
                name,
                caller_uri,
            ) {
                let template_uri = location.uri.to_string();
                if self.blade_virtual_php_arc(&template_uri).is_none() {
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
                    &StringKeyKind::View,
                    &name,
                    &caller_uri,
                ) {
                    let template_uri = location.uri.to_string();
                    if !visited.insert(template_uri.clone()) {
                        continue;
                    }
                    if self.blade_virtual_php_arc(&template_uri).is_none() {
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
        let mut frontier: HashSet<String> = self
            .view_names_for_blade_uri(layout_uri)
            .into_iter()
            .collect();
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
            let (children, rest): (Vec<_>, Vec<_>) = pending
                .into_iter()
                .partition(|(_, _, extends)| extends.iter().any(|name| frontier.contains(name)));
            pending = rest;
            frontier.clear();
            for (uri, content, _) in children {
                frontier.extend(self.view_names_for_blade_uri(&uri));
                if self.reinfer_and_reparse_blade(&uri, &content) {
                    self.schedule_diagnostics(uri);
                }
            }
        }
    }
}
