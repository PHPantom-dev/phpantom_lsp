//! The attributes an anonymous component's template implies by reading them.
//!
//! A small partial rendered as `<x-badge label="new" />` usually reads
//! `$label` straight out of the tag's attributes without a `@props()` line
//! to declare it. The template still declares its own contract, just
//! implicitly: a name it reads and nothing in it defines has to come from
//! the tag, so it is one of the component's attributes.
//!
//! This reads the *callee*'s own body, never its call sites, which is what
//! keeps it consistent with the priority chain in [`super::signature`] —
//! it is another way a template states what it expects, not an inference
//! from what one caller happened to pass.
//!
//! The scan runs on the template's virtual PHP rather than its raw source,
//! so `@php` assignments, `@foreach` bindings, `@props`/`@aware` entries,
//! `@use` imports and Blade's own component variables are all accounted
//! for by the preprocessor and the shared scope collector instead of by a
//! second set of Blade rules that would drift from them.

use std::collections::HashSet;

use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::blade::directives::CustomDirectives;
use crate::parser::with_parsed_program;
use crate::scope_collector::{AccessKind, ScopeMap, collect_function_scope};

use super::TemplateKind;

/// The variable names (without `$`) a template reads but never defines,
/// in the order they are first read.
///
/// A name the template writes anywhere is left out, whether the write
/// precedes the read or not: a template that assigns it has a value of its
/// own for it, so nothing about the tag is missing. So are the variables
/// Blade builds itself (`$slot`, `$attributes`, `$loop`, `$errors`, …),
/// which no attribute names.
pub(crate) fn implicit_props(content: &str, custom_directives: &CustomDirectives) -> Vec<String> {
    // Preprocessed *without* injected variables: a name another caller's
    // tag happened to pass is declared in the prologue of the template's
    // own virtual PHP, and reading that copy would make the answer depend
    // on which callers had been indexed.
    let (virtual_php, _) = super::preprocessor::preprocess_with_vars(
        content,
        &[],
        TemplateKind::Component,
        None,
        None,
        custom_directives,
    );

    with_parsed_program(&virtual_php, "blade_implicit_props", |program, _content| {
        let Some(scope) = wrapper_scope(program.statements.as_slice()) else {
            return Vec::new();
        };
        free_reads(&scope)
    })
}

/// The scope of the function the preprocessor wraps a template's body in.
fn wrapper_scope(statements: &[Statement<'_>]) -> Option<ScopeMap> {
    statements.iter().find_map(|stmt| {
        let Statement::Function(func) = stmt else {
            return None;
        };
        if bytes_to_str(func.name.value) != super::WRAPPER_FUNCTION {
            return None;
        }
        Some(collect_function_scope(
            &func.parameter_list,
            func.body.statements.as_slice(),
            func.body.left_brace.start.offset,
            func.body.right_brace.end.offset,
        ))
    })
}

/// The names read in a scope that nothing in it ever writes.
fn free_reads(scope: &ScopeMap) -> Vec<String> {
    let written: HashSet<&str> = scope
        .accesses
        .iter()
        .filter(|access| access.kind != AccessKind::Read)
        .map(|access| access.name.as_str())
        .collect();

    let mut names: Vec<String> = Vec::new();
    for access in scope.accesses.iter() {
        if access.kind != AccessKind::Read || written.contains(access.name.as_str()) {
            continue;
        }
        let name = access.name.trim_start_matches('$');
        // `$this` is the component instance a Livewire view renders with,
        // and the framework variables are Blade's own; neither is anything
        // a tag can pass.
        if name == "this" || super::contract::is_framework_scope_var(name) {
            continue;
        }
        if names.iter().any(|existing| existing == name) {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_read_is_an_implicit_prop() {
        assert_eq!(
            implicit_props("<h2>{{ $title }}</h2>\n", &CustomDirectives::default()),
            vec!["title"]
        );
    }

    #[test]
    fn a_name_the_template_assigns_itself_is_not_a_prop() {
        let blade = "@php($label = strtoupper($title))\n<b>{{ $label }}</b>\n";
        assert_eq!(
            implicit_props(blade, &CustomDirectives::default()),
            vec!["title"]
        );
    }

    #[test]
    fn a_declared_prop_is_not_reported_as_implicit() {
        let blade = "@props(['headline', 'level' => 'info'])\n<p>{{ $headline }} {{ $level }} {{ $extra }}</p>\n";
        assert_eq!(
            implicit_props(blade, &CustomDirectives::default()),
            vec!["extra"]
        );
    }

    #[test]
    fn an_aware_entry_comes_from_the_parent_component() {
        let blade = "@aware(['theme'])\n<div class=\"{{ $theme }}\">{{ $body }}</div>\n";
        assert_eq!(
            implicit_props(blade, &CustomDirectives::default()),
            vec!["body"]
        );
    }

    #[test]
    fn a_loop_binding_and_its_loop_object_are_not_props() {
        let blade =
            "@foreach ($rows as $row)\n<li>{{ $loop->index }}: {{ $row }}</li>\n@endforeach\n";
        assert_eq!(
            implicit_props(blade, &CustomDirectives::default()),
            vec!["rows"]
        );
    }

    #[test]
    fn blades_own_component_variables_are_not_props() {
        let blade = "<div {{ $attributes }}>{{ $slot }} {{ $componentName }} {{ $errors }}</div>\n";
        assert!(implicit_props(blade, &CustomDirectives::default()).is_empty());
    }

    /// A bound attribute on a nested tag is a read like any other: the
    /// expression it passes down has to come from somewhere.
    #[test]
    fn an_expression_passed_to_a_nested_tag_is_a_read() {
        let blade = "<x-icon :name=\"$icon\" />\n";
        assert_eq!(
            implicit_props(blade, &CustomDirectives::default()),
            vec!["icon"]
        );
    }

    #[test]
    fn reads_are_reported_in_source_order_without_repeats() {
        let blade = "{{ $second }}{{ $first }}{{ $second }}\n";
        assert_eq!(
            implicit_props(blade, &CustomDirectives::default()),
            vec!["second", "first"]
        );
    }
}
