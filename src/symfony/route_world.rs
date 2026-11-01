//! The route names a Symfony application has, by following its routing
//! configuration through every import.
//!
//! Routes are a closed world the project spells out.  The kernel loads
//! `config/routes.yaml` and the files under `config/routes/`, and every
//! other route enters through an import in one of them: a directory of
//! controllers whose `#[Route]` attributes declare routes, another routing
//! file, or a bundle's own (`@WebProfilerBundle/Resources/config/routing/wdt.php`).
//! Each file that declares or imports a route is a node, and each import an
//! edge that puts its `name_prefix` in front of every name the files it
//! loads declare.  A bundle's routing files are not part of the workspace
//! index, so they are read from disk (and kept while they are unchanged).
//!
//! A route is declared under every prefix the imports that reach its file
//! give it, and under its own name too when nothing reaches the file (an
//! import this cannot see, such as one a kernel builds from variables, is
//! the likelier story than a route nobody loads).  An import whose routes
//! cannot be read (a service or custom loader, a resource that is not a
//! literal, a file that is not there) leaves the names under its prefix
//! open, and so does a declaration whose name is not a literal.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::RwLock;

use crate::Backend;
use crate::symbol_map::{RouteImport, StringKeyKind, SymbolKind, SymbolMap};

/// The service that loads the logout route of every firewall, which it
/// names `_logout_` and the firewall's name.
const LOGOUT_ROUTE_LOADER: &str = "security.route_loader.logout";

/// The loaders whose routes are read: the file loaders, by `type` or by the
/// file's extension, and the attribute loaders.
const READ_LOADERS: &[&str] = &[
    "attribute",
    "annotation",
    "yaml",
    "yml",
    "xml",
    "php",
    "directory",
    "glob",
];

/// The routing files read from disk, by path.
#[derive(Default)]
pub(crate) struct ForeignRoutingFiles {
    files: RwLock<HashMap<PathBuf, ForeignRoutingFile>>,
}

/// One routing file read from disk: what it declares, or `None` when it
/// declares and imports no route, kept while its modification time and
/// length are the ones it was read at.
struct ForeignRoutingFile {
    modified: Option<SystemTime>,
    len: u64,
    map: Option<Arc<SymbolMap>>,
}

/// One file that declares or imports routes.
struct Node {
    uri: String,
    path: Option<PathBuf>,
    map: Arc<SymbolMap>,
    /// Whether the map is the workspace index's, whose configurator calls
    /// the type engine confirms, rather than one read from disk.
    indexed: bool,
    /// Whether the file declares a route with an attribute or annotation,
    /// which is what an attribute import loads.
    has_attribute_routes: bool,
}

/// What one import loads.
enum Loaded {
    /// The files, by node.
    Files(Vec<usize>),
    /// Routes that cannot be read, all named under this, after the
    /// import's own name prefix.
    Open(&'static str),
}

/// The routes the project has: every file that declares or imports one, and
/// the name prefixes each file's routes take.
pub(crate) struct RouteWorld {
    nodes: Vec<Node>,
    prefixes: Vec<Vec<String>>,
    /// The prefixes under which a name cannot be told undeclared, sorted.
    open_prefixes: Vec<String>,
    /// Whether the routing configuration the kernel loads was found, without
    /// which no name can be told undeclared.
    has_root: bool,
}

impl RouteWorld {
    /// Whether a route name `key` that nothing declares is a mistake.
    pub(crate) fn can_judge(&self, key: &str) -> bool {
        self.has_root
            && !self
                .open_prefixes
                .iter()
                .any(|prefix| key.starts_with(prefix.as_str()))
    }
}

/// Whether `uri` is routing configuration the kernel loads itself:
/// `config/routes.yaml` (or `.php`), or a file under `config/routes/`.
fn is_kernel_routing_uri(uri: &str) -> bool {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    let mut segments = path.split('/').peekable();
    while let Some(segment) = segments.next() {
        if segment != "config" {
            continue;
        }
        let Some(next) = segments.next() else {
            return false;
        };
        if next == "routes" && segments.peek().is_some() {
            return true;
        }
        if next
            .strip_prefix("routes.")
            .is_some_and(|ext| matches!(ext, "yaml" | "yml" | "xml" | "php"))
        {
            return true;
        }
    }
    false
}

fn is_routing_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "yaml" | "yml" | "xml" | "php"))
}

fn is_php(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "php")
}

/// Every file under `dir` that `keep` accepts, sorted.
fn files_under(dir: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
            } else if keep(&path) {
                found.push(path);
            }
        }
    }
    found.sort_unstable();
    found
}

/// Whether the route-declaring span `kind` is a declared route name.
fn declared_route(kind: &SymbolKind) -> Option<&str> {
    match kind {
        SymbolKind::StringKey {
            kind: StringKeyKind::SymfonyRoute,
            key,
            is_write: true,
            ..
        } => Some(key),
        _ => None,
    }
}

/// Whether `map` declares or imports a route.
fn holds_routes(map: &SymbolMap) -> bool {
    !map.route_imports.is_empty()
        || map
            .spans
            .iter()
            .any(|span| declared_route(&span.kind).is_some())
        || map
            .typed_receiver_sites
            .iter()
            .any(|site| site.is_write && site.receiver.declares_routes())
}

/// What a routing file read from disk declares and imports.  Its
/// configurator calls are taken as they are: an import named the file as
/// routing configuration.
fn read_routing_file(path: &Path, content: &str) -> Option<SymbolMap> {
    let uri = crate::util::path_to_uri(path);
    let (spans, route_imports) = if is_php(path) {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(uri.as_bytes());
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        let map = crate::symbol_map::extract_symbol_map_for_index(
            program,
            content,
            &names,
            crate::symbol_map::extraction::Frameworks {
                symfony: true,
                service_container: true,
                drupal: false,
                twig: false,
            },
            None,
        );
        let mut spans: Vec<_> = map
            .spans
            .into_iter()
            .filter(|span| declared_route(&span.kind).is_some())
            .collect();
        spans.extend(
            map.typed_receiver_sites
                .iter()
                .filter(|site| site.is_write && site.receiver.declares_routes())
                .map(|site| site.to_span(StringKeyKind::SymfonyRoute)),
        );
        spans.sort_by_key(|span| span.start);
        (spans, map.route_imports)
    } else {
        super::routes::routing_declarations(&uri, content)
    };
    let map = SymbolMap {
        spans,
        route_imports,
        source_len: u32::try_from(content.len()).unwrap_or(u32::MAX),
        ..SymbolMap::default()
    };
    holds_routes(&map).then_some(map)
}

/// The bundle a `@Name/…` resource names, by the class `config/bundles.php`
/// registers it under, or Symfony's own spelling of the name.
fn bundle_class(root: &Path, name: &str) -> String {
    let registered = std::fs::read_to_string(root.join("config/bundles.php"))
        .ok()
        .and_then(|bundles| {
            let suffix = format!("\\{name}::class");
            let end = bundles.find(&suffix)? + suffix.len() - "::class".len();
            let start = bundles[..end]
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\\'))
                .map_or(0, |index| index + 1);
            Some(bundles[start..end].trim_start_matches('\\').to_string())
        });
    registered.unwrap_or_else(|| format!("Symfony\\Bundle\\{name}\\{name}"))
}

impl Backend {
    /// The routing file at `path`, read from disk.
    fn foreign_routing_file(&self, path: &Path) -> Option<Arc<SymbolMap>> {
        let metadata = std::fs::metadata(path).ok()?;
        let (modified, len) = (metadata.modified().ok(), metadata.len());
        let cache = &self.symfony.foreign_routing.files;
        if let Some(cached) = cache.read().get(path)
            && cached.modified == modified
            && cached.len == len
        {
            return cached.map.clone();
        }
        let map = std::fs::read_to_string(path)
            .ok()
            .and_then(|content| read_routing_file(path, &content))
            .map(Arc::new);
        cache.write().insert(
            path.to_path_buf(),
            ForeignRoutingFile {
                modified,
                len,
                map: map.clone(),
            },
        );
        map
    }

    /// Whether `path` is one the workspace index holds, rather than a
    /// dependency's.
    fn is_indexed_path(&self, path: &Path) -> bool {
        let uri = crate::util::path_to_uri(path);
        !self
            .workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
    }

    /// The file a class is declared in.
    fn class_file(&self, fqn: &str) -> Option<PathBuf> {
        let uri = self.class_declaration_uri(fqn).or_else(|| {
            self.find_or_load_class(fqn);
            self.class_declaration_uri(fqn)
        })?;
        crate::util::uri_to_path(&uri)
    }

    /// The path a `@Name/rest` resource names inside a bundle: under the
    /// directory of the bundle's class, or the one above it for a bundle
    /// laid out the modern way.
    fn bundle_resource_path(&self, resource: &str) -> Option<PathBuf> {
        let (name, rest) = resource.strip_prefix('@')?.split_once('/')?;
        let root = self.workspace_root().read().clone()?;
        let class_dir = self.class_file(&bundle_class(&root, name))?;
        let class_dir = class_dir.parent()?;
        [Some(class_dir), class_dir.parent()]
            .into_iter()
            .flatten()
            .map(|dir| dir.join(rest))
            .find(|path| path.exists())
    }

    /// Every file that declares or imports a route, with the name prefixes
    /// its routes take.
    pub(crate) fn symfony_route_world(&self) -> RouteWorld {
        let mut nodes: Vec<Node> = Vec::new();
        let mut by_path: HashMap<PathBuf, usize> = HashMap::new();
        let mut uris: Vec<String> = self.symfony.declaring_uris.read().iter().cloned().collect();
        uris.sort_unstable();
        for uri in uris {
            let Some(map) = self.symbol_map_for(&uri) else {
                continue;
            };
            if !holds_routes(&map) {
                continue;
            }
            let path = crate::util::uri_to_path(&uri);
            if let Some(path) = &path {
                by_path.insert(path.clone(), nodes.len());
            }
            let has_attribute_routes = path.as_deref().is_some_and(is_php)
                && map
                    .spans
                    .iter()
                    .any(|span| declared_route(&span.kind).is_some());
            nodes.push(Node {
                uri,
                path,
                map,
                indexed: true,
                has_attribute_routes,
            });
        }
        let has_root = nodes.iter().any(|node| is_kernel_routing_uri(&node.uri));

        // Imports reach files read from disk, which import others in turn.
        let mut edges: Vec<(usize, usize, String)> = Vec::new();
        let mut opens: Vec<(usize, String)> = Vec::new();
        let mut next = 0;
        while next < nodes.len() {
            let importer = next;
            next += 1;
            let imports = nodes[importer].map.route_imports.clone();
            for import in &imports {
                match self.load_import(importer, import, &mut nodes, &mut by_path) {
                    Loaded::Files(targets) => edges.extend(
                        targets
                            .into_iter()
                            .map(|target| (importer, target, import.name_prefix.clone())),
                    ),
                    Loaded::Open(suffix) => {
                        opens.push((importer, format!("{}{suffix}", import.name_prefix)));
                    }
                }
            }
        }

        let mut into: Vec<Vec<(usize, &str)>> = vec![Vec::new(); nodes.len()];
        for (from, to, prefix) in &edges {
            into[*to].push((*from, prefix.as_str()));
        }
        let mut memo: Vec<Option<Vec<String>>> = vec![None; nodes.len()];
        let mut visiting = vec![false; nodes.len()];
        for node in 0..nodes.len() {
            node_prefixes(node, &nodes, &into, &mut memo, &mut visiting);
        }
        let prefixes: Vec<Vec<String>> = memo.into_iter().map(Option::unwrap_or_default).collect();

        let mut open_prefixes: Vec<String> = opens
            .iter()
            .flat_map(|(importer, prefix)| {
                prefixes[*importer]
                    .iter()
                    .map(move |outer| format!("{outer}{prefix}"))
            })
            .collect();
        open_prefixes.sort_unstable();
        open_prefixes.dedup();

        RouteWorld {
            nodes,
            prefixes,
            open_prefixes,
            has_root,
        }
    }

    /// The node of the file at `path`, read from disk when the index does
    /// not hold its routes.
    fn route_file_node(
        &self,
        path: &Path,
        nodes: &mut Vec<Node>,
        by_path: &mut HashMap<PathBuf, usize>,
    ) -> Option<usize> {
        if let Some(&node) = by_path.get(path) {
            return Some(node);
        }
        // A PHP file of the workspace the index has read and found no routes
        // in has none.
        if is_php(path)
            && self.is_indexed_path(path)
            && self
                .symbol_map_for(&crate::util::path_to_uri(path))
                .is_some()
        {
            return None;
        }
        let map = self.foreign_routing_file(path)?;
        let has_attribute_routes = is_php(path)
            && map
                .spans
                .iter()
                .any(|span| declared_route(&span.kind).is_some());
        by_path.insert(path.to_path_buf(), nodes.len());
        nodes.push(Node {
            uri: crate::util::path_to_uri(path),
            path: Some(path.to_path_buf()),
            map,
            indexed: false,
            has_attribute_routes,
        });
        Some(nodes.len() - 1)
    }

    /// The files `import`, written in `nodes[importer]`, loads.
    fn load_import(
        &self,
        importer: usize,
        import: &RouteImport,
        nodes: &mut Vec<Node>,
        by_path: &mut HashMap<PathBuf, usize>,
    ) -> Loaded {
        let Some(resource) = import.resource.as_deref() else {
            return Loaded::Open("");
        };
        let loader = import.loader.as_deref();
        if loader == Some("service") {
            return Loaded::Open(if resource == LOGOUT_ROUTE_LOADER {
                "_logout_"
            } else {
                ""
            });
        }
        if loader.is_some_and(|loader| !READ_LOADERS.contains(&loader)) {
            return Loaded::Open("");
        }
        // The controllers the container registers, which are the project's.
        if resource == "routing.controllers" {
            return Loaded::Files(
                (0..nodes.len())
                    .filter(|&node| nodes[node].indexed && nodes[node].has_attribute_routes)
                    .collect(),
            );
        }

        let path = if resource.starts_with('@') {
            self.bundle_resource_path(resource)
        } else if !resource.contains('/') && resource.contains('\\') {
            self.class_file(resource)
        } else {
            let dir = nodes[importer].path.as_deref().and_then(Path::parent);
            dir.map(|dir| crate::document_links::normalize_path(&dir.join(resource)))
        };
        let Some(path) = path else {
            return Loaded::Open("");
        };

        let path_text = path.to_string_lossy();
        if let Some(meta) = path_text.find(['*', '?', '{', '[']) {
            let Ok(glob) = globset::GlobBuilder::new(&path_text)
                .literal_separator(true)
                .build()
            else {
                return Loaded::Open("");
            };
            let matcher = glob.compile_matcher();
            let Some(base) = path_text[..meta]
                .rfind('/')
                .filter(|&slash| slash > 0)
                .map(|slash| PathBuf::from(&path_text[..slash]))
            else {
                return Loaded::Open("");
            };
            // The workspace's own files are all in the index already.
            if self.is_indexed_path(&base) {
                return Loaded::Files(
                    (0..nodes.len())
                        .filter(|&node| {
                            nodes[node]
                                .path
                                .as_deref()
                                .is_some_and(|file| matcher.is_match(file))
                        })
                        .collect(),
                );
            }
            let files = files_under(&base, &|file| {
                is_routing_extension(file) && matcher.is_match(file)
            });
            return Loaded::Files(
                files
                    .iter()
                    .filter_map(|file| self.route_file_node(file, nodes, by_path))
                    .collect(),
            );
        }

        if path.is_dir() {
            // A directory without a `type` is loaded for its attributes.
            if loader != Some("directory") {
                if self.is_indexed_path(&path) {
                    return Loaded::Files(
                        (0..nodes.len())
                            .filter(|&node| {
                                nodes[node].has_attribute_routes
                                    && nodes[node]
                                        .path
                                        .as_deref()
                                        .is_some_and(|file| file.starts_with(&path))
                            })
                            .collect(),
                    );
                }
                let files = files_under(&path, &is_php);
                return Loaded::Files(
                    files
                        .iter()
                        .filter_map(|file| self.route_file_node(file, nodes, by_path))
                        .collect(),
                );
            }
            let files = files_under(&path, &is_routing_extension);
            return Loaded::Files(
                files
                    .iter()
                    .filter_map(|file| self.route_file_node(file, nodes, by_path))
                    .collect(),
            );
        }
        if path.is_file() {
            return Loaded::Files(
                self.route_file_node(&path, nodes, by_path)
                    .into_iter()
                    .collect(),
            );
        }
        // Not there: a dependency that is not installed.
        Loaded::Open("")
    }

    /// Visit every route the project declares, under each name the imports
    /// that reach it give it: the file, its map, the byte range of the
    /// declaration, and the name.
    pub(crate) fn visit_route_declarations(
        &self,
        world: &RouteWorld,
        mut visit: impl FnMut(&str, &SymbolMap, u32, u32, &str),
    ) {
        let mut name = String::new();
        for (node, prefixes) in world.nodes.iter().zip(&world.prefixes) {
            let typed = if node.indexed {
                self.typed_receiver_spans_for(&node.uri, &node.map)
            } else {
                Arc::default()
            };
            for span in node.map.spans.iter().chain(typed.iter()) {
                let Some(key) = declared_route(&span.kind) else {
                    continue;
                };
                for prefix in prefixes {
                    name.clear();
                    name.push_str(prefix);
                    name.push_str(key);
                    visit(&node.uri, &node.map, span.start, span.end, &name);
                }
            }
        }
    }
}

/// The name prefixes the routes of `node` take: none for a file nothing
/// imports or the kernel loads itself, and each importer's own after the
/// one its import adds.  An import cycle, which Symfony refuses to load,
/// contributes nothing.
fn node_prefixes(
    node: usize,
    nodes: &[Node],
    into: &[Vec<(usize, &str)>],
    memo: &mut [Option<Vec<String>>],
    visiting: &mut [bool],
) -> Vec<String> {
    if let Some(known) = &memo[node] {
        return known.clone();
    }
    if visiting[node] {
        return Vec::new();
    }
    visiting[node] = true;
    let mut prefixes: Vec<String> = Vec::new();
    if into[node].is_empty() || (nodes[node].indexed && is_kernel_routing_uri(&nodes[node].uri)) {
        prefixes.push(String::new());
    }
    for &(importer, prefix) in &into[node] {
        for outer in node_prefixes(importer, nodes, into, memo, visiting) {
            prefixes.push(format!("{outer}{prefix}"));
        }
    }
    let mut seen = HashSet::new();
    prefixes.retain(|prefix| seen.insert(prefix.clone()));
    visiting[node] = false;
    memo[node] = Some(prefixes.clone());
    prefixes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kernel_loads_its_own_routing_configuration() {
        assert!(is_kernel_routing_uri("file:///p/config/routes.yaml"));
        assert!(is_kernel_routing_uri("file:///p/config/routes.php"));
        assert!(is_kernel_routing_uri("file:///p/config/routes/admin.yaml"));
        assert!(is_kernel_routing_uri(
            "file:///p/config/routes/dev/web_profiler.yaml"
        ));
        assert!(!is_kernel_routing_uri(
            "file:///p/src/Resources/config/routing/blog.xml"
        ));
        assert!(!is_kernel_routing_uri("file:///p/config/services.yaml"));
    }

    #[test]
    fn a_registered_bundle_is_found_by_its_class() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/bundles.php"),
            "<?php\nreturn [\n    Acme\\BlogBundle\\AcmeBlogBundle::class => ['all' => true],\n];\n",
        )
        .unwrap();
        assert_eq!(
            bundle_class(dir.path(), "AcmeBlogBundle"),
            "Acme\\BlogBundle\\AcmeBlogBundle"
        );
        assert_eq!(
            bundle_class(dir.path(), "WebProfilerBundle"),
            "Symfony\\Bundle\\WebProfilerBundle\\WebProfilerBundle"
        );
    }
}
