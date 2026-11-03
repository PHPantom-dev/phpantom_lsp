//! Pairing Twig block names up across templates.
//!
//! A block is defined once and overridden by the templates under it: the
//! ones that extend the template that defines it, however many levels down,
//! the ones that `use` it, and the embeds that embed it.  Upwards, from an
//! override to the block it overrides, is a walk of the `extends` and `use`
//! names the template spells out.  Downwards has nothing to walk, so the
//! templates that write the name at all are found through the reference
//! index (each template's map records its block names as
//! [`StringKeyKind::TwigBlock`] spans) and kept when the walk up from them
//! reaches the template that defines it.
//!
//! Nothing here is cached between requests: a request reads the handful of
//! templates the walk visits, each once.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::Location;

use super::blocks::{BlockRef, BlockRole, TemplateBlocks};
use crate::Backend;
use crate::symbol_map::StringKeyKind;

/// A template, by URI, or one of its embeds: an index into
/// [`TemplateBlocks::scopes`].
type Node = (String, usize);

/// A block name found in a template: the file and the name's bytes.
type Found = (String, Range<usize>);

/// A template's text and the blocks it writes.
type Read = (Arc<String>, Arc<TemplateBlocks>);

/// The templates one request reads, each read once.
struct BlockWorld<'a> {
    backend: &'a Backend,
    /// The files each template name is declared by, gathered on first use.
    named: Option<HashMap<String, Vec<String>>>,
    analysed: HashMap<String, Option<Read>>,
    ancestors: HashMap<Node, Arc<Vec<Node>>>,
}

impl<'a> BlockWorld<'a> {
    fn new(backend: &'a Backend) -> Self {
        Self {
            backend,
            named: None,
            analysed: HashMap::new(),
            ancestors: HashMap::new(),
        }
    }

    /// The files the template name `name` loads.
    fn uris_named(&mut self, name: &str) -> Vec<String> {
        let named = self.named.get_or_insert_with(|| {
            // The names are the declarations of every template, which only
            // the workspace walk has seen.
            self.backend.ensure_workspace_index_ready_for_request();
            let mut named: HashMap<String, Vec<String>> = HashMap::new();
            self.backend.visit_symfony_declarations(
                StringKeyKind::TwigTemplate,
                |uri, _, _, _, key| {
                    named
                        .entry(key.to_string())
                        .or_default()
                        .push(uri.to_string());
                },
            );
            named
        });
        named.get(name).cloned().unwrap_or_default()
    }

    fn read(&mut self, uri: &str) -> Option<Read> {
        if let Some(read) = self.analysed.get(uri) {
            return read.clone();
        }
        let read = self.backend.get_file_content_arc(uri).map(|content| {
            let blocks = Arc::new(super::blocks::analyse(&content));
            (content, blocks)
        });
        self.analysed.insert(uri.to_string(), read.clone());
        read
    }

    fn blocks(&mut self, uri: &str) -> Option<Arc<TemplateBlocks>> {
        self.read(uri).map(|(_, blocks)| blocks)
    }

    /// The nodes a block name written as `block` belongs to: the scope it
    /// is written in, or the template a `block('x', 'other.html.twig')`
    /// names.
    fn owners(&mut self, uri: &str, block: &BlockRef) -> Vec<Node> {
        match &block.template {
            Some(template) => self
                .uris_named(template)
                .into_iter()
                .map(|uri| (uri, 0))
                .collect(),
            None => vec![(uri.to_string(), block.scope)],
        }
    }

    /// The templates `node` pulls blocks in from, each with the name the
    /// block `name` has there and whether it is `use`d rather than
    /// extended, nearest first: the ones it `use`s, the last-used first
    /// since its blocks win, then the ones it extends.
    fn bases(&mut self, node: &Node, name: &str) -> Vec<(Node, String, bool)> {
        let Some(blocks) = self.blocks(&node.0) else {
            return Vec::new();
        };
        let Some(scope) = blocks.scopes.get(node.1) else {
            return Vec::new();
        };
        let mut bases = Vec::new();
        for used in scope.uses.iter().rev() {
            let Some(source) = used.source_name(name) else {
                continue;
            };
            for uri in self.uris_named(&used.template) {
                bases.push(((uri, 0), source.to_string(), true));
            }
        }
        for parent in &scope.parents {
            for uri in self.uris_named(parent) {
                bases.push(((uri, 0), name.to_string(), false));
            }
        }
        bases
    }

    /// Where `node` defines `name` itself.
    fn own_definition(&mut self, node: &Node, name: &str) -> Option<Found> {
        let blocks = self.blocks(&node.0)?;
        blocks
            .refs
            .iter()
            .find(|block| {
                block.scope == node.1 && block.role == BlockRole::Define && block.name == name
            })
            .map(|block| (node.0.clone(), block.span.clone()))
    }

    /// The block `name` renders as in `node`: its own, or else the one it
    /// inherits.
    fn lookup(&mut self, node: &Node, name: &str, seen: &mut HashSet<Node>) -> Vec<Found> {
        if !seen.insert(node.clone()) {
            return Vec::new();
        }
        match self.own_definition(node, name) {
            Some(found) => vec![found],
            None => self.lookup_above(node, name, seen),
        }
    }

    /// The block `name` overrides in `node`: the one the templates it
    /// pulls blocks in from define.  A `use`d template that defines it
    /// settles it; the candidates of a parent chosen at runtime each
    /// contribute theirs.
    fn lookup_above(&mut self, node: &Node, name: &str, seen: &mut HashSet<Node>) -> Vec<Found> {
        let mut found = Vec::new();
        for (base, source, is_used) in self.bases(node, name) {
            let from_base = self.lookup(&base, &source, seen);
            if is_used && !from_base.is_empty() {
                return from_base;
            }
            found.extend(from_base);
        }
        found
    }

    /// The names of the blocks `node` pulls in from the templates above it,
    /// under the names it knows them by.
    fn names_above(&mut self, node: &Node, seen: &mut HashSet<Node>) -> Vec<String> {
        if !seen.insert(node.clone()) {
            return Vec::new();
        }
        let Some(blocks) = self.blocks(&node.0) else {
            return Vec::new();
        };
        let Some(scope) = blocks.scopes.get(node.1) else {
            return Vec::new();
        };
        let mut names = Vec::new();
        for used in &scope.uses {
            for uri in self.uris_named(&used.template) {
                for name in self.names_in(&(uri, 0), seen) {
                    match used.aliases.iter().find(|(from, _)| *from == name) {
                        Some((_, to)) => names.push(to.clone()),
                        None => names.push(name),
                    }
                }
            }
        }
        for parent in &scope.parents {
            for uri in self.uris_named(parent) {
                names.extend(self.names_in(&(uri, 0), seen));
            }
        }
        names
    }

    /// The names of the blocks `node` defines or pulls in.
    fn names_in(&mut self, node: &Node, seen: &mut HashSet<Node>) -> Vec<String> {
        let mut names: Vec<String> = match self.blocks(&node.0) {
            Some(blocks) => blocks.defined_names(node.1).map(str::to_string).collect(),
            None => Vec::new(),
        };
        names.extend(self.names_above(node, seen));
        names
    }

    /// `node` and every template it pulls blocks in from, directly or not.
    fn ancestors(&mut self, node: &Node) -> Arc<Vec<Node>> {
        if let Some(known) = self.ancestors.get(node) {
            return Arc::clone(known);
        }
        let mut found: Vec<Node> = vec![node.clone()];
        let mut seen: HashSet<Node> = HashSet::from([node.clone()]);
        let mut next = 0;
        while let Some(current) = found.get(next).cloned() {
            next += 1;
            let Some(blocks) = self.blocks(&current.0) else {
                continue;
            };
            let Some(scope) = blocks.scopes.get(current.1) else {
                continue;
            };
            let names: Vec<String> = scope
                .uses
                .iter()
                .map(|used| used.template.clone())
                .chain(scope.parents.iter().cloned())
                .collect();
            for name in names {
                for uri in self.uris_named(&name) {
                    let base = (uri, 0);
                    if seen.insert(base.clone()) {
                        found.push(base);
                    }
                }
            }
        }
        let found = Arc::new(found);
        self.ancestors.insert(node.clone(), Arc::clone(&found));
        found
    }

    /// Every template the reference index says writes the block name
    /// `name`, with `also` among them.
    fn candidates(&mut self, name: &str, also: &str) -> Vec<String> {
        let key = crate::reference_index::string_key_reference_key(StringKeyKind::TwigBlock, name);
        let mut uris: Vec<String> = self
            .backend
            .user_file_symbol_maps_for_reference_keys(&[key])
            .into_iter()
            .map(|(uri, _)| uri)
            .collect();
        if !uris.iter().any(|uri| uri == also) {
            uris.push(also.to_string());
        }
        uris.sort_unstable();
        uris
    }

    /// The blocks that override the block `name` defines in `node`, however
    /// many templates down.
    fn overrides(&mut self, node: &Node, name: &str) -> Vec<Found> {
        let mut found = Vec::new();
        for uri in self.candidates(name, &node.0) {
            let Some(blocks) = self.blocks(&uri) else {
                continue;
            };
            let mut scopes_seen: Vec<usize> = Vec::new();
            for block in &blocks.refs {
                if block.role != BlockRole::Define
                    || block.name != name
                    || scopes_seen.contains(&block.scope)
                {
                    continue;
                }
                scopes_seen.push(block.scope);
                let overrider = (uri.clone(), block.scope);
                if overrider != *node && self.ancestors(&overrider).contains(node) {
                    found.push((uri.clone(), block.span.clone()));
                }
            }
        }
        found
    }

    /// Every place the templates that share the block `name` with `owners`
    /// write it: the ones above them, and every one under any of those.
    fn family(&mut self, owners: &[Node], name: &str, also: &str) -> Vec<Found> {
        let mut related: HashSet<Node> = HashSet::new();
        for owner in owners {
            related.extend(self.ancestors(owner).iter().cloned());
        }
        let mut found = Vec::new();
        for uri in self.candidates(name, also) {
            let Some(blocks) = self.blocks(&uri) else {
                continue;
            };
            for block in blocks.refs.iter().filter(|block| block.name == name) {
                let shares = self
                    .owners(&uri, block)
                    .iter()
                    .any(|owner| self.ancestors(owner).iter().any(|n| related.contains(n)));
                if shares {
                    found.push((uri.clone(), block.span.clone()));
                }
            }
        }
        found
    }

    /// Where the block the name at `block` refers to is defined.  A block
    /// that overrides nothing leads to the blocks that override it instead.
    fn definitions(&mut self, uri: &str, block: &BlockRef) -> Vec<Found> {
        let mut seen = HashSet::new();
        let owners = self.owners(uri, block);
        match block.role {
            BlockRole::Define => {
                let node = &owners[0];
                let above = self.lookup_above(node, &block.name, &mut seen);
                if above.is_empty() {
                    self.overrides(node, &block.name)
                } else {
                    above
                }
            }
            BlockRole::Parent => self.lookup_above(&owners[0], &block.name, &mut seen),
            BlockRole::Render => owners
                .iter()
                .flat_map(|owner| self.lookup(owner, &block.name, &mut seen))
                .collect(),
        }
    }

    fn locations(&mut self, found: Vec<Found>) -> Vec<Location> {
        let mut locations: Vec<Location> = found
            .into_iter()
            .filter_map(|(uri, span)| {
                let (content, _) = self.read(&uri)?;
                Some(Location {
                    uri: crate::util::parse_url(&uri)?,
                    range: crate::text_position::byte_range_to_lsp_range(
                        &content, span.start, span.end,
                    ),
                })
            })
            .collect();
        locations.sort_by(|a, b| {
            (a.uri.as_str(), a.range.start.line, a.range.start.character).cmp(&(
                b.uri.as_str(),
                b.range.start.line,
                b.range.start.character,
            ))
        });
        locations.dedup();
        locations
    }
}

impl Backend {
    /// The block name the template at `uri` writes at `offset`, or the
    /// first time it writes `name` when there is no offset.
    fn twig_block_at(
        &self,
        world: &mut BlockWorld<'_>,
        uri: &str,
        offset: Option<usize>,
        name: &str,
    ) -> Option<BlockRef> {
        let blocks = world.blocks(uri)?;
        match offset {
            Some(offset) => blocks.ref_at(offset),
            None => blocks.refs.iter().find(|block| block.name == name),
        }
        .filter(|block| block.name == name)
        .cloned()
    }

    /// Where the block the name at `offset` in the template at `uri`
    /// refers to is defined: the block a `{% block %}` overrides (or, when
    /// it overrides none, the blocks that override it), the block a
    /// `block()` renders, and the block a `parent()` renders.
    pub(crate) fn twig_block_definitions(
        &self,
        uri: &str,
        offset: Option<usize>,
        name: &str,
    ) -> Vec<Location> {
        let mut world = BlockWorld::new(self);
        let Some(block) = self.twig_block_at(&mut world, uri, offset, name) else {
            return Vec::new();
        };
        let found = world.definitions(uri, &block);
        world.locations(found)
    }

    /// Every place the templates that share the block named at `offset`
    /// write its name.
    ///
    /// Two unrelated templates that both define `body` define two
    /// different blocks, so this is not the reference index's project-wide
    /// answer: only the templates above the one the name is written in,
    /// and the ones under any of those, share it.
    pub(crate) fn twig_block_references(
        &self,
        uri: &str,
        offset: Option<usize>,
        name: &str,
    ) -> Vec<Location> {
        let mut world = BlockWorld::new(self);
        let Some(block) = self.twig_block_at(&mut world, uri, offset, name) else {
            return Vec::new();
        };
        let owners = world.owners(uri, &block);
        let found = world.family(&owners, name, uri);
        world.locations(found)
    }

    /// What a block's hover says: the block it overrides or the ones that
    /// override it, or where the block a call renders is defined.
    pub(crate) fn twig_block_detail(&self, uri: &str, offset: Option<usize>, name: &str) -> String {
        let mut world = BlockWorld::new(self);
        let Some(block) = self.twig_block_at(&mut world, uri, offset, name) else {
            return "Block".to_string();
        };
        let mut seen = HashSet::new();
        let owners = world.owners(uri, &block);
        let (found, opening, none) = match block.role {
            BlockRole::Define => {
                let above = world.lookup_above(&owners[0], name, &mut seen);
                if above.is_empty() {
                    (
                        world.overrides(&owners[0], name),
                        "Overridden in",
                        "Not overridden by any template",
                    )
                } else {
                    (above, "Overrides the block in", "")
                }
            }
            BlockRole::Parent => (
                world.lookup_above(&owners[0], name, &mut seen),
                "Renders the block in",
                "Overrides no block",
            ),
            BlockRole::Render => (
                world.definitions(uri, &block),
                "Defined in",
                "Not defined by the template or the ones it extends",
            ),
        };
        let mut paths: Vec<String> = found
            .iter()
            .map(|(uri, _)| {
                self.workspace_relative_path(uri)
                    .unwrap_or_else(|| uri.clone())
            })
            .collect();
        paths.dedup();
        match paths.as_slice() {
            [] => none.to_string(),
            [only] => format!("{opening} `{only}`"),
            [first, rest @ ..] => format!("{opening} `{first}` and {} more", rest.len()),
        }
    }

    /// The block names worth offering where the template at `uri` is typing
    /// one at `cursor`: the blocks it can override for a `{% block %}` tag,
    /// leaving out the ones it already does, and every block it renders for
    /// a `block()` call.
    pub(crate) fn twig_block_name_candidates(
        &self,
        uri: &str,
        content: &str,
        cursor: usize,
        is_tag: bool,
    ) -> Vec<String> {
        let blocks = super::blocks::analyse(content);
        let scope = blocks.scope_at(cursor);
        let typed = blocks.ref_at(cursor).map(|block| block.span.start);
        let own: Vec<&str> = blocks
            .refs
            .iter()
            .filter(|block| {
                block.scope == scope
                    && block.role == BlockRole::Define
                    && Some(block.span.start) != typed
            })
            .map(|block| block.name.as_str())
            .collect();

        let mut world = BlockWorld::new(self);
        // The buffer being typed in, not what the file held before.
        world.analysed.insert(
            uri.to_string(),
            Some((Arc::new(content.to_string()), Arc::new(blocks.clone()))),
        );
        let node: Node = (uri.to_string(), scope);
        let mut names = world.names_above(&node, &mut HashSet::new());
        if is_tag {
            names.retain(|name| !own.contains(&name.as_str()));
        } else {
            names.extend(own.iter().map(|name| name.to_string()));
        }
        names.sort_unstable();
        names.dedup();
        names
    }
}
