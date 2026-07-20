/// `use` statement and namespace extraction.
///
/// This module handles parsing PHP `use` statements and namespace
/// declarations from the AST, building a mapping of short (imported)
/// names to their fully-qualified equivalents.
use std::collections::HashMap;

use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::util::short_name;

impl Backend {
    /// Walk statements and extract `use` statement mappings.
    pub(crate) fn extract_use_statements_from_statements<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        use_map: &mut HashMap<String, String>,
    ) {
        for statement in statements {
            match statement {
                Statement::Use(use_stmt) => {
                    Self::extract_use_items(&use_stmt.items, use_map);
                }
                Statement::Namespace(namespace) => {
                    // Recurse into namespace bodies to find use statements
                    Self::extract_use_statements_from_statements(
                        namespace.statements().iter(),
                        use_map,
                    );
                }
                _ => {}
            }
        }
    }

    /// Extract individual use items from a `UseItems` node.
    pub(crate) fn extract_use_items(items: &UseItems, use_map: &mut HashMap<String, String>) {
        match items {
            UseItems::Sequence(seq) => {
                // `use Foo\Bar;` or `use Foo\Bar, Baz\Qux;`
                for item in seq.items.iter() {
                    Self::register_use_item(item, None, use_map);
                }
            }
            UseItems::TypedSequence(seq) => {
                // `use function Foo\bar;` or `use const Foo\BAR;`
                // Function and constant imports are included in the
                // use_map so that `resolve_function_name` /
                // `resolve_class_name` can find them.  Class resolution
                // harmlessly ignores entries that don't match a class.
                for item in seq.items.iter() {
                    Self::register_use_item(item, None, use_map);
                }
            }
            UseItems::TypedList(list) => {
                // `use function Foo\{bar, baz};` or `use const Foo\{BAR, BAZ};`
                let prefix = bytes_to_str(list.namespace.value());
                for item in list.items.iter() {
                    Self::register_use_item(item, Some(prefix), use_map);
                }
            }
            UseItems::MixedList(list) => {
                // `use Foo\{Bar, function baz, const QUX};`
                let prefix = bytes_to_str(list.namespace.value());
                for maybe_typed in list.items.iter() {
                    Self::register_use_item(&maybe_typed.item, Some(prefix), use_map);
                }
            }
        }
    }

    /// Register a single `UseItem` into the use_map.
    ///
    /// If `group_prefix` is `Some`, the item name is relative to that prefix
    /// (e.g. for `use Foo\{Bar}`, prefix is `"Foo"` and item name is `"Bar"`,
    /// giving FQN `"Foo\Bar"`).
    fn register_use_item(
        item: &UseItem,
        group_prefix: Option<&str>,
        use_map: &mut HashMap<String, String>,
    ) {
        let item_name = bytes_to_str(item.name.value());

        // Build the fully-qualified name
        let fqn = if let Some(prefix) = group_prefix {
            format!("{}\\{}", prefix, item_name)
        } else {
            item_name.to_string()
        };

        // The short (imported) name is either the alias or the last segment
        let alias_name = if let Some(ref alias) = item.alias {
            bytes_to_str(alias.identifier.value).to_string()
        } else {
            // Last segment of the FQN
            short_name(&fqn).to_string()
        };

        use_map.insert(alias_name, fqn);
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
        let use_map = backend.file_imports.read();
        let file_map = use_map
            .get(uri)
            .expect("use_map should have an entry for the file");

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
        drop(use_map);

        // ── mago-names resolved_names ───────────────────────────────
        let resolved = backend.resolved_names.read();
        let rn = resolved
            .get(uri)
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

        let use_map = backend.file_imports.read();
        let file_map = use_map.get(uri).expect("use_map entry");

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

        let use_map = backend.file_imports.read();
        let file_map = use_map
            .get(uri)
            .expect("use_map should have an entry for the file");

        assert_eq!(
            file_map.get("Disciplines"),
            Some(&"Models\\Common\\Disciplines".to_string())
        );
        assert_eq!(
            file_map.get("TeamMembers"),
            Some(&"Models\\Common\\TeamMembers".to_string())
        );
        drop(use_map);

        let resolved = backend.resolved_names.read();
        let rn = resolved
            .get(uri)
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

    /// `use function Foo\bar;` should populate the use_map.
    #[test]
    fn use_function_populates_use_map() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace Tests\Unit;

use function PHPUnit\Framework\assertSame;
use function PHPUnit\Framework\assertCount;

class MyTest {}
"#;
        backend.update_ast(uri, content);

        let use_map = backend.file_imports.read();
        let file_map = use_map
            .get(uri)
            .expect("use_map should have an entry for the file");

        assert_eq!(
            file_map.get("assertSame"),
            Some(&"PHPUnit\\Framework\\assertSame".to_string()),
            "use function should add assertSame to use_map"
        );
        assert_eq!(
            file_map.get("assertCount"),
            Some(&"PHPUnit\\Framework\\assertCount".to_string()),
            "use function should add assertCount to use_map"
        );
    }

    /// `use function Foo\{bar, baz};` (grouped) should populate the use_map.
    #[test]
    fn use_function_grouped_populates_use_map() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace Tests\Unit;

use function PHPUnit\Framework\{assertSame, assertCount};

class MyTest {}
"#;
        backend.update_ast(uri, content);

        let use_map = backend.file_imports.read();
        let file_map = use_map
            .get(uri)
            .expect("use_map should have an entry for the file");

        assert_eq!(
            file_map.get("assertSame"),
            Some(&"PHPUnit\\Framework\\assertSame".to_string()),
            "grouped use function should add assertSame to use_map"
        );
        assert_eq!(
            file_map.get("assertCount"),
            Some(&"PHPUnit\\Framework\\assertCount".to_string()),
            "grouped use function should add assertCount to use_map"
        );
    }

    /// `use const Foo\BAR;` should populate the use_map.
    #[test]
    fn use_const_populates_use_map() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace App;

use const JSON_THROW_ON_ERROR;

class MyClass {}
"#;
        backend.update_ast(uri, content);

        let use_map = backend.file_imports.read();
        let file_map = use_map
            .get(uri)
            .expect("use_map should have an entry for the file");

        assert_eq!(
            file_map.get("JSON_THROW_ON_ERROR"),
            Some(&"JSON_THROW_ON_ERROR".to_string()),
            "use const should add JSON_THROW_ON_ERROR to use_map"
        );
    }

    /// Mixed `use Foo\{Bar, function baz, const QUX};` should include all items.
    #[test]
    fn mixed_use_includes_functions_and_consts() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse App\\{MyClass, function myFunc, const MY_CONST};\n";
        backend.update_ast(uri, content);

        let use_map = backend.file_imports.read();
        let file_map = use_map
            .get(uri)
            .expect("use_map should have an entry for the file");

        assert_eq!(
            file_map.get("MyClass"),
            Some(&"App\\MyClass".to_string()),
            "mixed use should include class import"
        );
        assert_eq!(
            file_map.get("myFunc"),
            Some(&"App\\myFunc".to_string()),
            "mixed use should include function import"
        );
        assert_eq!(
            file_map.get("MY_CONST"),
            Some(&"App\\MY_CONST".to_string()),
            "mixed use should include const import"
        );
    }
}
