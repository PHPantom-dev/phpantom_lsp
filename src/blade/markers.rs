//! The marker functions the Blade lowering calls to stand in for the
//! directives it cannot express as PHP.
//!
//! The lowering emits these names and the readers that recognise a
//! directive by its marker (symbol extraction, the view-call walker, the
//! unused-variable check) match them, so both sides name every marker
//! through the constants here.

use std::sync::LazyLock;

/// The generic marker for a directive whose arguments are only evaluated:
/// `@class`, `@props`, `@json`, `@dump`, and the like.
pub(crate) const DIRECTIVE: &str = "blade_directive";

/// A component tag's bound attribute (`:title="$post->title"`).
///
/// Its calls are counted in document order to pair each attribute with the
/// expression it passes (`super::component_tags`), so no other directive
/// may lower to it.
pub(crate) const BOUND_ATTR: &str = "blade_bound_attr_directive";

/// The `@include` family and `@extends`, which render a template with the
/// scope they are written in.
pub(crate) const VIEW: &str = "blade_view_directive";

/// `@component` and `@componentFirst`, which render their view from the
/// data they are passed alone.
pub(crate) const COMPONENT: &str = "blade_component_directive";

/// `@each`, which renders its partial once per entry of a collection, with
/// only the item and the key in scope.
pub(crate) const EACH: &str = "blade_each_directive";

/// `@can`, `@cannot` and `@canany`, so their ability strings reach the same
/// extraction as the PHP forms.
pub(crate) const CAN: &str = "blade_can_directive";

/// A section-naming directive.
///
/// A marker of its own rather than the generic [`DIRECTIVE`]: symbol
/// extraction recognises a Laravel string key by the callee it is passed
/// to, and `@class(['a' => $b])` shares that generic one.
pub(crate) const SECTION: &str = "blade_section_directive";

/// The same for a stack-naming directive.
pub(crate) const STACK: &str = "blade_stack_directive";

/// The same for `@pushIf`, whose stack name is its *last* argument rather
/// than its first.
pub(crate) const PUSH_IF: &str = "blade_push_if_directive";

/// A custom directive registered through `Blade::directive()` or
/// `Blade::if()`.
///
/// Returning `bool` lets the same marker stand inside the condition a
/// `Blade::if()` family compiles to, which the generic [`DIRECTIVE`] does
/// not.
pub(crate) const CUSTOM: &str = "blade_custom_directive";

/// Every marker, with the return type its call sites need: a directive
/// that compiles into a condition needs a `bool`, the rest are called as
/// statements or as argument wrappers.
const MARKER_FUNCTIONS: &[(&str, Option<&str>)] = &[
    (DIRECTIVE, None),
    (BOUND_ATTR, None),
    (VIEW, None),
    (COMPONENT, None),
    (EACH, None),
    (CAN, Some("bool")),
    (SECTION, Some("bool")),
    (STACK, Some("bool")),
    (PUSH_IF, None),
    (CUSTOM, Some("bool")),
];

/// One declaration of every [`MARKER_FUNCTIONS`] entry, as a stub the
/// whole project shares.
///
/// A template used to carry these declarations in its own prologue, which
/// made each of them a symbol as many times over as the project has
/// templates.  Registering the file once instead keeps the calls the
/// lowering emits resolvable without any template declaring anything.
static MARKER_STUB: LazyLock<String> = LazyLock::new(|| {
    use std::fmt::Write;

    let mut stub = String::from("<?php\n");
    for (name, return_type) in MARKER_FUNCTIONS {
        match return_type {
            Some(ty) => {
                let _ = writeln!(stub, "function {name}(...$args): {ty} {{ return true; }}");
            }
            None => {
                let _ = writeln!(stub, "function {name}(...$args) {{}}");
            }
        }
    }
    stub
});

/// Add the marker stub to a function-stub index under each name it
/// declares, so `find_or_load_function` resolves a marker call the same
/// way it resolves a call to a built-in.
pub(crate) fn with_marker_stubs(
    mut index: crate::ci_map::CiMap<&'static str>,
) -> crate::ci_map::CiMap<&'static str> {
    let stub: &'static str = &MARKER_STUB;
    for (name, _) in MARKER_FUNCTIONS {
        index.insert(*name, stub);
    }
    index
}

/// Whether `name` is one of the markers.
pub(super) fn is_marker(name: &str) -> bool {
    MARKER_FUNCTIONS.iter().any(|(marker, _)| *marker == name)
}
