pub(crate) mod backing_class;
pub(crate) mod balance;
pub(crate) mod block_index;
pub(crate) mod blocks;
pub(crate) mod call_site_inference;
pub(crate) mod component_names;
pub(crate) mod component_tags;
pub(crate) mod contract;
pub mod directive_completion;
pub mod directives;
pub(crate) mod discovery;
pub(crate) mod echo_delimiter;
pub(crate) mod implicit_props;
pub(crate) mod layout;
pub(crate) mod outline;
pub(crate) mod pairing;
pub mod preprocessor;
pub(crate) mod shared_vars;
pub(crate) mod signature;
pub mod source_map;
pub(crate) mod typed_receiver;
pub(crate) mod use_directive;
pub(crate) mod view_call_walker;
pub(crate) mod view_paths;

pub use view_paths::discover_view_paths;

use std::sync::LazyLock;

/// Number of lines the Blade preprocessor injects as a prologue
/// (<?php header, $errors declaration, $__env declaration, wrapper function, etc.).
pub const PROLOGUE_LINES: u32 = 6;

/// Name of the function the preprocessor wraps a template's body in, so
/// that collectors which only analyse function bodies see the template as
/// analysable code.
pub const WRAPPER_FUNCTION: &str = "__blade_template";

/// The marker functions the lowering calls to stand in for the directives
/// it cannot express as PHP, each with the return type its call sites
/// need: a directive that compiles into a condition needs a `bool`, the
/// rest are called as statements or as argument wrappers.
const MARKER_FUNCTIONS: &[(&str, Option<&str>)] = &[
    ("blade_directive", None),
    ("blade_bound_attr_directive", None),
    ("blade_view_directive", None),
    ("blade_each_directive", None),
    ("blade_can_directive", Some("bool")),
    ("blade_section_directive", Some("bool")),
    ("blade_stack_directive", Some("bool")),
    ("blade_push_if_directive", None),
    ("blade_custom_directive", Some("bool")),
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

/// Whether `name` is a function the lowering declared for itself rather
/// than one the template wrote: the wrapper holding the template body, or
/// one of the marker functions its directives compile to.
///
/// They have to resolve, or every marker call the lowering emits reads as
/// a call to a function that does not exist, but they are boilerplate no
/// file wrote: nothing should offer them as a symbol of the project.
pub fn is_synthetic_function(name: &str) -> bool {
    name == WRAPPER_FUNCTION || MARKER_FUNCTIONS.iter().any(|(marker, _)| *marker == name)
}

/// The variable a component tag binds its instance to, matching the name
/// Blade's own compiled output uses.
///
/// No caller assigns it and a template that renders a component may never
/// read it, so it is exempt from the unused-variable diagnostic the way
/// `$loop` is.
pub const COMPONENT_VAR: &str = "component";

/// What Laravel instantiates for a component tag that names a template
/// with no class of its own.
pub const ANONYMOUS_COMPONENT: &str = "Illuminate\\View\\AnonymousComponent";

/// Prefix of the class the preprocessor wraps a template's body in when
/// the template renders with a component instance bound to `$this`.
const SCOPE_CLASS_PREFIX: &str = "__blade_scope_";

/// The name of the synthesized subclass whose method holds the body of a
/// template rendered with `fqn` bound to `$this`.
///
/// Deriving the name from the bound class keeps two templates backed by
/// different classes from colliding in the project-wide class index; two
/// templates backed by the *same* class do collide, but they synthesize
/// the identical class, so nothing is lost.
pub fn scope_class_name(fqn: &str) -> String {
    format!(
        "{SCOPE_CLASS_PREFIX}{}",
        fqn.trim_matches('\\').replace('\\', "_")
    )
}

/// Whether a class name is one [`scope_class_name`] produced.
pub fn is_scope_class(name: &str) -> bool {
    name.starts_with(SCOPE_CLASS_PREFIX)
}

/// Check whether a URI refers to a Blade template file.
pub fn is_blade_file(uri: &str) -> bool {
    uri.ends_with(".blade.php")
}

/// How Laravel renders a Blade template, which decides what it gets in
/// scope beyond the data its caller passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TemplateKind {
    /// An ordinary view rendered through `view()` or `@include`.
    #[default]
    View,
    /// A component view, which additionally receives `$attributes` and
    /// `$slot`.
    Component,
}

/// Classify a Blade template from its path and source.
///
/// Either signal is conclusive on its own: the template sits in a
/// `components` directory (Laravel's anonymous-component convention, and
/// where a class-based component's default view lives), or it uses a
/// directive only a component can use.  The directive has to be a real one:
/// a `@props` inside a comment, `@verbatim`, or `@php` block is inert to
/// Blade and makes nothing a component.
pub fn template_kind(uri: &str, content: &str) -> TemplateKind {
    if uri.contains("/components/") || signature::declares_component_directive(content) {
        TemplateKind::Component
    } else {
        TemplateKind::View
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_blade_file_by_extension() {
        assert!(is_blade_file("file:///app/views/welcome.blade.php"));
        assert!(!is_blade_file("file:///app/controllers/Home.php"));
    }

    #[test]
    fn test_is_blade_file_by_language_id() {
        let backend = crate::Backend::test_defaults();
        // Not blade by extension
        let uri = "file:///app/views/welcome.php";
        assert!(!backend.is_blade_file(uri));

        // Register via language_id
        backend.blade_uris.write().insert(uri.to_string());
        assert!(backend.is_blade_file(uri));
    }

    #[test]
    fn template_kind_reads_only_real_component_directives() {
        let view = "file:///resources/views/page.blade.php";
        assert_eq!(
            template_kind(view, "@props(['caption'])\n"),
            TemplateKind::Component
        );
        assert_eq!(
            template_kind(view, "@aware(['color'])\n"),
            TemplateKind::Component
        );
        // A dynamic list is still a component; the directive is the signal.
        assert_eq!(
            template_kind(view, "@props($dynamic)\n"),
            TemplateKind::Component
        );
        // Inert to Blade, so it makes nothing a component.
        assert_eq!(
            template_kind(view, "{{-- @props(['caption']) --}}\n"),
            TemplateKind::View
        );
        assert_eq!(
            template_kind(view, "@php\n$x = \"@props(['caption'])\";\n@endphp\n"),
            TemplateKind::View
        );
        // The path convention is conclusive on its own.
        assert_eq!(
            template_kind(
                "file:///resources/views/components/box.blade.php",
                "{{ $slot }}"
            ),
            TemplateKind::Component
        );
    }
}
