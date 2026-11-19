//! Statically known Tempest discovery locations and component registrations.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mago_span::HasSpan;
use mago_syntax::{cst::*, walker::Walker};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::php_type::{PhpType, TypeKind};
use crate::resolution::FileLoaders;
use crate::symbol_map::SymbolKind;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Configuration {
    /// Extra roots contributed by discovery configuration.
    pub(super) locations: Vec<Location>,
    /// Explicit names and the component files they identify.
    pub(super) components: BTreeMap<String, Component>,
    /// Files and directories excluded from discovery with `skipPaths()`.
    pub(super) skipped: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Location {
    /// Canonical directory to scan.
    pub(super) path: PathBuf,
    /// Subtrees this location excludes.
    pub(super) ignore: Vec<PathBuf>,
    /// Whether project components may override these discoveries.
    pub(super) vendor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Component {
    /// File backing a registered component.
    pub(super) path: PathBuf,
    /// Tempest's project-over-vendor precedence marker.
    pub(super) vendor: bool,
}

/// Config files can be outside the PHP index; other PHP sources contribute
/// only when their indexed symbols name the relevant APIs.
pub(super) fn sources(backend: &Backend, catalog: &super::discovery::Catalog) -> BTreeSet<String> {
    let mut sources: BTreeSet<_> = catalog
        .config_files
        .iter()
        .map(|path| crate::util::path_to_uri(path))
        .collect();
    for (uri, file) in backend.files.read().iter() {
        if super::is_view(uri) {
            continue;
        }
        if uri.ends_with(".config.php") {
            // New and unsaved configuration files may not be in the cached
            // filesystem scan. A removed discovery root must still stop
            // contributing its previously indexed configurations.
            if crate::util::uri_to_path(uri).is_some_and(|path| {
                catalog
                    .discovery_kind(&crate::util::canonical_path_for_comparison(&path))
                    .is_some()
            }) {
                sources.insert(uri.clone());
            }
            continue;
        }
        if file.symbol_map.as_ref().is_some_and(|map| {
            map.spans.iter().any(|span| match &span.kind {
                SymbolKind::ClassReference { name, .. } => [
                    "Tempest\\View\\ViewConfig",
                    "Tempest\\Discovery\\DiscoveryConfig",
                    "Tempest\\Discovery\\DiscoveryLocation",
                ]
                .iter()
                .any(|class| name.trim_start_matches('\\').eq_ignore_ascii_case(class)),
                SymbolKind::MemberAccess {
                    member_name,
                    is_method_call: true,
                    ..
                } => ["addViewComponent", "addViewComponents", "skipPaths"]
                    .iter()
                    .any(|method| member_name.eq_ignore_ascii_case(method)),
                _ => false,
            })
        }) {
            sources.insert(uri.clone());
        }
    }
    let open: BTreeMap<_, _> = backend
        .open_files
        .read()
        .keys()
        .map(|uri| (super::discovery::canonical_uri(uri), uri.clone()))
        .collect();
    sources
        .into_iter()
        .map(|uri| {
            let canonical = super::discovery::canonical_uri(&uri);
            open.get(&canonical).cloned().unwrap_or(canonical)
        })
        .collect()
}

/// Read configuration values through the declaring file's PHP loaders.
pub(super) fn collect(backend: &Backend, sources: &BTreeSet<String>) -> Configuration {
    let mut config = Configuration::default();
    let root = backend
        .workspace
        .workspace_root
        .read()
        .clone()
        .unwrap_or_default();
    for uri in sources {
        let Some(content) = backend.get_file_content_arc(uri) else {
            continue;
        };
        if backend
            .symbol_map_for(uri)
            .is_none_or(|map| !map.built_from(&content))
        {
            backend.update_ast(uri, &content);
        }
        let file = backend.file_context(uri);
        let loaders = backend.file_loaders(&file);
        let _engine = crate::type_engine::call_resolution::activate_type_engine_caches();
        let _parse = crate::parser::with_parse_cache(&content);
        crate::parser::with_parsed_program(
            &content,
            "tempest_configuration",
            |program, content| {
                let mut calls = ConfigCalls::default();
                Calls.walk_program(program, &mut calls);
                let reader = Reader {
                    content,
                    loaders: &loaders,
                    file: &file,
                    root: &root,
                    map: backend.symbol_map_for(uri),
                };
                for expr in &calls.calls {
                    match expr {
                        Expression::Instantiation(new)
                            if reader
                                .is_class(new.class, "Tempest\\Discovery\\DiscoveryLocation") =>
                        {
                            let Some(args) = &new.argument_list else {
                                continue;
                            };
                            let Some(path) =
                                argument(args, "path", 1).and_then(|expr| reader.path(expr))
                            else {
                                continue;
                            };
                            let namespace = argument(args, "namespace", 0)
                                .and_then(|expr| reader.string(expr))
                                .unwrap_or_default();
                            let ignore = argument(args, "ignore", 2)
                                .map(|expr| {
                                    let ty =
                                        super::view_data::expression_type(expr, content, &loaders);
                                    let mut paths = Vec::new();
                                    reader.type_paths(&ty, &mut paths, false);
                                    paths
                                })
                                .unwrap_or_default();
                            let vendor = namespace.starts_with("Tempest") || is_vendor_path(&path);
                            config.locations.push(Location {
                                path,
                                ignore,
                                vendor,
                            });
                        }
                        Expression::Instantiation(new)
                            if reader.is_class(new.class, "Tempest\\View\\ViewConfig") =>
                        {
                            if let Some(expr) = new
                                .argument_list
                                .as_ref()
                                .and_then(|args| argument(args, "viewComponents", 0))
                            {
                                reader.components(expr, &mut config.components, &calls, true);
                            }
                        }
                        Expression::Call(Call::Method(call)) => {
                            let ClassLikeMemberSelector::Identifier(method) = &call.method else {
                                continue;
                            };
                            let method = bytes_to_str(method.value);
                            if (method.eq_ignore_ascii_case("addViewComponent")
                                || method.eq_ignore_ascii_case("addViewComponents"))
                                && reader.has_type(call.object, "Tempest\\View\\ViewConfig")
                            {
                                for arg in call.argument_list.arguments.iter() {
                                    reader.components(
                                        arg.value(),
                                        &mut config.components,
                                        &calls,
                                        false,
                                    );
                                }
                            } else if method.eq_ignore_ascii_case("skipPaths")
                                && reader
                                    .has_type(call.object, "Tempest\\Discovery\\DiscoveryConfig")
                            {
                                for arg in call.argument_list.arguments.iter() {
                                    config.skipped.extend(reader.paths(arg.value()));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            },
        );
    }
    config.locations.sort_by(|a, b| a.path.cmp(&b.path));
    config.locations.dedup();
    config.skipped.sort();
    config.skipped.dedup();
    config
}

struct Reader<'a, 'b> {
    content: &'a str,
    loaders: &'a FileLoaders<'b>,
    file: &'a crate::types::FileContext,
    root: &'a Path,
    map: Option<std::sync::Arc<crate::symbol_map::SymbolMap>>,
}

impl Reader<'_, '_> {
    fn is_class(&self, expr: &Expression<'_>, expected: &str) -> bool {
        let Expression::Identifier(name) = expr else {
            return false;
        };
        self.file
            .resolve_name_at(bytes_to_str(name.value()), name.span().start.offset)
            .trim_start_matches('\\')
            .eq_ignore_ascii_case(expected)
    }

    fn has_type(&self, expr: &Expression<'_>, expected: &str) -> bool {
        super::view_data::expression_type(expr, self.content, self.loaders)
            .base_name()
            .is_some_and(|name| name.trim_start_matches('\\').eq_ignore_ascii_case(expected))
    }

    fn string(&self, expr: &Expression<'_>) -> Option<String> {
        let ty = super::view_data::expression_type(expr, self.content, self.loaders);
        Some(ty.as_literal()?.string_content()?.into_owned())
    }

    fn path(&self, expr: &Expression<'_>) -> Option<PathBuf> {
        self.string(expr)
            .filter(|path| !path.is_empty())
            .map(|path| self.absolute(&path))
    }

    fn absolute(&self, value: &str) -> PathBuf {
        let path = self.root.join(value);
        dunce::canonicalize(&path).unwrap_or(path)
    }

    fn paths(&self, expr: &Expression<'_>) -> Vec<PathBuf> {
        let ty = super::view_data::expression_type(expr, self.content, self.loaders);
        let mut paths = Vec::new();
        self.type_paths(&ty, &mut paths, true);
        paths
    }

    fn type_paths(&self, ty: &PhpType, paths: &mut Vec<PathBuf>, normalize: bool) {
        if let Some(value) = ty.as_literal().and_then(|literal| literal.string_content()) {
            if !value.is_empty() {
                paths.push(if normalize {
                    self.absolute(&value)
                } else {
                    PathBuf::from(value.as_ref())
                });
            }
        } else if let TypeKind::ArrayShape(entries) = ty.kind() {
            for entry in entries {
                if !entry.optional {
                    self.type_paths(&entry.value_type, paths, normalize);
                }
            }
        }
    }

    fn components(
        &self,
        expr: &Expression<'_>,
        components: &mut BTreeMap<String, Component>,
        calls: &ConfigCalls<'_, '_>,
        keyed: bool,
    ) {
        // The symbol index supplies lexical binding and reassignment rules.
        // Follow only a definite assignment when a configuration stores a
        // component object or a list before registering it.
        if let Expression::Variable(Variable::Direct(var)) = expr
            && let Some(map) = &self.map
            && let Some(definition) = map.find_var_definition(
                bytes_to_str(var.name).trim_start_matches('$'),
                var.span.start.offset,
                map.find_enclosing_scope(var.span.start.offset),
            )
            && definition.kind == crate::symbol_map::VarDefKind::Assignment
            && let Some(value) = calls.assignments.get(&definition.offset)
        {
            self.components(value, components, calls, keyed);
            return;
        }
        if let Expression::Array(array) = expr {
            for entry in array.elements.iter() {
                match entry {
                    ArrayElement::KeyValue(entry) if keyed => {
                        if let Some(name) = self.string(entry.key) {
                            let mut values = BTreeMap::new();
                            self.components(entry.value, &mut values, calls, false);
                            if let Some((_, value)) = values.into_iter().next() {
                                add_component(components, name, value);
                            }
                        }
                    }
                    ArrayElement::KeyValue(entry) => {
                        self.components(entry.value, components, calls, false)
                    }
                    ArrayElement::Value(entry) if !keyed => {
                        self.components(entry.value, components, calls, false)
                    }
                    ArrayElement::Variadic(entry) => {
                        self.components(entry.value, components, calls, keyed)
                    }
                    _ => {}
                }
            }
            return;
        }
        if keyed {
            return;
        }
        if let Expression::Instantiation(new) = expr
            && self.is_class(new.class, "Tempest\\View\\ViewComponent")
            && let Some(args) = &new.argument_list
            && let Some(name) = argument(args, "name", 0).and_then(|expr| self.string(expr))
            && let Some(path) = argument(args, "file", 2).and_then(|expr| self.path(expr))
        {
            let vendor = argument(args, "isVendorComponent", 3).is_some_and(|expr| {
                super::view_data::expression_type(expr, self.content, self.loaders).is_true()
            });
            add_component(components, name, Component { path, vendor });
            return;
        }
        let expr = match expr {
            Expression::Call(Call::StaticMethod(call))
                if self.is_class(call.class, "Tempest\\View\\ViewComponent")
                    && matches!(&call.method, ClassLikeMemberSelector::Identifier(name) if name.value.eq_ignore_ascii_case(b"fromPath")) =>
            {
                argument(&call.argument_list, "path", 0).unwrap_or(expr)
            }
            _ => expr,
        };
        for path in self.paths(expr) {
            if let Some(name) = super::discovery::component_name(&path) {
                let name = name.to_owned();
                let vendor = is_vendor_path(&path);
                add_component(components, name, Component { path, vendor });
            }
        }
    }
}

fn add_component(components: &mut BTreeMap<String, Component>, name: String, component: Component) {
    if components
        .get(&name)
        .is_none_or(|old| old.vendor && !component.vendor)
    {
        components.insert(name, component);
    }
}

fn is_vendor_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "vendor")
}

/// A named argument or its positional equivalent, without guessing spread contents.
pub(super) fn argument<'a>(
    args: &'a ArgumentList<'_>,
    name: &str,
    index: usize,
) -> Option<&'a Expression<'a>> {
    args.arguments
        .iter()
        .enumerate()
        .find_map(|(at, arg)| match arg {
            Argument::Named(arg) if arg.name.value == name.as_bytes() => Some(arg.value),
            Argument::Positional(arg) if at == index && arg.ellipsis.is_none() => Some(arg.value),
            _ => None,
        })
}

struct Calls;
#[derive(Default)]
struct ConfigCalls<'ast, 'arena> {
    calls: Vec<&'ast Expression<'arena>>,
    assignments: BTreeMap<u32, &'ast Expression<'arena>>,
}
impl<'ast, 'arena> Walker<'ast, 'arena, ConfigCalls<'ast, 'arena>> for Calls {
    fn walk_in_expression(
        &self,
        expr: &'ast Expression<'arena>,
        calls: &mut ConfigCalls<'ast, 'arena>,
    ) {
        if matches!(
            expr,
            Expression::Instantiation(_) | Expression::Call(Call::Method(_))
        ) {
            calls.calls.push(expr);
        }
        if let Expression::Assignment(assignment) = expr
            && let Expression::Variable(Variable::Direct(var)) = assignment.lhs
        {
            calls
                .assignments
                .insert(var.span.start.offset, assignment.rhs);
        }
    }
}
