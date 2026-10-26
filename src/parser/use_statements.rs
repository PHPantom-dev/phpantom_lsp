//! `use` statement and namespace extraction.
//!
//! This module handles parsing PHP `use` statements and namespace
//! declarations from the AST, building a mapping of short (imported)
//! names to their fully-qualified equivalents.

use std::collections::HashMap;

use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::types::SymbolImports;
use crate::util::short_name;

impl Backend {
    /// Walk statements and extract `use` statement mappings.
    ///
    /// Class imports go to `use_map`.  Function and constant imports go to
    /// `symbol_imports` when the caller wants them, and are dropped
    /// otherwise; they never land in `use_map`, where they would answer
    /// for a class of the same short name.
    pub(crate) fn extract_use_statements_from_statements<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        use_map: &mut HashMap<String, String>,
        mut symbol_imports: Option<&mut SymbolImports>,
    ) {
        for statement in statements {
            match statement {
                Statement::Use(use_stmt) => {
                    Self::extract_use_items(
                        &use_stmt.items,
                        use_map,
                        symbol_imports.as_deref_mut(),
                    );
                }
                Statement::Namespace(namespace) => {
                    // Recurse into namespace bodies to find use statements
                    Self::extract_use_statements_from_statements(
                        namespace.statements().iter(),
                        use_map,
                        symbol_imports.as_deref_mut(),
                    );
                }
                _ => {}
            }
        }
    }

    /// Extract individual use items from a `UseItems` node, sorting them
    /// by kind as [`extract_use_statements_from_statements`](Self::extract_use_statements_from_statements)
    /// describes.
    pub(crate) fn extract_use_items(
        items: &UseItems,
        use_map: &mut HashMap<String, String>,
        mut symbol_imports: Option<&mut SymbolImports>,
    ) {
        let mut register = |item: &UseItem, prefix: Option<&str>, kind: Option<&UseType>| {
            let table = match kind {
                None => &mut *use_map,
                Some(UseType::Function(_)) => match symbol_imports.as_deref_mut() {
                    Some(imports) => &mut imports.functions,
                    None => return,
                },
                Some(UseType::Const(_)) => match symbol_imports.as_deref_mut() {
                    Some(imports) => &mut imports.constants,
                    None => return,
                },
            };
            Self::register_use_item(item, prefix, table);
        };
        match items {
            UseItems::Sequence(seq) => {
                // `use Foo\Bar;` or `use Foo\Bar, Baz\Qux;`
                for item in seq.items.iter() {
                    register(item, None, None);
                }
            }
            UseItems::TypedSequence(seq) => {
                // `use function Foo\bar;` or `use const Foo\BAR;`
                for item in seq.items.iter() {
                    register(item, None, Some(&seq.r#type));
                }
            }
            UseItems::TypedList(list) => {
                // `use function Foo\{bar, baz};` or `use const Foo\{BAR, BAZ};`
                let prefix = bytes_to_str(list.namespace.value());
                for item in list.items.iter() {
                    register(item, Some(prefix), Some(&list.r#type));
                }
            }
            UseItems::MixedList(list) => {
                // `use Foo\{Bar, function baz, const QUX};`
                let prefix = bytes_to_str(list.namespace.value());
                for maybe_typed in list.items.iter() {
                    register(&maybe_typed.item, Some(prefix), maybe_typed.r#type.as_ref());
                }
            }
        }
    }

    /// Register a single `UseItem` into an import table.
    ///
    /// If `group_prefix` is `Some`, the item name is relative to that prefix
    /// (e.g. for `use Foo\{Bar}`, prefix is `"Foo"` and item name is `"Bar"`,
    /// giving FQN `"Foo\Bar"`).
    fn register_use_item(
        item: &UseItem,
        group_prefix: Option<&str>,
        table: &mut HashMap<String, String>,
    ) {
        // `use \Foo\Bar;` is legal PHP, but every FQN this codebase stores
        // is unprefixed, so normalize here rather than leaving each
        // consumer of the table to strip it.
        let item_name = bytes_to_str(item.name.value()).trim_start_matches('\\');

        let fqn = if let Some(prefix) = group_prefix {
            format!("{}\\{}", prefix.trim_start_matches('\\'), item_name)
        } else {
            item_name.to_string()
        };

        // The short (imported) name is either the alias or the last segment
        let alias_name = if let Some(ref alias) = item.alias {
            bytes_to_str(alias.identifier.value).to_string()
        } else {
            short_name(&fqn).to_string()
        };

        table.insert(alias_name, fqn);
    }

    /// Walk statements and extract the first namespace declaration found.
    pub(crate) fn extract_namespace_from_statements<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
    ) -> Option<String> {
        for statement in statements {
            if let Statement::Namespace(namespace) = statement {
                // The namespace name is an `Option<Identifier>`.
                // Both implicit (`namespace Foo;`) and brace-delimited
                // (`namespace Foo { ... }`) forms may have a name.
                if let Some(ident) = &namespace.name {
                    let name = bytes_to_str(ident.value());
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grouped `use` statement: `use Foo\{Bar, Baz};`
    ///
    /// This is the syntax reported in issue #42 — verify that both the
    /// legacy `extract_use_items` path and the new `mago-names` resolver
    /// produce correct mappings.
    #[test]
    fn grouped_use_populates_use_map_and_resolved_names() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace Controllers\Registration;

use Models\Common\{Disciplines, TeamMembers, TournamentLeagueRosters, TournamentsLeagues};

class RegistrationController {
    public function foo(Disciplines $d): TeamMembers {
    }
}
"#;
        backend.update_ast(uri, content);

        // ── Legacy use_map ──────────────────────────────────────────
        let file_map = backend.file_use_map(uri);

        assert_eq!(
            file_map.get("Disciplines"),
            Some(&"Models\\Common\\Disciplines".to_string()),
            "Disciplines should be in the use_map"
        );
        assert_eq!(
            file_map.get("TeamMembers"),
            Some(&"Models\\Common\\TeamMembers".to_string()),
            "TeamMembers should be in the use_map"
        );
        assert_eq!(
            file_map.get("TournamentLeagueRosters"),
            Some(&"Models\\Common\\TournamentLeagueRosters".to_string()),
            "TournamentLeagueRosters should be in the use_map"
        );
        assert_eq!(
            file_map.get("TournamentsLeagues"),
            Some(&"Models\\Common\\TournamentsLeagues".to_string()),
            "TournamentsLeagues should be in the use_map"
        );

        // ── mago-names resolved_names ───────────────────────────────
        let rn = backend
            .file_state(uri)
            .and_then(|state| state.resolved_names.clone())
            .expect("resolved_names should have an entry for the file");

        // The `Disciplines` type hint in `foo(Disciplines $d)` should
        // resolve to its FQN via the grouped import.
        let hint_offset = content
            .find("Disciplines $d")
            .expect("should find Disciplines type hint") as u32;
        assert_eq!(
            rn.get(hint_offset),
            Some("Models\\Common\\Disciplines"),
            "mago-names should resolve Disciplines type hint to FQN"
        );

        // The `TeamMembers` return type should also resolve.
        let ret_offset = content
            .find("): TeamMembers")
            .map(|p| p + "): ".len())
            .expect("should find TeamMembers return type") as u32;
        assert_eq!(
            rn.get(ret_offset),
            Some("Models\\Common\\TeamMembers"),
            "mago-names should resolve TeamMembers return type to FQN"
        );
    }

    /// Aliased grouped `use`: `use Foo\{Bar as B, Baz};`
    #[test]
    fn grouped_use_with_alias() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse Models\\Common\\{Disciplines as Disc, TeamMembers};\n\nclass X extends Disc {}\n";

        backend.update_ast(uri, content);

        let file_map = backend.file_use_map(uri);

        assert_eq!(
            file_map.get("Disc"),
            Some(&"Models\\Common\\Disciplines".to_string()),
            "aliased short name should map to the full FQN"
        );
        assert_eq!(
            file_map.get("TeamMembers"),
            Some(&"Models\\Common\\TeamMembers".to_string()),
        );
        // The original name should NOT appear — only the alias.
        assert!(
            !file_map.contains_key("Disciplines"),
            "original name should not be in the use_map when aliased"
        );
    }

    #[test]
    fn multiline_grouped_use_populates_use_map_and_resolved_names() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace Controllers\Registration;

use Models\Common\{
    Disciplines,
    TeamMembers
};

class RegistrationController {
    public function foo(Disciplines $d): TeamMembers {
    }
}
"#;
        backend.update_ast(uri, content);

        let file_map = backend.file_use_map(uri);

        assert_eq!(
            file_map.get("Disciplines"),
            Some(&"Models\\Common\\Disciplines".to_string())
        );
        assert_eq!(
            file_map.get("TeamMembers"),
            Some(&"Models\\Common\\TeamMembers".to_string())
        );

        let rn = backend
            .file_state(uri)
            .and_then(|state| state.resolved_names.clone())
            .expect("resolved_names should have an entry for the file");

        let hint_offset = content
            .find("Disciplines $d")
            .expect("should find Disciplines type hint") as u32;
        assert_eq!(rn.get(hint_offset), Some("Models\\Common\\Disciplines"));

        let ret_offset = content
            .find("): TeamMembers")
            .map(|p| p + "): ".len())
            .expect("should find TeamMembers return type") as u32;
        assert_eq!(rn.get(ret_offset), Some("Models\\Common\\TeamMembers"));
    }

    /// The function and constant imports of `uri`, read from the index.
    fn symbol_imports(backend: &Backend, uri: &str) -> SymbolImports {
        backend.symbol_imports_for(uri)
    }

    /// `use function Foo\bar;` is a function import, not a class import.
    #[test]
    fn use_function_populates_function_imports() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace Tests\Unit;

use function PHPUnit\Framework\assertSame;
use function PHPUnit\Framework\assertCount;

class MyTest {}
"#;
        backend.update_ast(uri, content);

        let imports = symbol_imports(&backend, uri);
        assert_eq!(
            imports.functions.get("assertSame"),
            Some(&"PHPUnit\\Framework\\assertSame".to_string()),
        );
        assert_eq!(
            imports.functions.get("assertCount"),
            Some(&"PHPUnit\\Framework\\assertCount".to_string()),
        );
        assert!(backend.file_use_map(uri).is_empty());
    }

    /// `use function Foo\{bar, baz};` (grouped) is a function import too.
    #[test]
    fn use_function_grouped_populates_function_imports() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace Tests\Unit;

use function PHPUnit\Framework\{assertSame, assertCount};

class MyTest {}
"#;
        backend.update_ast(uri, content);

        let imports = symbol_imports(&backend, uri);
        assert_eq!(
            imports.functions.get("assertSame"),
            Some(&"PHPUnit\\Framework\\assertSame".to_string()),
        );
        assert_eq!(
            imports.functions.get("assertCount"),
            Some(&"PHPUnit\\Framework\\assertCount".to_string()),
        );
        assert!(backend.file_use_map(uri).is_empty());
    }

    /// `use const Foo\BAR;` is a constant import.
    #[test]
    fn use_const_populates_constant_imports() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace App;

use const JSON_THROW_ON_ERROR;

class MyClass {}
"#;
        backend.update_ast(uri, content);

        let imports = symbol_imports(&backend, uri);
        assert_eq!(
            imports.constants.get("JSON_THROW_ON_ERROR"),
            Some(&"JSON_THROW_ON_ERROR".to_string()),
        );
        assert!(backend.file_use_map(uri).is_empty());
    }

    /// Mixed `use Foo\{Bar, function baz, const QUX};` sorts each item
    /// into the table for its kind.
    #[test]
    fn mixed_use_sorts_items_by_kind() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse App\\{MyClass, function myFunc, const MY_CONST};\n";
        backend.update_ast(uri, content);

        let use_map = backend.file_use_map(uri);
        assert_eq!(use_map.get("MyClass"), Some(&"App\\MyClass".to_string()));
        assert_eq!(
            use_map.len(),
            1,
            "only the class import is a class: {use_map:?}"
        );

        let imports = symbol_imports(&backend, uri);
        assert_eq!(
            imports.functions.get("myFunc"),
            Some(&"App\\myFunc".to_string())
        );
        assert_eq!(
            imports.constants.get("MY_CONST"),
            Some(&"App\\MY_CONST".to_string())
        );
    }
}
