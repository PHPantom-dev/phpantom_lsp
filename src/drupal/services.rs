//! The service IDs a Drupal site's container may hold that no services file
//! lists.
//!
//! The container is built from every enabled extension's `*.services.yml`,
//! but PHP adds to it too: a module's service provider registers services,
//! a compiler pass defines and aliases them, and the kernel sets its own.
//! An unknown service ID is only worth a warning when none of these could
//! have given it, so what PHP may define is read the way PHPlumb (MIT) reads
//! it, rules its authors ran against core and contributed modules for
//! false alarms:
//!
//! - Every string literal and class name of a file that mentions the
//!   container builder, a service provider or modifier, or a compiler pass
//!   may be a service ID, and an ID such a file builds from parts is kept as
//!   the shape of the names it can build (`search.*`).
//! - The first argument of every `set()`, `register()`, `autowire()`,
//!   `setAlias()`, or `setDefinition()` call, wherever it is written, may
//!   be one too.
//! - The services files of test modules declare IDs, though nothing else
//!   reads them (see [`crate::drupal::is_declaration_file`]).
//!
//! The same files are where plugin definitions are added in code: every
//! string literal of a file that calls `setDefinition()` may be a plugin
//! ID (see [`crate::drupal::plugins`]).
//!
//! The files are found by the web-root walk, which reads every PHP file
//! anyway, and read once a lookup has to be judged.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock};

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;
use memchr::memmem::Finder;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// The services the kernel puts in the container itself.
const KERNEL_SERVICES: [&str; 3] = ["class_loader", "kernel", "service_container"];

/// The containers whose `get()` takes a service of the site's: Symfony's,
/// and Drupal's, which extends it.  A plain PSR container may be any other
/// container.
pub(crate) const SERVICE_CONTAINERS: [&str; 2] = [
    "Symfony\\Component\\DependencyInjection\\ContainerInterface",
    "Drupal\\Component\\DependencyInjection\\ContainerInterface",
];

/// The classes whose mention makes a file one that may define services:
/// the container builder, a service provider or modifier, a compiler pass,
/// and the subtypes core gives the first three.
const SERVICE_DEFINERS: [&str; 6] = [
    "Symfony\\Component\\DependencyInjection\\ContainerBuilder",
    "Symfony\\Component\\DependencyInjection\\Compiler\\CompilerPassInterface",
    "Drupal\\Core\\DependencyInjection\\ContainerBuilder",
    "Drupal\\Core\\DependencyInjection\\ServiceProviderInterface",
    "Drupal\\Core\\DependencyInjection\\ServiceModifierInterface",
    "Drupal\\Core\\DependencyInjection\\ServiceProviderBase",
];

/// The methods whose first argument may name a service they define.
const SERVICE_SETTERS: [&[u8]; 5] = [
    b"set",
    b"register",
    b"autowire",
    b"setAlias",
    b"setDefinition",
];

/// The text a file that may define services has to hold: the short names
/// of [`SERVICE_DEFINERS`], or a call of one of [`SERVICE_SETTERS`].
static CANDIDATE_TEXT: LazyLock<[Finder<'static>; 6]> = LazyLock::new(|| {
    [
        Finder::new(b"ContainerBuilder"),
        Finder::new(b"CompilerPassInterface"),
        Finder::new(b"ServiceProvider"),
        Finder::new(b"ServiceModifierInterface"),
        Finder::new(b"->register("),
        Finder::new(b"->autowire("),
    ]
});

/// `->set` starts `->setAlias(` and `->setDefinition(` too, and every
/// other setter besides, which the bytes after it tell apart.
static SET_CALL: LazyLock<Finder<'static>> = LazyLock::new(|| Finder::new(b"->set"));

/// Whether a PHP file holding `content` may define services: one that
/// mentions a class of [`SERVICE_DEFINERS`] or calls a method of
/// [`SERVICE_SETTERS`], by its text alone.  Cheap enough to ask of every
/// file the web-root walk reads.
pub(crate) fn may_define_services(content: &[u8]) -> bool {
    CANDIDATE_TEXT
        .iter()
        .any(|finder| finder.find(content).is_some())
        || SET_CALL.find_iter(content).any(|at| {
            let rest = &content[at + b"->set".len()..];
            [b"(".as_slice(), b"Alias(", b"Definition("]
                .iter()
                .any(|tail| rest.starts_with(tail))
        })
}

/// Whether `path` is a test module's services file, which declares service
/// IDs but nothing else reads: under a `tests` folder of the web root.
pub(crate) fn is_test_services_file(path: &Path, web_root: &Path) -> bool {
    path.to_str()
        .and_then(super::declaration_file_kind)
        .is_some_and(|kind| kind == super::DeclarationFile::Services)
        && super::is_test_fixture(path, web_root)
        && !super::is_unloaded(path, web_root)
}

/// What one file may define: service IDs, the shapes of the IDs it builds
/// from parts, `*` for each part that is not written out, and the plugin
/// IDs it may add definitions for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FileServiceIds {
    pub(crate) ids: Vec<String>,
    pub(crate) patterns: Vec<String>,
    pub(crate) plugin_ids: Vec<String>,
}

impl FileServiceIds {
    fn is_empty(&self) -> bool {
        self.ids.is_empty() && self.patterns.is_empty() && self.plugin_ids.is_empty()
    }
}

/// The service IDs PHP code of the PHP file holding `content` may define;
/// see the module documentation.
pub(crate) fn defined_service_ids(content: &str) -> FileServiceIds {
    let arena = LocalArena::new();
    let program = parse_file_content(&arena, FileId::new(b"input.php"), content.as_bytes());
    let names = OwnedResolvedNames::from_resolved(&NameResolver::new(&arena).resolve(program));
    let definer = names.iter().any(|(_, name, _)| {
        let name = name.trim_start_matches('\\');
        SERVICE_DEFINERS
            .iter()
            .any(|definer| definer.eq_ignore_ascii_case(name))
    });
    let mut found = Collected::default();
    let walker = Collector {
        content,
        definer,
        plugin_definer: content.contains("setDefinition("),
    };
    for statement in program.statements.iter() {
        mago_syntax::walker::Walker::walk_statement(&walker, statement, &mut found);
    }
    let mut ids = found.ids;
    if definer {
        ids.extend(
            names
                .iter()
                .map(|(_, name, _)| name.trim_start_matches('\\').to_string()),
        );
    }
    ids.sort_unstable();
    ids.dedup();
    let mut patterns = found.patterns;
    patterns.sort_unstable();
    patterns.dedup();
    let mut plugin_ids = found.plugin_ids;
    plugin_ids.sort_unstable();
    plugin_ids.dedup();
    FileServiceIds {
        ids,
        patterns,
        plugin_ids,
    }
}

#[derive(Default)]
struct Collected {
    ids: Vec<String>,
    patterns: Vec<String>,
    plugin_ids: Vec<String>,
    /// How many concatenations the walk is inside: only the outermost one
    /// spells a whole name.
    concatenations: u32,
}

impl Collected {
    /// Keep `id`, when it can be a service ID at all: no ID holds
    /// whitespace, which leaves out the file's messages and queries.
    fn push_id(&mut self, id: &str) {
        if !id.is_empty() && !id.contains(char::is_whitespace) {
            self.ids.push(id.to_string());
        }
    }

    /// Keep the shape of a name built from parts, when it says enough to
    /// stand for names: one of its written parts holds a `.` and a letter
    /// or digit, so `*.settings` is kept and `*.*` is not.  A shape with no
    /// `*` is every part written out, one name.
    fn push_shape(&mut self, expression: &Expression<'_>) {
        let shape = crate::symbol_map::extraction::name_shape(expression);
        if !shape.contains('*') {
            self.push_id(&shape);
        } else if shape
            .split('*')
            .any(|part| part.contains('.') && part.chars().any(char::is_alphanumeric))
        {
            self.patterns.push(shape);
        }
    }
}

struct Collector<'c> {
    content: &'c str,
    /// Whether every literal and name of the file may be a service ID.
    definer: bool,
    /// Whether every literal of the file may be a plugin ID.
    plugin_definer: bool,
}

impl Collector<'_> {
    fn setter_call(
        &self,
        method: &ClassLikeMemberSelector<'_>,
        arguments: &ArgumentList<'_>,
        found: &mut Collected,
    ) {
        let ClassLikeMemberSelector::Identifier(name) = method else {
            return;
        };
        if !SERVICE_SETTERS
            .iter()
            .any(|setter| name.value.eq_ignore_ascii_case(setter))
        {
            return;
        }
        if let Some(Argument::Positional(first)) = arguments.arguments.first()
            && let Some((id, _, _)) = resolved_string_literal(first.value, self.content)
        {
            found.push_id(id);
        }
    }
}

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Collected> for Collector<'_> {
    fn walk_in_method_call(&self, call: &'ast MethodCall<'arena>, found: &mut Collected) {
        self.setter_call(&call.method, &call.argument_list, found);
    }

    fn walk_in_null_safe_method_call(
        &self,
        call: &'ast NullSafeMethodCall<'arena>,
        found: &mut Collected,
    ) {
        self.setter_call(&call.method, &call.argument_list, found);
    }

    fn walk_in_literal_string(&self, string: &'ast LiteralString<'arena>, found: &mut Collected) {
        let Some(value) = string.value else {
            return;
        };
        let value = bytes_to_str(value);
        if self.definer {
            found.push_id(value);
        }
        if self.plugin_definer && !value.is_empty() {
            found.plugin_ids.push(value.to_string());
        }
    }

    fn walk_in_expression(&self, expression: &'ast Expression<'arena>, found: &mut Collected) {
        if !self.definer {
            return;
        }
        match expression {
            Expression::Binary(binary) if binary.operator.is_concatenation() => {
                if found.concatenations == 0 {
                    found.push_shape(expression);
                }
                found.concatenations += 1;
            }
            Expression::CompositeString(CompositeString::Interpolated(_))
                if found.concatenations == 0 =>
            {
                found.push_shape(expression);
            }
            _ => {}
        }
    }

    fn walk_out_expression(&self, expression: &'ast Expression<'arena>, found: &mut Collected) {
        if self.definer
            && let Expression::Binary(binary) = expression
            && binary.operator.is_concatenation()
        {
            found.concatenations -= 1;
        }
    }
}

/// What every file found to define services may define, once read.
#[derive(Debug, Clone, Default)]
pub(crate) struct PhpServiceIds {
    by_uri: HashMap<String, FileServiceIds>,
    /// Every file's IDs, sorted.
    ids: Vec<String>,
    /// Every file's shapes, sorted.
    patterns: Vec<String>,
    /// Every file's plugin IDs, sorted.
    plugin_ids: Vec<String>,
}

impl PhpServiceIds {
    fn from_files(by_uri: HashMap<String, FileServiceIds>) -> Self {
        let mut all = Self {
            by_uri,
            ..Self::default()
        };
        all.gather();
        all
    }

    fn gather(&mut self) {
        let mut ids: Vec<String> = Vec::new();
        let mut patterns: Vec<String> = Vec::new();
        let mut plugin_ids: Vec<String> = Vec::new();
        for file in self.by_uri.values() {
            ids.extend(file.ids.iter().cloned());
            patterns.extend(file.patterns.iter().cloned());
            plugin_ids.extend(file.plugin_ids.iter().cloned());
        }
        for list in [&mut ids, &mut patterns, &mut plugin_ids] {
            list.sort_unstable();
            list.dedup();
        }
        self.ids = ids;
        self.patterns = patterns;
        self.plugin_ids = plugin_ids;
    }

    /// Put what the file at `uri` now defines in place of what it did,
    /// reporting whether anything changed.
    fn replace(&mut self, uri: &str, file: FileServiceIds) -> bool {
        let changed = if file.is_empty() {
            self.by_uri.remove(uri).is_some()
        } else if self.by_uri.get(uri) == Some(&file) {
            false
        } else {
            self.by_uri.insert(uri.to_string(), file);
            true
        };
        if changed {
            self.gather();
        }
        changed
    }

    /// Whether `id` is one the kernel sets, or PHP code may define.
    pub(crate) fn declares(&self, id: &str) -> bool {
        KERNEL_SERVICES.contains(&id)
            || self
                .ids
                .binary_search_by(|declared| declared.as_str().cmp(id))
                .is_ok()
            || self
                .patterns
                .iter()
                .any(|pattern| super::matches_route_pattern(pattern, id))
    }
}

impl PhpServiceIds {
    /// Whether PHP code may add a definition for the plugin `id`, or for
    /// the base of a derivative `id`.
    pub(crate) fn declares_plugin(&self, id: &str) -> bool {
        let listed = |id: &str| {
            self.plugin_ids
                .binary_search_by(|declared| declared.as_str().cmp(id))
                .is_ok()
        };
        listed(id) || crate::drupal::plugins::derivative_bases(id).any(listed)
    }
}

/// The service IDs a Drupal site's services files declare, sorted, and what
/// its PHP code may declare (see [`Backend::drupal_service_ids`]).
pub(crate) struct DrupalServiceIds {
    declared: Vec<String>,
    php: Arc<PhpServiceIds>,
}

impl DrupalServiceIds {
    /// Whether the site may declare the service `id`, or `None` when its
    /// services files were not read.
    pub(crate) fn may_declare(&self, id: &str) -> Option<bool> {
        if self.declared.is_empty() {
            return None;
        }
        Some(
            self.declared
                .binary_search_by(|name| name.as_str().cmp(id))
                .is_ok()
                || self.php.declares(id),
        )
    }
}

/// What a file the web-root walk found may define: a test module's
/// services file declares the IDs it lists, a PHP file what
/// [`defined_service_ids`] reads.
fn read_service_ids(uri: &str, content: &str) -> FileServiceIds {
    if !uri.ends_with(".yml") {
        return defined_service_ids(content);
    }
    let ids = crate::symfony::resources::container_declarations(uri, content)
        .spans
        .into_iter()
        .filter_map(|span| match span.kind {
            crate::symbol_map::SymbolKind::StringKey {
                kind: crate::symbol_map::StringKeyKind::ServiceId,
                key,
                is_write: true,
                ..
            } => Some(key),
            _ => None,
        })
        .collect();
    FileServiceIds {
        ids,
        ..FileServiceIds::default()
    }
}

impl Backend {
    /// Remember the files the Drupal walk found to define services: the PHP
    /// files [`may_define_services`] picks, and test modules' services
    /// files.
    pub(crate) fn note_drupal_service_definition_files(&self, files: Vec<std::path::PathBuf>) {
        let mut known = self.drupal.service_definition_files.write();
        known.extend(files.iter().map(|path| crate::util::path_to_uri(path)));
    }

    /// The service IDs the Drupal site declares.  Core's and contributed
    /// modules' services files are only read once the workspace index has
    /// walked the folders `.gitignore` leaves out.
    pub(crate) fn drupal_service_ids(&self) -> DrupalServiceIds {
        self.ensure_workspace_index_ready_for_request();
        DrupalServiceIds {
            declared: self.symfony_declared_names(crate::symbol_map::StringKeyKind::ServiceId),
            php: self.drupal_php_service_ids(),
        }
    }

    /// Whether the name `name` a callback gives, which names no function,
    /// names a class or a service, which Drupal calls as an invokable, or
    /// may name one the site's unread services files declare.  `services`
    /// holds the site's service IDs once a name needs them, so a diagnostic
    /// pass reads them at most once.
    pub(crate) fn drupal_callback_names_invokable(
        &self,
        name: &str,
        services: &std::cell::OnceCell<DrupalServiceIds>,
    ) -> bool {
        let name = name.trim_start_matches('\\');
        self.find_or_load_class(name).is_some()
            || services
                .get_or_init(|| self.drupal_service_ids())
                .may_declare(name)
                != Some(false)
    }

    /// What the files that may define services define, read on first use.
    pub(crate) fn drupal_php_service_ids(&self) -> Arc<PhpServiceIds> {
        if let Some(ids) = self.drupal.php_service_ids.read().as_ref() {
            return Arc::clone(ids);
        }
        let uris: Vec<String> = self
            .drupal
            .service_definition_files
            .read()
            .iter()
            .cloned()
            .collect();
        let read = crate::parallel::map_in_blocks("drupal_service_ids", &uris, 32, |uri| {
            let content = match self.open_files.read().get(uri) {
                Some(open) => Arc::clone(open),
                None => Arc::new(crate::util::read_php_source(&crate::util::uri_to_path(
                    uri,
                )?)?),
            };
            let file = read_service_ids(uri, &content);
            (!file.is_empty()).then(|| (uri.clone(), file))
        });
        let ids = Arc::new(PhpServiceIds::from_files(read.into_iter().collect()));
        *self.drupal.php_service_ids.write() = Some(Arc::clone(&ids));
        ids
    }

    /// Follow an edit to a PHP file of a web root, outside test modules, in
    /// what [`Self::drupal_php_service_ids`] read.  Cheap for every file
    /// that neither mentions what defines services nor did before.
    pub(crate) fn refresh_drupal_service_definitions(&self, uri: &str, content: &str) {
        if !self.is_drupal_project() || uri.ends_with(".yml") {
            return;
        }
        let candidate = may_define_services(content.as_bytes());
        let known = self.drupal.service_definition_files.read().contains(uri);
        if !candidate && !known {
            return;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return;
        };
        if !self.is_in_drupal_web_root(uri) || self.is_drupal_test_path(&path) {
            return;
        }
        if candidate != known {
            let mut files = self.drupal.service_definition_files.write();
            if candidate {
                files.insert(uri.to_string());
            } else {
                files.remove(uri);
            }
        }
        if self.drupal.php_service_ids.read().is_none() {
            return;
        }
        let file = if candidate {
            defined_service_ids(content)
        } else {
            FileServiceIds::default()
        };
        if let Some(ids) = self.drupal.php_service_ids.write().as_mut() {
            Arc::make_mut(ids).replace(uri, file);
        }
    }

    /// Forget what a deleted file defined.
    pub(crate) fn forget_drupal_service_definitions(&self, uri: &str) {
        if !self.drupal.service_definition_files.read().contains(uri) {
            return;
        }
        self.drupal.service_definition_files.write().remove(uri);
        if let Some(ids) = self.drupal.php_service_ids.write().as_mut() {
            Arc::make_mut(ids).replace(uri, FileServiceIds::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_provider_may_define_its_literals_names_and_shapes() {
        let php = r#"<?php
namespace Drupal\acme;

use Drupal\Core\DependencyInjection\ContainerBuilder;
use Drupal\Core\DependencyInjection\ServiceProviderBase;

class AcmeServiceProvider extends ServiceProviderBase {
    public function register(ContainerBuilder $container) {
        $container->register('acme.registered', Registered::class);
        $id = 'acme.listed';
        foreach (['a', 'b'] as $kind) {
            $container->setDefinition('acme.storage.' . $kind, new Definition());
            $container->setAlias("acme.alias.$kind", 'acme.listed');
        }
        $message = 'Not an id at all';
    }
}
"#;
        let found = defined_service_ids(php);
        for id in [
            "acme.registered",
            "acme.listed",
            "Drupal\\acme\\Registered",
            "Drupal\\Core\\DependencyInjection\\ServiceProviderBase",
        ] {
            assert!(found.ids.iter().any(|found| found == id), "{id}: {found:?}");
        }
        assert!(!found.ids.iter().any(|id| id.contains(' ')), "{found:?}");
        assert_eq!(
            found.patterns,
            vec!["acme.alias.*".to_string(), "acme.storage.*".to_string()]
        );
    }

    #[test]
    fn elsewhere_only_a_setter_defines_a_service() {
        let php = r#"<?php
function acme_setup($container, $config) {
    $container->set('acme.runtime', new \stdClass());
    $config?->setAlias('acme.alias', 'acme.runtime');
    $x = 'acme.not_defined';
    $container->set('acme.' . $x, 1);
}
"#;
        let found = defined_service_ids(php);
        assert_eq!(
            found.ids,
            vec!["acme.alias".to_string(), "acme.runtime".to_string()]
        );
        assert!(found.patterns.is_empty());
    }

    #[test]
    fn a_shape_says_enough_only_with_a_dotted_written_part() {
        let php = "<?php\nuse Symfony\\Component\\DependencyInjection\\ContainerBuilder;\n$a = $x . '.' . $y;\n$b = $x . '.settings';\n";
        assert_eq!(
            defined_service_ids(php).patterns,
            vec!["*.settings".to_string()]
        );
    }

    #[test]
    fn the_text_picks_the_files_worth_reading() {
        assert!(may_define_services(b"<?php $c->set('a', 1);"));
        assert!(may_define_services(b"<?php use X\\ContainerBuilder;"));
        assert!(may_define_services(b"<?php $c->setAlias('a', 'b');"));
        assert!(!may_define_services(b"<?php $c->get('a');"));
        assert!(!may_define_services(b"<?php $form->setTitle('a');"));
    }

    #[test]
    fn kernel_services_ids_and_shapes_are_declared() {
        let mut by_uri = HashMap::new();
        by_uri.insert(
            "file:///a.php".to_string(),
            FileServiceIds {
                ids: vec!["acme.one".to_string()],
                patterns: vec!["acme.storage.*".to_string()],
                plugin_ids: vec!["acme_block".to_string()],
            },
        );
        let ids = PhpServiceIds::from_files(by_uri);
        for id in [
            "kernel",
            "service_container",
            "acme.one",
            "acme.storage.node",
        ] {
            assert!(ids.declares(id), "{id}");
        }
        assert!(!ids.declares("acme.two"));
        assert!(ids.declares_plugin("acme_block"));
        assert!(ids.declares_plugin("acme_block:main"));
        assert!(!ids.declares_plugin("acme_other"));
    }

    #[test]
    fn a_file_that_sets_a_definition_may_declare_its_literals_as_plugins() {
        let php = r#"<?php
function acme_block_alter(array &$definitions, $manager) {
    $manager->setDefinition('acme_runtime', ['label' => 'Runtime']);
}
"#;
        let found = defined_service_ids(php);
        assert_eq!(
            found.plugin_ids,
            vec![
                "Runtime".to_string(),
                "acme_runtime".to_string(),
                "label".to_string()
            ]
        );
        assert!(
            defined_service_ids("<?php $a = 'acme';")
                .plugin_ids
                .is_empty()
        );
    }

    #[test]
    fn test_modules_services_files_are_found() {
        let web = Path::new("/site/web");
        assert!(is_test_services_file(
            Path::new(
                "/site/web/core/modules/system/tests/modules/acme_test/acme_test.services.yml"
            ),
            web
        ));
        assert!(!is_test_services_file(
            Path::new("/site/web/modules/custom/acme/acme.services.yml"),
            web
        ));
        assert!(!is_test_services_file(
            Path::new(
                "/site/web/core/modules/system/tests/modules/acme_test/acme_test.routing.yml"
            ),
            web
        ));
    }
}
