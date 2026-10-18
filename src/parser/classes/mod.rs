//! Class, interface, trait, and enum extraction.
//!
//! Each class-like declaration is tagged with a [`ClassLikeKind`] so that
//! downstream consumers (e.g. `throw new` completion) can distinguish
//! concrete classes from interfaces, traits, and enums.
//!
//! This module walks statements for the class-like declarations: `class`,
//! `interface`, `trait`, and `enum`, including those nested in conditional
//! and control-flow blocks.  [`class_likes`] builds each kind's
//! `ClassInfo` from its header and docblock (`@template`, `@extends`,
//! `@mixin`, `@deprecated`, …), and [`members`] extracts the methods,
//! properties, constants and trait uses of its body.
//!
//! Anonymous classes (`new class { ... }`) are extracted separately, by
//! the sibling [`super::anonymous`] module.

mod class_likes;
mod members;
mod methods;
mod properties;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{Atom, atom};
use crate::types::*;

use super::{DocblockCtx, for_each_nested_statement_list, is_removed_for_version};

impl Backend {
    /// De-duplicate parsed class-likes by `(name, namespace)`, keeping the
    /// first declaration in source order.
    ///
    /// A class-like declared in more than one branch of a conditional —
    /// e.g. Doctrine's `ServiceEntityRepository`, defined differently for
    /// ORM2 vs ORM3 inside an `if`/`else` version guard — yields one
    /// [`ClassInfo`] per branch once we descend into conditional bodies.
    /// Keeping the first declaration makes resolution deterministic and
    /// matches PHPantom's existing first-occurrence-wins convention
    /// (`classmap_scanner`, `find_class_by_name`, `fqn_uri_index`) as well
    /// as PHPStan/Psalm. This must run before the classes reach
    /// `fqn_class_index`, whose insert is last-wins and would otherwise pick
    /// the wrong (later) branch.
    pub(crate) fn dedup_class_likes_first_wins<T>(
        items: &mut Vec<T>,
        class_and_namespace: impl Fn(&T) -> (&ClassInfo, &Option<String>),
    ) {
        let mut seen: std::collections::HashSet<(Atom, Option<String>)> =
            std::collections::HashSet::new();
        items.retain(|item| {
            let (cls, ns) = class_and_namespace(item);
            seen.insert((cls.name, ns.clone()))
        });
    }

    /// Whether `stmt` may itself declare a class-like, or contain one
    /// nested in its body, and so needs to be handed to
    /// [`extract_classes_from_statements`](Self::extract_classes_from_statements).
    ///
    /// Beyond the four class-like kinds, this covers the conditional and
    /// control-flow statements a class-like can be nested inside: version
    /// guards (`if (! class_exists(...)) { class Foo {} }`), Doctrine-style
    /// `ServiceEntityRepository` shims, and similar patterns real code
    /// uses to conditionally define a class or trait.
    pub(crate) fn is_classlike_extraction_candidate(stmt: &Statement<'_>) -> bool {
        matches!(
            stmt,
            Statement::Class(_)
                | Statement::Interface(_)
                | Statement::Trait(_)
                | Statement::Enum(_)
                | Statement::If(_)
                | Statement::Block(_)
                | Statement::Try(_)
                | Statement::Switch(_)
                | Statement::While(_)
                | Statement::DoWhile(_)
                | Statement::For(_)
                | Statement::Foreach(_)
        )
    }

    /// Recursively walk statements and extract class information.
    /// This handles classes at the top level as well as classes nested
    /// inside namespace declarations.
    pub(crate) fn extract_classes_from_statements<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for statement in statements {
            match statement {
                Statement::Class(class) => {
                    if removed_in_target_version(class, doc_ctx) {
                        continue;
                    }
                    classes.push(class_likes::extract_class(class, doc_ctx));
                    // Walk method bodies for anonymous classes.
                    Self::find_anonymous_classes_in_members(class.members.iter(), classes, doc_ctx);
                }
                Statement::Interface(iface) => {
                    if removed_in_target_version(iface, doc_ctx) {
                        continue;
                    }
                    classes.push(class_likes::extract_interface(iface, doc_ctx));
                    Self::find_anonymous_classes_in_members(iface.members.iter(), classes, doc_ctx);
                }
                Statement::Trait(trait_def) => {
                    if removed_in_target_version(trait_def, doc_ctx) {
                        continue;
                    }
                    classes.push(class_likes::extract_trait(trait_def, doc_ctx));
                    Self::find_anonymous_classes_in_members(
                        trait_def.members.iter(),
                        classes,
                        doc_ctx,
                    );
                }
                Statement::Enum(enum_def) => {
                    if removed_in_target_version(enum_def, doc_ctx) {
                        continue;
                    }
                    classes.push(class_likes::extract_enum(enum_def, doc_ctx));
                    Self::find_anonymous_classes_in_members(
                        enum_def.members.iter(),
                        classes,
                        doc_ctx,
                    );
                }
                // Named class-likes can be declared inside conditional and
                // control-flow blocks — most notably Doctrine's
                // `ServiceEntityRepository`, defined inside an
                // `if (! property_exists(EntityRepository::class, '_entityName'))`
                // guard that selects the ORM2 vs ORM3 base class. Descend into
                // these container bodies so such declarations are indexed with
                // their parent class and `@extends` generics, not merely
                // discovered by name. Anonymous classes nested in the
                // non-container statements within these bodies are still
                // collected via the `_` arm when the recursion reaches them.
                Statement::Namespace(_)
                | Statement::If(_)
                | Statement::Block(_)
                | Statement::Try(_)
                | Statement::Switch(_)
                | Statement::While(_)
                | Statement::DoWhile(_)
                | Statement::For(_)
                | Statement::Foreach(_) => {
                    for_each_nested_statement_list(statement, |statements| {
                        Self::extract_classes_from_statements(statements.iter(), classes, doc_ctx);
                    });
                }
                _ => {
                    // Walk into all other statement types to find anonymous
                    // classes nested inside expressions, control flow, method
                    // bodies, closures, etc.
                    Self::find_anonymous_classes_in_statement(statement, classes, doc_ctx);
                }
            }
        }
    }
}

/// Whether `node`'s docblock has `@removed X.Y` with X.Y at or below the
/// target PHP version, which drops the declaration.
fn removed_in_target_version(node: &impl HasSpan, doc_ctx: Option<&DocblockCtx<'_>>) -> bool {
    doc_ctx.is_some_and(|ctx| {
        ctx.php_version
            .is_some_and(|ver| is_removed_for_version(node, ctx, ver))
    })
}

/// The FQN of a class name written in code, read against the file's
/// imports and namespace.  Without a [`DocblockCtx`] to read it against,
/// the name is taken as written.
fn resolve_written_class(name: &str, doc_ctx: Option<&DocblockCtx<'_>>) -> String {
    match doc_ctx {
        Some(ctx) => crate::names::resolve_to_fqn(name, &ctx.use_map, &ctx.namespace),
        None => name.strip_prefix('\\').unwrap_or(name).to_string(),
    }
}

/// PHP implicitly adds `Stringable` to every class and interface that
/// declares `__toString()`.  Mirror that so hierarchy checks accept such
/// a class wherever `Stringable` is expected.  The leading backslash
/// keeps `resolve_to_fqn` from prepending the current namespace.
pub(super) fn add_implicit_stringable(interfaces: &mut Vec<Atom>, methods: &[MethodInfo]) {
    if methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("__toString"))
        && !interfaces.iter().any(|i| {
            i.trim_start_matches('\\')
                .eq_ignore_ascii_case("Stringable")
        })
    {
        interfaces.push(atom("\\Stringable"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A class declared inside an `if` block (e.g. a version guard, like
    /// Doctrine's `ServiceEntityRepository`) must be indexed with its native
    /// parent and its `@extends Parent<Concrete>` generics — not merely
    /// discovered by name.
    #[test]
    fn conditional_class_inside_if_is_extracted_with_parent_and_generics() {
        let src = r#"<?php
/** @template T of object */
class Repo {}
class Entity {}
if (\PHP_VERSION_ID >= 80000) {
    /** @extends Repo<Entity> */
    class ConditionalRepo extends Repo {}
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let conditional = classes
            .iter()
            .find(|(c, _)| c.name == atom("ConditionalRepo"))
            .map(|(c, _)| c)
            .expect("class declared inside `if` should be indexed");

        assert_eq!(
            conditional.parent_class,
            Some(atom("Repo")),
            "conditional class should carry its native parent",
        );
        assert!(
            conditional
                .extends_generics
                .iter()
                .any(|(parent, args)| *parent == atom("Repo") && !args.is_empty()),
            "conditional class should carry its `@extends Repo<Entity>` generics, got {:?}",
            conditional.extends_generics,
        );
    }

    /// When the same class name is declared in both branches of a conditional
    /// (the Doctrine ORM2-vs-ORM3 shape), the first declaration in source
    /// order wins and exactly one `ClassInfo` is produced.
    #[test]
    fn conditional_class_in_both_branches_keeps_first() {
        let src = r#"<?php
if (\defined('SOME_FLAG')) {
    class Dup extends First {}
} else {
    class Dup extends Second {}
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let dups: Vec<_> = classes
            .iter()
            .filter(|(c, _)| c.name == atom("Dup"))
            .collect();

        assert_eq!(
            dups.len(),
            1,
            "duplicate-branch class must be de-duplicated"
        );
        assert_eq!(
            dups[0].0.parent_class,
            Some(atom("First")),
            "first (source-order) branch should win",
        );
    }
}
