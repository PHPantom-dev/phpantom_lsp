//! The service IDs and container parameters the installed bundles declare.
//!
//! A bundle declares its services where the application's configuration
//! never sees them: in the files under its own `Resources/config/` (or the
//! `config/` beside the `src/` of a bundle laid out the modern way), and in
//! the extension under its `DependencyInjection/` that loads them and
//! registers more through the container builder.  None of them are in the
//! workspace index, so they are read from disk once, the way Twig's vendor
//! extensions are (see `twig::callables`), and kept until the dependencies
//! change.
//!
//! A configurator call in one of these files is taken as what the file
//! names it: the files are a bundle's container configuration, which the
//! type engine is never asked about.  `set()` and `alias()` have to say
//! which configurator they are on, by the `services()` or `parameters()`
//! the chain starts from (or a `$services` or `$parameters` variable),
//! which is how every bundle writes them.
//!
//! An ID that starts with a `.` is a hidden service, which no application
//! names, so it is left out.  The parent a bundle's configuration gives a
//! service is no guide to its class: the extension re-parents services from
//! the configuration the application gives it (`cache.app` is whichever
//! adapter `framework.cache.app` names), so only a class or an alias the
//! bundle declares types the service.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_syntax::cst::*;

use crate::Backend;
use crate::symbol_map::{ContainerValue, ReceiverClass, StringKeyKind, SymbolKind, SymbolMap};

/// What the installed bundles declare, file by file.
#[derive(Default)]
pub(crate) struct BundleDeclarations {
    files: Vec<(String, Arc<SymbolMap>)>,
}

/// Text one of a PHP file's declaring calls has to contain, so the files
/// that make none are not parsed.
const DECLARING_CALLS: [&[u8]; 7] = [
    b"->set(",
    b"->alias(",
    b"register(",
    b"autowire(",
    b"setAlias(",
    b"setDefinition(",
    b"setParameter(",
];

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.contains(&ext))
}

/// Every file under `dir` with one of `extensions`, leaving out tests.
fn files_under(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
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
                if !matches!(entry.file_name().to_str(), Some("Tests" | "tests")) {
                    pending.push(path);
                }
            } else if has_extension(&path, extensions) {
                out.push(path);
            }
        }
    }
}

/// The files a bundle whose class is at `class_file` declares services in:
/// the class itself (an `AbstractBundle` loads its own extension), its
/// extension, and its configuration.
fn bundle_files(class_file: &Path) -> Vec<PathBuf> {
    let mut files = vec![class_file.to_path_buf()];
    let Some(dir) = class_file.parent() else {
        return files;
    };
    let config = ["php", "yaml", "yml", "xml"];
    files_under(&dir.join("Resources/config"), &config, &mut files);
    files_under(&dir.join("DependencyInjection"), &["php"], &mut files);
    if dir.file_name().is_some_and(|name| name == "src")
        && let Some(root) = dir.parent()
    {
        files_under(&root.join("config"), &config, &mut files);
    }
    files
}

/// The kind of name a configurator's `set()` or `alias()` declares, by the
/// configurator its chain starts from.
fn chain_kind(mut object: &Expression<'_>) -> Option<StringKeyKind> {
    loop {
        match object {
            Expression::Call(Call::Method(call)) => {
                if let ClassLikeMemberSelector::Identifier(name) = &call.method {
                    if name.value.eq_ignore_ascii_case(b"services") {
                        return Some(StringKeyKind::ServiceId);
                    }
                    if name.value.eq_ignore_ascii_case(b"parameters") {
                        return Some(StringKeyKind::ContainerParameter);
                    }
                }
                object = call.object;
            }
            Expression::Variable(Variable::Direct(variable)) => {
                return match variable.name {
                    b"$services" => Some(StringKeyKind::ServiceId),
                    b"$parameters" => Some(StringKeyKind::ContainerParameter),
                    _ => None,
                };
            }
            _ => return None,
        }
    }
}

/// Finds the configurator `set()` and `alias()` calls of a file, keyed by
/// the offset of the name they declare, with the kind of name it is.
struct ConfiguratorCalls;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, HashMap<u32, StringKeyKind>>
    for ConfiguratorCalls
{
    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        found: &mut HashMap<u32, StringKeyKind>,
    ) {
        let ClassLikeMemberSelector::Identifier(name) = &node.method else {
            return;
        };
        if !name.value.eq_ignore_ascii_case(b"set") && !name.value.eq_ignore_ascii_case(b"alias") {
            return;
        }
        let Some(Argument::Positional(first)) = node.argument_list.arguments.first() else {
            return;
        };
        let Expression::Literal(literal::Literal::String(string)) = first.value else {
            return;
        };
        if let Some(kind) = chain_kind(node.object) {
            found.insert(string.span.start.offset + 1, kind);
        }
    }
}

/// The kind of name a declaring call on `receiver` makes, in a bundle's
/// file, where the call is taken as what the file names it.
fn declared_kind(
    receiver: ReceiverClass,
    start: u32,
    configurators: &HashMap<u32, StringKeyKind>,
) -> Option<StringKeyKind> {
    match receiver {
        ReceiverClass::ContainerBuilder => Some(StringKeyKind::ServiceId),
        ReceiverClass::ParameterHolder => Some(StringKeyKind::ContainerParameter),
        ReceiverClass::ConfiguratorSet | ReceiverClass::ConfiguratorAlias => {
            configurators.get(&start).copied()
        }
        _ => None,
    }
}

/// What the PHP file at `uri` declares.
fn read_php_file(uri: &str, content: &str) -> SymbolMap {
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(uri.as_bytes());
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let resolver = mago_names::resolver::NameResolver::new(&arena);
    let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
    let map = crate::symbol_map::extraction::extract_symbol_map_for_file(
        program,
        content,
        &names,
        crate::symbol_map::extraction::Frameworks {
            symfony: true,
            service_container: true,
            drupal: false,
            twig: false,
            doctrine: false,
            yii_params: false,
        },
        None,
        crate::drupal::hooks::DrupalFile::Other,
    );
    let mut configurators = HashMap::new();
    for statement in program.statements.iter() {
        mago_syntax::walker::Walker::walk_statement(
            &ConfiguratorCalls,
            statement,
            &mut configurators,
        );
    }
    let mut spans: Vec<_> = map
        .typed_receiver_sites
        .iter()
        .filter(|site| site.is_write)
        .filter_map(|site| {
            declared_kind(site.receiver, site.start, &configurators).map(|kind| site.to_span(kind))
        })
        .collect();
    spans.sort_by_key(|span| span.start);
    SymbolMap {
        spans,
        container_values: map.container_values,
        ..SymbolMap::default()
    }
}

/// What the configuration file at `path` declares, or `None` when it
/// declares no service or parameter.
fn read_bundle_file(path: &Path) -> Option<(String, SymbolMap)> {
    let content = std::fs::read_to_string(path).ok()?;
    let uri = crate::util::path_to_uri(path);
    let mut map = if has_extension(path, &["php"]) {
        let finder = |needle: &[u8]| memchr::memmem::find(content.as_bytes(), needle).is_some();
        if !DECLARING_CALLS.iter().any(|needle| finder(needle)) {
            return None;
        }
        read_php_file(&uri, &content)
    } else {
        let (spans, container_values) = super::resources::container_declarations(&uri, &content);
        SymbolMap {
            spans,
            container_values,
            ..SymbolMap::default()
        }
    };
    map.spans.retain(|span| {
        matches!(
            &span.kind,
            SymbolKind::StringKey {
                kind: StringKeyKind::ServiceId | StringKeyKind::ContainerParameter,
                key,
                is_write: true,
                ..
            } if !key.starts_with('.')
        )
    });
    if map.spans.is_empty() {
        return None;
    }
    let starts: HashSet<u32> = map.spans.iter().map(|span| span.start).collect();
    map.container_values.retain(|site| {
        starts.contains(&site.start) && !matches!(site.value, ContainerValue::Parent(_))
    });
    map.container_values.sort_by_key(|site| site.start);
    map.source_len = u32::try_from(content.len()).unwrap_or(u32::MAX);
    Some((uri, map))
}

/// Whether the class `fqn`, declared at `uri`, is named like a bundle and
/// lives in a file of its own name.
fn is_bundle_class(fqn: &str, uri: &str) -> bool {
    let short = fqn.rsplit('\\').next().unwrap_or(fqn);
    short.len() > "Bundle".len()
        && short.ends_with("Bundle")
        && uri
            .rsplit('/')
            .next()
            .and_then(|file| file.strip_suffix(".php"))
            == Some(short)
        && !uri.contains("/Tests/")
        && !uri.contains("/tests/")
}

impl Backend {
    /// What the installed bundles declare.
    pub(crate) fn symfony_bundle_declarations(&self) -> Arc<BundleDeclarations> {
        let mut known = self.symfony.bundle_declarations.lock();
        if let Some(known) = known.as_ref() {
            return Arc::clone(known);
        }
        let prefixes = self.workspace.vendor_uri_prefixes.lock().clone();
        let mut class_files: Vec<PathBuf> = self
            .symbols
            .fqn_uri_index
            .read()
            .iter()
            .filter(|(fqn, uri)| {
                is_bundle_class(fqn, uri)
                    && prefixes
                        .iter()
                        .any(|prefix| uri.starts_with(prefix.as_str()))
            })
            .filter_map(|(_, uri)| crate::util::uri_to_path(uri))
            .collect();
        class_files.sort_unstable();
        class_files.dedup();

        let mut paths: Vec<PathBuf> = class_files
            .iter()
            .flat_map(|class_file| bundle_files(class_file))
            .collect();
        paths.sort_unstable();
        paths.dedup();
        let files = paths
            .iter()
            .filter_map(|path| read_bundle_file(path))
            .map(|(uri, map)| (uri, Arc::new(map)))
            .collect();

        let found = Arc::new(BundleDeclarations { files });
        *known = Some(Arc::clone(&found));
        found
    }

    /// Forget what the installed bundles declare, once they changed.
    pub(crate) fn forget_symfony_bundle_declarations(&self) {
        *self.symfony.bundle_declarations.lock() = None;
    }

    /// Visit every declaration of a `kind` name an installed bundle makes:
    /// the file and its map, the byte range of the name, and the name.
    pub(crate) fn visit_bundle_declarations(
        &self,
        kind: StringKeyKind,
        mut visit: impl FnMut(&str, &SymbolMap, u32, u32, &str),
    ) {
        if !matches!(
            kind,
            StringKeyKind::ServiceId | StringKeyKind::ContainerParameter
        ) || !self.is_symfony_project()
        {
            return;
        }
        let bundles = self.symfony_bundle_declarations();
        for (uri, map) in &bundles.files {
            for span in &map.spans {
                if let SymbolKind::StringKey {
                    kind: span_kind,
                    key,
                    ..
                } = &span.kind
                    && *span_kind == kind
                {
                    visit(uri, map, span.start, span.end, key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(path: &str, content: &str) -> Vec<(String, String)> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        let Some((_, map)) = read_bundle_file(&path) else {
            return Vec::new();
        };
        map.spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey { kind, key, .. } => Some((format!("{kind:?}"), key.clone())),
                _ => None,
            })
            .collect()
    }

    fn pair(kind: &str, key: &str) -> (String, String) {
        (kind.to_string(), key.to_string())
    }

    #[test]
    fn a_configurator_file_declares_by_the_configurator_its_chain_starts_from() {
        let php = "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\n\nreturn static function (ContainerConfigurator $container) {\n    $container->parameters()->set('acme.prefix', 'acme');\n    $container->services()\n        ->set('acme.cache')\n            ->parent('acme.cache.base')\n        ->set('.acme.hidden')\n        ->alias('acme.cache_alias', 'acme.cache');\n    $services = $container->services();\n    $services->set('acme.other');\n    $container->import('routing.php')->set('ambiguous');\n};\n";
        assert_eq!(
            declared("Resources/config/services.php", php),
            vec![
                pair("ContainerParameter", "acme.prefix"),
                pair("ServiceId", "acme.cache"),
                pair("ServiceId", "acme.cache_alias"),
                pair("ServiceId", "acme.other"),
            ]
        );
    }

    #[test]
    fn an_extension_declares_through_the_container_builder() {
        let php = "<?php\nnamespace Acme\\DependencyInjection;\n\nuse Symfony\\Component\\DependencyInjection\\ContainerBuilder;\nuse Symfony\\Component\\DependencyInjection\\Definition;\n\nfinal class AcmeExtension\n{\n    public function load(array $configs, ContainerBuilder $container): void\n    {\n        $container->setParameter('acme.enabled', true);\n        $container->register('acme.mailer', Mailer::class);\n        $container->setDefinition('acme.client', new Definition(Client::class));\n        $container->setAlias('acme.default_mailer', 'acme.mailer');\n        $container->register($dynamic, Other::class);\n    }\n}\n";
        assert_eq!(
            declared("DependencyInjection/AcmeExtension.php", php),
            vec![
                pair("ContainerParameter", "acme.enabled"),
                pair("ServiceId", "acme.mailer"),
                pair("ServiceId", "acme.client"),
                pair("ServiceId", "acme.default_mailer"),
            ]
        );
    }

    #[test]
    fn yaml_and_xml_declare_as_the_application_does() {
        assert_eq!(
            declared(
                "Resources/config/services.yaml",
                "parameters:\n    acme.locale: en\nservices:\n    acme.translator:\n        class: Acme\\Translator\n        arguments: ['@acme.other', '%acme.locale%']\n",
            ),
            vec![
                pair("ContainerParameter", "acme.locale"),
                pair("ServiceId", "acme.translator"),
            ]
        );
    }

    #[test]
    fn a_bundle_class_is_named_like_its_file() {
        assert!(is_bundle_class(
            "Symfony\\Bundle\\TwigBundle\\TwigBundle",
            "file:///p/vendor/symfony/twig-bundle/TwigBundle.php"
        ));
        assert!(!is_bundle_class(
            "Symfony\\Component\\HttpKernel\\Bundle\\Bundle",
            "file:///p/vendor/symfony/http-kernel/Bundle/Bundle.php"
        ));
        assert!(!is_bundle_class(
            "Acme\\Tests\\FixtureBundle",
            "file:///p/vendor/acme/bundle/Tests/FixtureBundle.php"
        ));
    }
}
