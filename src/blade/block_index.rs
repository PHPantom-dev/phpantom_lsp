//! Pairing section and stack names up across the templates of a project.
//!
//! A name is declared in one file and filled in another: the layout writes
//! `@yield('content')` and every page under it writes `@section('content')`.
//! Both halves are plain strings, so answering "where is the other half?"
//! means knowing which templates render which.
//!
//! Two directions, and they are not symmetrical. Upwards — from a page to
//! the layouts above it — is a short walk of the `@extends` chain the
//! template itself spells out, plus the partials those layouts render, and
//! it costs a handful of file reads. Downwards — from a layout to the pages
//! that fill it — has nothing in the layout to walk, so it needs the
//! project's templates indexed by what they extend.
//!
//! That index is built once from the view roots [`super::discovery`]
//! already walks and then kept current per edit: a template that changes
//! rescans itself rather than throwing the walk away, since the whole point
//! of the index is that it does not run per keystroke.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tower_lsp::lsp_types::{Location, Url};

use crate::Backend;

use super::blocks::{BlockKind, BlockRef, BlockRole, TemplateBlocks};

/// How far the walk of a render tree follows `@include`s.
///
/// A partial that includes a partial that includes a partial is already
/// unusual; past this the tree is either a cycle (which the visited set
/// stops on its own) or deep enough that the name being looked for is not
/// really in it.
const MAX_INCLUDE_DEPTH: usize = 8;

/// One template's contribution to the index.
#[derive(Debug)]
pub(crate) struct IndexedTemplate {
    /// The file the view name renders.
    pub(crate) uri: String,
    pub(crate) blocks: TemplateBlocks,
}

/// The project's templates, both ways round: a walk up a chain looks a
/// layout up by the view name that named it, and everything driven by the
/// file the cursor is in starts from a URI.
#[derive(Debug, Default)]
struct Templates {
    by_view: HashMap<String, Arc<IndexedTemplate>>,
    view_by_uri: HashMap<String, String>,
}

/// Every template of the project, by view name.
#[derive(Debug, Default)]
pub(crate) struct BladeBlockIndex {
    templates: RwLock<Templates>,
}

impl BladeBlockIndex {
    /// The entry for one view name.
    fn get(&self, view: &str) -> Option<Arc<IndexedTemplate>> {
        self.templates.read().by_view.get(view).cloned()
    }

    /// The view name a template file is addressed by.
    fn view_of(&self, uri: &str) -> Option<String> {
        self.templates.read().view_by_uri.get(uri).cloned()
    }

    /// Whether some template of the project renders the one at `uri`
    /// inline, which makes it part of that one's render tree rather than
    /// the root of its own.
    fn is_rendered_by_a_template(&self, uri: &str) -> bool {
        let templates = self.templates.read();
        let Some(view) = templates.view_by_uri.get(uri) else {
            return false;
        };
        templates
            .by_view
            .values()
            .any(|entry| entry.uri != uri && entry.blocks.includes.iter().any(|name| name == view))
    }

    /// Every template that extends `layout`, directly or through another
    /// layout, with the view name each is known by.
    ///
    /// The walk is breadth-first over "who extends what I have found so
    /// far", so a page that extends a layout that extends `layout` is
    /// reached as well. A chain that loops back on itself terminates on the
    /// visited set.
    fn descendants(&self, layout: &str) -> Vec<Arc<IndexedTemplate>> {
        let templates = &self.templates.read().by_view;
        let mut found: Vec<(String, Arc<IndexedTemplate>)> = Vec::new();
        let mut frontier = vec![layout.to_string()];
        let mut seen: Vec<String> = vec![layout.to_string()];
        while let Some(parent) = frontier.pop() {
            for (view, entry) in templates.iter() {
                if !entry.blocks.extends.contains(&parent) || seen.iter().any(|name| name == view) {
                    continue;
                }
                seen.push(view.clone());
                frontier.push(view.clone());
                found.push((view.clone(), Arc::clone(entry)));
            }
        }
        // The map's iteration order is not stable between runs, so the
        // answer a user navigates through would not be either.
        found.sort_by(|(a, _), (b, _)| a.cmp(b));
        found.into_iter().map(|(_, entry)| entry).collect()
    }
}

impl Backend {
    /// The project's section and stack index, built on first use.
    pub(crate) fn blade_block_index(&self) -> Arc<BladeBlockIndex> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.blade_blocks,
            |cache| cache.blade_blocks.clone(),
            |cache, index| cache.blade_blocks = Some(index),
            || Arc::new(self.build_blade_block_index()),
        )
    }

    fn build_blade_block_index(&self) -> BladeBlockIndex {
        let views = self.blade_discovery();
        let mut templates = Templates {
            by_view: HashMap::with_capacity(views.views.len()),
            view_by_uri: HashMap::with_capacity(views.views.len()),
        };
        for (view, path) in &views.views {
            let Ok(uri) = Url::from_file_path(path) else {
                continue;
            };
            let uri = uri.to_string();
            let Some(content) = self.blade_template_source(&uri, path) else {
                continue;
            };
            templates.view_by_uri.insert(uri.clone(), view.clone());
            templates.by_view.insert(
                view.clone(),
                Arc::new(IndexedTemplate {
                    uri,
                    blocks: super::blocks::analyse(&content),
                }),
            );
        }
        BladeBlockIndex {
            templates: RwLock::new(templates),
        }
    }

    /// A template's source, preferring an open buffer so an unsaved edit to
    /// a layout is what its children are paired against.
    fn blade_template_source(&self, uri: &str, path: &std::path::Path) -> Option<String> {
        self.get_file_content(uri)
            .or_else(|| std::fs::read_to_string(path).ok())
    }

    /// Rescan one template's entry after an edit.
    ///
    /// Only when the index is already built: a project whose index nobody
    /// has asked for yet has nothing to keep current, and building it here
    /// would put a walk of every view root on the edit path.
    pub(crate) fn refresh_blade_block_index(&self, uri: &str, content: &str) {
        if !self.is_blade_file(uri) {
            return;
        }
        let Some(index) = self.laravel_string_key_cache.read().blade_blocks.clone() else {
            return;
        };
        // A template the index has never seen is one the discovery
        // refresh above has already dropped the whole index for, or one no
        // view root holds and so no name addresses.
        let Some(view) = index.view_of(uri) else {
            return;
        };
        index.templates.write().by_view.insert(
            view,
            Arc::new(IndexedTemplate {
                uri: uri.to_string(),
                blocks: super::blocks::analyse(content),
            }),
        );
    }

    /// The templates whose names a template shares: the layouts above it
    /// and the partials all of them render, nearest first.
    ///
    /// The template itself leads the list — a `@yield` and the `@section`
    /// that fills it can perfectly well sit in one file — followed by its
    /// `@extends` chain, with each template's own `@include`s walked before
    /// moving up.
    ///
    /// The second half of the answer is whether the walk saw the whole
    /// render tree. It did not when a view name in it is built at runtime,
    /// when a component tag renders a template this cannot follow, or when
    /// the chain ends somewhere that is not the outermost template: a
    /// component, or a partial another template renders, both of which
    /// leave the tree continuing above the top of the `@extends` chain,
    /// where the sections a page fills are just as likely to be rendered.
    pub(crate) fn blade_render_scope(
        &self,
        uri: &str,
        content: &str,
    ) -> (Vec<Arc<IndexedTemplate>>, bool) {
        let index = self.blade_block_index();
        let mut complete = true;
        let mut scope: Vec<Arc<IndexedTemplate>> = Vec::new();
        let mut seen: Vec<String> = Vec::new();

        let mut current = Arc::new(IndexedTemplate {
            uri: uri.to_string(),
            blocks: super::blocks::analyse(content),
        });
        loop {
            seen.push(current.uri.clone());
            if current.blocks.opaque || self.blade_is_component_template(&current) {
                complete = false;
            }
            // The partials a template renders inline see the same sections
            // it does.
            self.walk_includes(&index, &current, &mut scope, &mut seen, &mut complete);
            // `@extends` names candidates and Blade renders the first that
            // exists, which is the first the index knows.
            let layout = current
                .blocks
                .extends
                .iter()
                .find_map(|name| index.get(name));
            if layout.is_none() && !current.blocks.extends.is_empty() {
                // A layout no view root holds: whatever it declares is
                // unreadable from here.
                complete = false;
            }
            // A chain that loops back on itself stops here with what it
            // found; the layout it names is already in the scope.
            let next = layout.filter(|layout| !seen.iter().any(|seen| *seen == layout.uri));
            if next.is_none() && index.is_rendered_by_a_template(&current.uri) {
                // The chain ends at a partial another template renders, so
                // the render tree goes on above it.
                complete = false;
            }
            scope.push(current);
            match next {
                Some(layout) => current = layout,
                None => break,
            }
        }
        (scope, complete)
    }

    /// Whether a template is a component: one a tag in another template
    /// renders, rather than a page.
    ///
    /// Either signal is conclusive on its own, matching
    /// [`crate::blade::template_kind`]: the template declares a component
    /// directive, or it sits in a `components` directory, which is where
    /// both an anonymous component and a class-based component's default
    /// view live.
    fn blade_is_component_template(&self, entry: &IndexedTemplate) -> bool {
        entry.blocks.component || entry.uri.contains("/components/")
    }

    /// Add every template `entry` renders inline, and the ones those
    /// render, to `scope`.
    fn walk_includes(
        &self,
        index: &BladeBlockIndex,
        entry: &Arc<IndexedTemplate>,
        scope: &mut Vec<Arc<IndexedTemplate>>,
        seen: &mut Vec<String>,
        complete: &mut bool,
    ) {
        let mut frontier: Vec<(usize, Vec<String>)> = vec![(0, entry.blocks.includes.clone())];
        while let Some((depth, includes)) = frontier.pop() {
            if depth >= MAX_INCLUDE_DEPTH {
                *complete = false;
                continue;
            }
            for view in includes {
                let Some(included) = index.get(&view) else {
                    *complete = false;
                    continue;
                };
                if seen.iter().any(|seen| *seen == included.uri) {
                    continue;
                }
                seen.push(included.uri.clone());
                if included.blocks.opaque {
                    *complete = false;
                }
                frontier.push((depth + 1, included.blocks.includes.clone()));
                scope.push(included);
            }
        }
    }

    /// The templates that fill what a layout declares: everything under it
    /// in the `@extends` graph.
    pub(crate) fn blade_extending_templates(&self, uri: &str) -> Vec<Arc<IndexedTemplate>> {
        let index = self.blade_block_index();
        let Some(view) = index.view_of(uri) else {
            return Vec::new();
        };
        index.descendants(&view)
    }

    /// Every name a template could sensibly write for `kind`: what the
    /// layouts above it declare (when it fills one) or what the templates
    /// under it fill (when it declares one).
    pub(crate) fn blade_block_name_candidates(
        &self,
        uri: &str,
        content: &str,
        kind: BlockKind,
        role: BlockRole,
    ) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut push = |name: &str| {
            if !names.iter().any(|seen| seen == name) {
                names.push(name.to_string());
            }
        };
        match role {
            // A layout offers what its own pages already fill, so the
            // second `@yield` of a name is spelled like the first.
            BlockRole::Declare => {
                for entry in self.blade_extending_templates(uri) {
                    for name in entry.blocks.filled_names(kind) {
                        push(name);
                    }
                }
            }
            BlockRole::Fill | BlockRole::Check => {
                let (scope, _) = self.blade_render_scope(uri, content);
                for entry in scope {
                    for name in entry.blocks.consumed_names(kind) {
                        push(name);
                    }
                }
            }
        }
        names.sort();
        names
    }

    /// Where the other half of a section or stack name is written.
    ///
    /// A fill (`@section('content')`) resolves to what renders it, a
    /// declaration (`@yield('content')`) to the templates that fill it.
    /// Both directions skip the template the cursor is in: a name has to
    /// lead somewhere else to be worth navigating to.
    pub(crate) fn blade_block_definitions(
        &self,
        uri: &str,
        kind: BlockKind,
        name: &str,
    ) -> Vec<Location> {
        self.blade_block_pairing(uri, kind, name).1
    }

    /// What a template does with a name, and where its other half is.
    ///
    /// The role is the one the template the cursor is in writes: a name it
    /// never writes at all is read as a fill, since that is what a name
    /// being typed into a page is about to become.
    pub(crate) fn blade_block_pairing(
        &self,
        uri: &str,
        kind: BlockKind,
        name: &str,
    ) -> (BlockRole, Vec<Location>) {
        let Some(content) = self.get_file_content(uri) else {
            return (BlockRole::Fill, Vec::new());
        };
        let role = super::blocks::analyse(&content)
            .blocks
            .iter()
            .find(|block| block.kind == kind && block.name == name)
            .map(|block| block.role)
            .unwrap_or(BlockRole::Fill);

        let mut locations = Vec::new();
        match role {
            BlockRole::Declare => {
                for entry in self.blade_extending_templates(uri) {
                    self.push_block_locations(&entry, kind, name, BlockRole::Fill, &mut locations);
                }
            }
            BlockRole::Fill | BlockRole::Check => {
                let (scope, _) = self.blade_render_scope(uri, &content);
                for entry in scope {
                    if entry.uri == uri {
                        continue;
                    }
                    self.push_block_locations(
                        &entry,
                        kind,
                        name,
                        BlockRole::Declare,
                        &mut locations,
                    );
                }
            }
        }
        (role, locations)
    }

    /// Every place the render trees `uri` takes part in write one section
    /// or stack name.
    ///
    /// The project-wide span index cannot answer this: two unrelated pages
    /// both filling `content` fill two different sections, and only the
    /// templates that render each other share a name space. The family is
    /// therefore the template's own render scope plus everything that
    /// extends any of it, and every occurrence in those files counts.
    pub(crate) fn blade_block_references(
        &self,
        uri: &str,
        kind: BlockKind,
        name: &str,
    ) -> Vec<Location> {
        let Some(content) = self.get_file_content(uri) else {
            return Vec::new();
        };
        let (scope, _) = self.blade_render_scope(uri, &content);
        let mut family: Vec<Arc<IndexedTemplate>> = Vec::new();
        for entry in scope {
            for descendant in self.blade_extending_templates(&entry.uri) {
                if !family.iter().any(|seen| seen.uri == descendant.uri) {
                    family.push(descendant);
                }
            }
            if !family.iter().any(|seen| seen.uri == entry.uri) {
                family.push(entry);
            }
        }

        let mut locations = Vec::new();
        for entry in &family {
            for role in [BlockRole::Declare, BlockRole::Fill] {
                self.push_block_locations(entry, kind, name, role, &mut locations);
            }
        }
        locations.sort_by(|a, b| {
            (a.uri.as_str(), a.range.start.line, a.range.start.character).cmp(&(
                b.uri.as_str(),
                b.range.start.line,
                b.range.start.character,
            ))
        });
        locations
    }

    /// Add every place `entry` writes `name` in `role` to `out`.
    ///
    /// A `@hasSection` counts as a declaration for this: it is a template
    /// asking about the name, which is as much a use of it as rendering it.
    fn push_block_locations(
        &self,
        entry: &Arc<IndexedTemplate>,
        kind: BlockKind,
        name: &str,
        role: BlockRole,
        out: &mut Vec<Location>,
    ) {
        let matches = |block: &BlockRef| {
            block.kind == kind
                && block.name == name
                && match role {
                    BlockRole::Declare => {
                        matches!(block.role, BlockRole::Declare | BlockRole::Check)
                    }
                    BlockRole::Fill | BlockRole::Check => block.role == role,
                }
        };
        let spans: Vec<_> = entry
            .blocks
            .blocks
            .iter()
            .filter(|block| matches(block))
            .map(|block| block.name_span.clone())
            .collect();
        if spans.is_empty() {
            return;
        }
        let Ok(url) = Url::parse(&entry.uri) else {
            return;
        };
        let Some(content) = self.get_file_content(&entry.uri) else {
            return;
        };
        for span in spans {
            let range =
                crate::text_position::byte_range_to_lsp_range(&content, span.start, span.end);
            out.push(Location {
                uri: url.clone(),
                range: self.translate_blade_range_to_php(&entry.uri, range),
            });
        }
    }
}
