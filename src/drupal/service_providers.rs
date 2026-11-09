//! The classes a module's service provider builds another module's services
//! from.
//!
//! Drupal looks for a `Drupal\<module>\<Module>ServiceProvider` class in
//! every enabled module, and its `alter()` may change any service the
//! container holds by then: `$container->getDefinition('language_manager')
//! ->setClass(ConfigurableLanguageManager::class)` builds the language
//! manager from the module's class while the module is enabled.  Core's own
//! `CoreServiceProvider` is always loaded.  Which modules are enabled lives
//! in the database, so a service a provider alters is built from either
//! class: the one its YAML names, or the provider's.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;

use crate::Backend;
use crate::atom::{Atom, atom, bytes_to_str};
use crate::names::OwnedResolvedNames;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// Where core keeps the provider it registers on every site.
const CORE_SERVICE_PROVIDER: &str = "core/lib/Drupal/Core/CoreServiceProvider.php";

/// The name Drupal gives a module's service provider class: the module's
/// machine name camelized the way Symfony's `Container::camelize()` does,
/// followed by `ServiceProvider`.
fn provider_file_name(module: &str) -> String {
    let mut name = String::with_capacity(module.len() + "ServiceProvider.php".len());
    for word in module.split('_') {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            name.extend(first.to_uppercase());
            name.push_str(chars.as_str());
        }
    }
    name.push_str("ServiceProvider.php");
    name
}

/// Whether `path` is a file Drupal loads a service provider from: core's
/// own, or the `src/<Module>ServiceProvider.php` of an extension of the web
/// root outside test modules.
pub(crate) fn is_service_provider_file(path: &Path, web_root: &Path) -> bool {
    if !path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("ServiceProvider.php"))
        || !path.starts_with(web_root)
        || super::is_test_fixture(path, web_root)
    {
        return false;
    }
    if path == web_root.join(CORE_SERVICE_PROVIDER) {
        return true;
    }
    let Some(src) = path.parent() else {
        return false;
    };
    let Some(module) = src
        .parent()
        .and_then(|extension| extension.file_name())
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    src.file_name().is_some_and(|name| name == "src")
        && path
            .file_name()
            .is_some_and(|name| name.to_str() == Some(provider_file_name(module).as_str()))
}

/// Every service ID a service provider's `alter()` gives a class with a
/// literal `setClass()`, paired with that class.
///
/// The definition is the one a `getDefinition()` or `findDefinition()` call
/// with a literal ID returns, called on the chain itself or earlier assigned
/// to a variable.
pub(crate) fn service_class_replacements(content: &str) -> Vec<(Atom, Atom)> {
    if memchr::memmem::find(content.as_bytes(), b"setClass(").is_none() {
        return Vec::new();
    }
    let arena = LocalArena::new();
    let program = parse_file_content(&arena, FileId::new(b"input.php"), content.as_bytes());
    let names = OwnedResolvedNames::from_resolved(&NameResolver::new(&arena).resolve(program));
    let mut found = Vec::new();
    for statement in program.statements.iter() {
        match statement {
            Statement::Namespace(namespace) => {
                for statement in namespace.statements().iter() {
                    collect_from_class(statement, &names, content, &mut found);
                }
            }
            statement => collect_from_class(statement, &names, content, &mut found),
        }
    }
    found
}

fn collect_from_class(
    statement: &Statement<'_>,
    names: &OwnedResolvedNames,
    content: &str,
    found: &mut Vec<(Atom, Atom)>,
) {
    let Statement::Class(class) = statement else {
        return;
    };
    for member in class.members.iter() {
        if let ClassLikeMember::Method(method) = member
            && method.name.value.eq_ignore_ascii_case(b"alter")
            && let MethodBody::Concrete(body) = &method.body
        {
            collect_from_alter(body, names, content, found);
        }
    }
}

fn collect_from_alter(
    body: &Block<'_>,
    names: &OwnedResolvedNames,
    content: &str,
    found: &mut Vec<(Atom, Atom)>,
) {
    // The service each variable holds the definition of, as the statements
    // assign them in order.
    let mut definitions: HashMap<String, String> = HashMap::new();
    crate::virtual_members::laravel::walk_block_expressions(body, &mut |expr| {
        match expr {
            Expression::Assignment(assignment) if assignment.operator.is_assign() => {
                if let Expression::Variable(Variable::Direct(variable)) = assignment.lhs {
                    let name = bytes_to_str(variable.name).to_string();
                    match definition_id(assignment.rhs, &definitions, content) {
                        Some(id) => {
                            definitions.insert(name, id);
                        }
                        None => {
                            definitions.remove(&name);
                        }
                    }
                }
            }
            Expression::Call(Call::Method(call))
                if matches!(&call.method, ClassLikeMemberSelector::Identifier(name)
                    if name.value.eq_ignore_ascii_case(b"setClass")) =>
            {
                if let Some(Argument::Positional(class)) = call.argument_list.arguments.first()
                    && let Some(class) = class_name(class.value, names, content)
                    && let Some(id) = definition_id(call.object, &definitions, content)
                {
                    let pair = (atom(&id), atom(&class));
                    if !found.contains(&pair) {
                        found.push(pair);
                    }
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });
}

/// The service whose definition `expr` is: a `getDefinition('id')` or
/// `findDefinition('id')` call, a variable holding one, or a chain of the
/// definition's own setters on either, which return the definition.
fn definition_id(
    mut expr: &Expression<'_>,
    definitions: &HashMap<String, String>,
    content: &str,
) -> Option<String> {
    loop {
        match expr {
            Expression::Call(Call::Method(call)) => {
                if let ClassLikeMemberSelector::Identifier(name) = &call.method
                    && (name.value.eq_ignore_ascii_case(b"getDefinition")
                        || name.value.eq_ignore_ascii_case(b"findDefinition"))
                {
                    let Some(Argument::Positional(id)) = call.argument_list.arguments.first()
                    else {
                        return None;
                    };
                    let (id, _, _) = resolved_string_literal(id.value, content)?;
                    return (!id.is_empty()).then(|| id.to_string());
                }
                expr = call.object;
            }
            Expression::Parenthesized(inner) => expr = inner.expression,
            Expression::Variable(Variable::Direct(variable)) => {
                return definitions.get(bytes_to_str(variable.name)).cloned();
            }
            _ => return None,
        }
    }
}

/// The class a `setClass()` argument names: a `Foo::class` fetch or a
/// string spelling a class.
fn class_name(expr: &Expression<'_>, names: &OwnedResolvedNames, content: &str) -> Option<String> {
    if let Some(class) = names.class_fetch_fqn(expr) {
        return Some(class);
    }
    let (text, _, _) = resolved_string_literal(expr, content)?;
    let class = text.strip_prefix('\\').unwrap_or(text);
    (!class.is_empty() && !class.contains(char::is_whitespace)).then(|| class.to_string())
}

impl Backend {
    /// Read the service providers the Drupal walk found.
    pub(crate) fn note_drupal_service_providers(&self, files: Vec<PathBuf>) {
        let mut read = Vec::with_capacity(files.len());
        for path in files {
            let Some(content) = crate::util::read_php_source(&path) else {
                continue;
            };
            let replacements = service_class_replacements(&content);
            if !replacements.is_empty() {
                read.push((crate::util::path_to_uri(&path), replacements));
            }
        }
        self.drupal.service_replacements.write().extend(read);
    }

    /// Read a service provider again after its file changed.  Cheap for
    /// every other file: only a name ending in `ServiceProvider.php` in a
    /// Drupal project is looked at.
    pub(crate) fn refresh_drupal_service_provider(&self, uri: &str, content: &str) {
        if !uri.ends_with("ServiceProvider.php") || !self.is_drupal_project() {
            return;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return;
        };
        if !self
            .drupal
            .web_roots
            .read()
            .iter()
            .any(|root| is_service_provider_file(&path, root))
        {
            return;
        }
        let replacements = service_class_replacements(content);
        let mut all = self.drupal.service_replacements.write();
        if replacements.is_empty() {
            all.remove(uri);
        } else {
            all.insert(uri.to_string(), replacements);
        }
    }

    /// Forget what a deleted service provider replaced.
    pub(crate) fn forget_drupal_service_provider(&self, uri: &str) {
        if self.drupal.service_replacements.read().contains_key(uri) {
            self.drupal.service_replacements.write().remove(uri);
        }
    }

    /// The classes service providers give the service `id`, beside the one
    /// its YAML names.
    pub(crate) fn drupal_service_replacements(&self, id: &str) -> Vec<Atom> {
        let mut classes = Vec::new();
        for replacements in self.drupal.service_replacements.read().values() {
            for (replaced, class) in replacements {
                if replaced.as_str() == id && !classes.contains(class) {
                    classes.push(*class);
                }
            }
        }
        classes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replacements(php: &str) -> Vec<(String, String)> {
        service_class_replacements(php)
            .into_iter()
            .map(|(id, class)| (id.to_string(), class.to_string()))
            .collect()
    }

    #[test]
    fn set_class_calls_in_alter_are_read() {
        let php = r#"<?php
namespace Drupal\language;

use Drupal\Core\DependencyInjection\ServiceProviderBase;
use Drupal\inline_form_errors\FormErrorHandler;

class LanguageServiceProvider extends ServiceProviderBase {
    public function alter(ContainerBuilder $container) {
        $definition = $container->getDefinition('language_manager');
        $definition->setClass('Drupal\language\ConfigurableLanguageManager')
            ->addArgument(new Reference('config.factory'));
        $container->getDefinition('form_error_handler')
            ->setPublic(TRUE)
            ->setClass(FormErrorHandler::class);
        if ($container->hasDefinition('path_alias.repository')) {
            $alias = $container->findDefinition('path_alias.repository');
            $alias->setClass(AliasRepository::class);
        }
        $definition = new ChildDefinition('serializer.normalizer.config_entity');
        $definition->setClass(Normalizer::class);
    }

    public function register(ContainerBuilder $container) {
        $container->getDefinition('other')->setClass(Other::class);
    }
}
"#;
        assert_eq!(
            replacements(php),
            vec![
                (
                    "language_manager".to_string(),
                    "Drupal\\language\\ConfigurableLanguageManager".to_string()
                ),
                (
                    "form_error_handler".to_string(),
                    "Drupal\\inline_form_errors\\FormErrorHandler".to_string()
                ),
                (
                    "path_alias.repository".to_string(),
                    "Drupal\\language\\AliasRepository".to_string()
                ),
            ]
        );
    }

    #[test]
    fn provider_files_are_found_by_the_name_drupal_gives_them() {
        let web = Path::new("/site/web");
        for (path, expected) in [
            (
                "/site/web/core/modules/language/src/LanguageServiceProvider.php",
                true,
            ),
            (
                "/site/web/modules/contrib/inline_form_errors/src/InlineFormErrorsServiceProvider.php",
                true,
            ),
            (
                "/site/web/core/lib/Drupal/Core/CoreServiceProvider.php",
                true,
            ),
            (
                "/site/web/core/lib/Drupal/Core/Installer/NormalInstallerServiceProvider.php",
                false,
            ),
            (
                "/site/web/modules/custom/acme/src/OtherServiceProvider.php",
                false,
            ),
            (
                "/site/web/core/modules/system/tests/modules/acme/src/AcmeServiceProvider.php",
                false,
            ),
            ("/elsewhere/acme/src/AcmeServiceProvider.php", false),
        ] {
            assert_eq!(
                is_service_provider_file(Path::new(path), web),
                expected,
                "{path}"
            );
        }
    }
}
