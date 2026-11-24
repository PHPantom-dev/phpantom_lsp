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
        Self::extract_classes_nested(statements, classes, doc_ctx, AutoloadGuard::None);
    }

    /// [`extract_classes_from_statements`](Self::extract_classes_from_statements)
    /// for statements under `guard`, which marks the named class-likes it
    /// applies to with [`ClassInfo::yields_to_autoloader`].
    fn extract_classes_nested<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
        guard: AutoloadGuard<'a>,
    ) {
        let named = |mut class: ClassInfo| {
            class.yields_to_autoloader = guard.applies_to(&class.name);
            class
        };
        for statement in statements {
            match statement {
                Statement::Class(class) => {
                    if removed_in_target_version(class, doc_ctx) {
                        continue;
                    }
                    classes.push(named(class_likes::extract_class(class, doc_ctx)));
                    // Walk method bodies for anonymous classes.
                    Self::find_anonymous_classes_in_members(class.members.iter(), classes, doc_ctx);
                }
                Statement::Interface(iface) => {
                    if removed_in_target_version(iface, doc_ctx) {
                        continue;
                    }
                    classes.push(named(class_likes::extract_interface(iface, doc_ctx)));
                    Self::find_anonymous_classes_in_members(iface.members.iter(), classes, doc_ctx);
                }
                Statement::Trait(trait_def) => {
                    if removed_in_target_version(trait_def, doc_ctx) {
                        continue;
                    }
                    classes.push(named(class_likes::extract_trait(trait_def, doc_ctx)));
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
                    classes.push(named(class_likes::extract_enum(enum_def, doc_ctx)));
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
                Statement::If(if_stmt) => {
                    let (when_true, when_false) = AutoloadGuard::of_condition(if_stmt.condition);
                    let (then_guard, else_guard) = (guard.or(when_true), guard.or(when_false));
                    let mut branch = 0;
                    for_each_nested_statement_list(statement, |statements| {
                        // The first list visited is the `if` body itself;
                        // its `elseif` and `else` bodies run when the
                        // condition does not hold.
                        let guard = if branch == 0 { then_guard } else { else_guard };
                        branch += 1;
                        Self::extract_classes_nested(statements.iter(), classes, doc_ctx, guard);
                    });
                }
                Statement::Namespace(_)
                | Statement::Block(_)
                | Statement::Try(_)
                | Statement::Switch(_)
                | Statement::While(_)
                | Statement::DoWhile(_)
                | Statement::For(_)
                | Statement::Foreach(_) => {
                    for_each_nested_statement_list(statement, |statements| {
                        Self::extract_classes_nested(statements.iter(), classes, doc_ctx, guard);
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

/// A condition under which PHP only runs a class declaration when the
/// autoloader has no other file for the name, or never runs it.
#[derive(Clone, Copy)]
enum AutoloadGuard<'a> {
    None,
    /// `if (! class_exists(Foo::class)) { class Foo {} }`: the check
    /// autoloads `Foo` first, so the declaration below only runs when no
    /// file declares it.  Holds the short name checked.
    Autoloads(&'a str),
    /// `if (false) { class Foo {} }`: a declaration for tools to read,
    /// which PHP never runs.
    Never,
}

impl<'a> AutoloadGuard<'a> {
    /// The guards an `if` condition puts on the body that runs when it
    /// holds and on the bodies that run when it does not.
    ///
    /// Only conditions that settle the question count: an existence check
    /// with autoloading off (`class_exists(Foo::class, false)`) is how a
    /// file Composer loads at bootstrap defines the class itself before
    /// anything could autoload it, and a version check picks the variant
    /// that runs.
    fn of_condition(condition: &'a Expression<'a>) -> (Self, Self) {
        use crate::parser::unwrap_parens;
        match unwrap_parens(condition) {
            Expression::Literal(Literal::False(_)) => (Self::Never, Self::None),
            Expression::Literal(Literal::True(_)) => (Self::None, Self::Never),
            Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
                let (when_true, when_false) = Self::of_condition(prefix.operand);
                (when_false, when_true)
            }
            Expression::Call(Call::Function(call)) => match Self::existence_check(call) {
                Some(name) => (Self::None, Self::Autoloads(name)),
                None => (Self::None, Self::None),
            },
            _ => (Self::None, Self::None),
        }
    }

    /// The short name `call` checks the existence of, when it is a
    /// `class_exists`-like check that autoloads the name first.
    fn existence_check(call: &'a FunctionCall<'a>) -> Option<&'a str> {
        use crate::parser::unwrap_parens;
        let Expression::Identifier(function) = call.function else {
            return None;
        };
        let function = crate::atom::bytes_to_str(function.value());
        let function = function.strip_prefix('\\').unwrap_or(function);
        if ![
            "class_exists",
            "interface_exists",
            "trait_exists",
            "enum_exists",
        ]
        .iter()
        .any(|f| function.eq_ignore_ascii_case(f))
        {
            return None;
        }
        let mut arguments = call.argument_list.arguments.iter();
        let checked = arguments.next()?.value();
        // Autoloading is on unless the second argument says otherwise.
        if let Some(autoload) = arguments.next()
            && !matches!(
                unwrap_parens(autoload.value()),
                Expression::Literal(Literal::True(_))
            )
        {
            return None;
        }
        let name = match crate::parser::class_fetch_identifier(checked) {
            Some(ident) => crate::atom::bytes_to_str(ident.value()),
            None => match checked {
                Expression::Literal(Literal::String(string)) => {
                    crate::atom::literal_bytes_to_str(string.value?)?
                }
                _ => return None,
            },
        };
        Some(crate::util::short_name(name))
    }

    /// `self`, unless it puts no guard on the body and `inner` does.
    fn or(self, inner: Self) -> Self {
        match self {
            Self::None => inner,
            _ => self,
        }
    }

    /// Whether the guard applies to a declaration of `short_name`.
    fn applies_to(self, short_name: &str) -> bool {
        match self {
            Self::None => false,
            Self::Never => true,
            Self::Autoloads(checked) => checked.eq_ignore_ascii_case(short_name),
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
/// imports and namespace. Keep the leading backslash so the later declaration
/// qualification pass recognizes it as resolved. Without a [`DocblockCtx`],
/// the name is taken as written.
fn resolve_written_class(name: &str, doc_ctx: Option<&DocblockCtx<'_>>) -> String {
    match doc_ctx {
        Some(ctx) => format!(
            "\\{}",
            crate::names::resolve_to_fqn(name, &ctx.use_map, &ctx.namespace)
        ),
        None => name.to_string(),
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

    #[test]
    fn inferred_class_without_context_preserves_its_written_name() {
        for name in ["Repository", "App\\Repository", "\\Repository"] {
            assert_eq!(resolve_written_class(name, None), name);
        }
    }

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

    /// Only a guard that defers to the autoloader, or one that never
    /// runs, marks the declarations under it, and an autoloading check
    /// only marks the name it checks.
    #[test]
    fn declarations_behind_an_autoloading_guard_yield_to_the_autoloader() {
        let src = r#"<?php
namespace Acme {
    class Plain {}
    if (!class_exists(Checked::class)) { class Checked {} class Other {} }
    if (!\interface_exists('Acme\\Iface')) { while (true) { interface Iface {} } }
    if (!(trait_exists(Tr::class, true))) { trait Tr {} }
    if (!class_exists(NoAutoload::class, false)) { class NoAutoload {} }
    if (false) { enum Never {} }
    if (\PHP_VERSION_ID >= 80000) { class Versioned {} }
    if (class_exists(InElse::class)) {} else { class InElse {} }
    if (!class_exists(Loaded::class)) {} else { class Loaded {} }
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let yields = |name: &str| {
            classes
                .iter()
                .find(|(c, _)| c.name == atom(name))
                .map(|(c, _)| c.yields_to_autoloader)
                .unwrap_or_else(|| panic!("{name} should be indexed"))
        };
        for name in ["Checked", "Iface", "Tr", "Never", "InElse"] {
            assert!(yields(name), "{name} should yield to the autoloader");
        }
        for name in ["Plain", "Other", "NoAutoload", "Versioned", "Loaded"] {
            assert!(!yields(name), "{name} should not yield to the autoloader");
        }
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
