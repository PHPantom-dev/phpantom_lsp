use crate::blade::TemplateKind;
use crate::blade::source_map::BladeSourceMap;

/// The variables Blade puts in a component view's scope on top of the data
/// its caller passes: (name without `$`, docblock type, initialiser).
///
/// No caller passes these — Blade injects them when it renders the
/// component — so no signature or `@props` list can be expected to declare
/// them.
const COMPONENT_VARS: [(&str, &str, &str); 3] = [
    (
        "attributes",
        "\\Illuminate\\View\\ComponentAttributeBag",
        "new \\Illuminate\\View\\ComponentAttributeBag()",
    ),
    (
        "slot",
        "\\Illuminate\\View\\ComponentSlot",
        "new \\Illuminate\\View\\ComponentSlot()",
    ),
    ("componentName", "string", "''"),
];

/// A type string that is safe to place inside a one-line `/** @var … */`
/// docblock, or `mixed` when it is not.
///
/// Inferred types are rendered from expressions in caller files, so they
/// can carry arbitrary text: a literal-string type keeps its source form,
/// and PHP allows a real line break inside a quoted string. A line break
/// would add a prologue line the source map has to account for, and a
/// `*/` would close the docblock early and spill the rest into code.
/// Neither is worth reproducing faithfully, so such a type degrades to
/// `mixed` and the variable is still declared.
fn docblock_safe_type(type_string: &str) -> &str {
    let usable = !type_string.trim().is_empty()
        && !type_string.contains(['\n', '\r'])
        && !type_string.contains("*/");
    if usable { type_string } else { "mixed" }
}

/// Whether `name` (without the `$`) is something PHP can bind as a
/// variable.
///
/// A component tag's attributes become the template's variables, but an
/// attribute name is HTML, not PHP: `wire:model.live`, `@click` and
/// `x-on:keydown` are all legal there.  Blade hands the data to
/// `extract()`, which silently skips any key that is not a valid variable
/// name, so those attributes are reachable only through `$attributes`.
/// Declaring one anyway would emit `$wire:model.live = null;` into the
/// prologue and break the whole template with a syntax error.
fn is_php_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || !first.is_ascii())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || !ch.is_ascii())
}

/// Emit the top-level prologue the template body is wrapped in, and
/// return the offset in it where the hoisted `@use` imports are spliced
/// in once the whole template has been scanned.
///
/// See [`super::preprocess_with_vars`] for the priority chain the
/// declarations follow.
pub(super) fn emit(
    content: &str,
    injected_vars: &[(String, String)],
    kind: TemplateKind,
    this_class: Option<&str>,
    virtual_php: &mut String,
    source_map: &mut BladeSourceMap,
) -> usize {
    let signature = crate::blade::signature::extract(content);
    // (name without `$`, the PHP that declares it), in priority order.
    let mut declared: Vec<(String, String)> = Vec::new();
    let mut declare = |name: &str, decl: String| {
        if !is_php_variable_name(name)
            || signature.declares(name)
            || declared.iter().any(|(existing, _)| existing == name)
        {
            return;
        }
        declared.push((name.to_string(), decl));
    };

    // `@props`/`@aware` entries. A default value types its prop directly
    // (the expression is emitted verbatim, so anything the type engine can
    // resolve works); an entry without one is a *required* prop, whose
    // value the caller supplies, so it is declared `mixed` rather than
    // being invented as `null`.
    let entries = crate::blade::signature::extract_props(content)
        .into_iter()
        .chain(crate::blade::signature::extract_aware(content))
        .flatten();
    for entry in entries {
        let decl = match &entry.default {
            Some(default) => format!("${} = {};\n", entry.name, default),
            None => format!(
                "/** @var mixed ${name} */\n${name} = null;\n",
                name = entry.name
            ),
        };
        declare(&entry.name, decl);
    }

    if kind == TemplateKind::Component {
        for &(name, type_name, init) in &COMPONENT_VARS {
            declare(
                name,
                format!("/** @var {type_name} ${name} */\n${name} = {init};\n"),
            );
        }
    }

    for (name, type_string) in injected_vars {
        let type_string = docblock_safe_type(type_string);
        declare(
            name,
            format!("/** @var {type_string} ${name} */\n${name} = null;\n"),
        );
    }

    // ── Prologue ──
    // The marker functions the lowering calls are declared once for the
    // whole project, as a stub (see `blade::with_marker_stubs`), rather
    // than by every template that calls them.
    virtual_php.push_str("<?php\n");
    // Where hoisted `@use` imports are spliced in once the whole
    // template has been scanned: still in the prologue, so they precede
    // every name they import (name resolution runs in source order and
    // an import written after a use of the name does not apply to it).
    let uses_insert_at = virtual_php.len();
    virtual_php.push_str("/** @var \\Illuminate\\Support\\ViewErrorBag $errors */\n");
    virtual_php.push_str("$errors = new \\Illuminate\\Support\\ViewErrorBag();\n");
    virtual_php.push_str("/** @var \\Illuminate\\View\\Factory $__env */\n");
    virtual_php.push_str("$__env = new \\Illuminate\\View\\Factory();\n");
    for (_, decl) in &declared {
        virtual_php.push_str(decl);
    }

    // Wrap the template body in a function so that diagnostic
    // collectors (which only analyse function/method bodies) treat
    // the Blade content as analysable code.  The closing brace is
    // appended after the main loop.  `$errors`/`$__env` (and every
    // declared variable) are assigned in the outer scope above, so
    // pull them in with `global` — otherwise every use of them inside
    // the wrapped function is a false-positive "undefined variable".
    //
    // A template that renders with a component instance bound gets a
    // method of a subclass of that component instead, so `$this` resolves
    // off the component the way it does in any other method body.  The
    // subclass is abstract: it exists only to carry the body, and a
    // concrete one would be reported for every method its parent leaves
    // abstract.
    if let Some(fqn) = this_class {
        virtual_php.push_str("abstract class ");
        virtual_php.push_str(&crate::blade::scope_class_name(fqn));
        virtual_php.push_str(" extends \\");
        virtual_php.push_str(fqn.trim_matches('\\'));
        virtual_php.push_str(" { public ");
    }
    virtual_php.push_str("function ");
    virtual_php.push_str(crate::blade::WRAPPER_FUNCTION);
    virtual_php.push_str("() { global $errors, $__env");
    for (name, _) in &declared {
        virtual_php.push_str(", $");
        virtual_php.push_str(name);
    }
    virtual_php.push_str(";\n");
    // Derive the prologue height from what was actually emitted rather
    // than assuming a line count per injected variable.  Every Blade
    // position is offset by this number, so a type string that carried
    // an unexpected line break would shift the whole file.
    source_map.prologue_lines = virtual_php.matches('\n').count() as u32;

    uses_insert_at
}
