//! Member override completion in a class body.
//!
//! - Methods: after `function get|` — parent/interface methods with signatures
//! - Properties: after `protected $tit|` — parent public/protected properties
//! - Constants: after `public const FO|` — parent public/protected constants
//!
//! Override snippets include `#[\Override]` according to the PHP versions that
//! support it for each member kind: methods on PHP 8.3+, properties on PHP
//! 8.5+, and constants on PHP 8.6+.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionTextEdit,
    InsertTextFormat, Position, Range, TextEdit,
};

use crate::class_lookup::find_class_at_offset;
use crate::code_actions::implement_methods::{
    detect_class_indent, format_params, format_return_type, native_hint_expresses_return_type,
    native_param_hint,
};
use crate::php_type::PhpType;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::text_scan::{skip_block_comment, skip_line_comment};
use crate::types::{
    ClassInfo, ClassLikeKind, ConstantInfo, MethodInfo, PhpVersion, PropertyInfo, PropertySource,
    Visibility,
};
use crate::util::short_name;

const METHOD_OVERRIDE_ATTR_MIN: PhpVersion = PhpVersion::new(8, 3);
const PROPERTY_OVERRIDE_ATTR_MIN: PhpVersion = PhpVersion::new(8, 5);
const CONSTANT_OVERRIDE_ATTR_MIN: PhpVersion = PhpVersion::new(8, 6);
/// Implementing classes may redeclare an interface constant from PHP 8.1.
const INTERFACE_CONST_OVERRIDE_MIN: PhpVersion = PhpVersion::new(8, 1);
/// Traits may declare constants from PHP 8.2.
const TRAIT_CONST_MIN: PhpVersion = PhpVersion::new(8, 2);

/// Collect public/protected methods from parents, interfaces, and
/// directly-used traits that the current class can still override or
/// implement.
///
/// The returned bool (`skip_override_attr`) is `true` for methods that
/// come from a directly-used trait (where `#[\Override]` would be a
/// compile error) and `false` for parent/interface methods.
pub(crate) fn collect_overridable_methods(
    class: &ClassInfo,
    partial: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<(MethodInfo, String, bool)> {
    let own_names: HashSet<String> = class
        .methods
        .iter()
        .map(|m| m.name.to_lowercase())
        .collect();

    let mut results: Vec<(MethodInfo, String, bool)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut visited: HashSet<String> = HashSet::new();

    let mut collector = MethodCollector {
        partial,
        class_loader,
        own_names: &own_names,
        seen: &mut seen,
        visited: &mut visited,
        results: &mut results,
        from_own_trait: false,
    };

    collector.collect_from_parent_chain(&class.parent_class, 0);

    for iface in &class.interfaces {
        if class.kind == ClassLikeKind::Enum {
            let s: &str = iface;
            let stripped = s.strip_prefix('\\').unwrap_or(s);
            if stripped == "BackedEnum" || stripped == "UnitEnum" {
                continue;
            }
        }
        collector.collect_from_interface(iface, 0);
    }

    collector.from_own_trait = true;
    collector.collect_from_traits(&class.used_traits, 0);

    results
}

struct MethodCollector<'a> {
    partial: &'a str,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    own_names: &'a HashSet<String>,
    seen: &'a mut HashSet<String>,
    visited: &'a mut HashSet<String>,
    results: &'a mut Vec<(MethodInfo, String, bool)>,
    /// Whether the members currently being collected come from a trait the
    /// editing class uses directly.  Such a method is redeclarable even when
    /// `final` (the trait's copy loses to the class's own declaration), and
    /// `#[\Override]` on it is a compile error.
    from_own_trait: bool,
}

impl MethodCollector<'_> {
    fn collect_from_parent_chain(&mut self, parent_name: &Option<crate::atom::Atom>, depth: usize) {
        if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
            return;
        }
        let Some(pname) = parent_name else {
            return;
        };
        if !self.visited.insert(pname.to_string()) {
            return;
        }
        let Some(parent) = (self.class_loader)(pname) else {
            return;
        };

        self.push_from_class(&parent);
        self.collect_from_traits(&parent.used_traits, depth + 1);

        for iface in &parent.interfaces {
            self.collect_from_interface(iface, depth + 1);
        }

        self.collect_from_parent_chain(&parent.parent_class, depth + 1);
    }

    fn collect_from_traits(&mut self, traits: &[crate::atom::Atom], depth: usize) {
        if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
            return;
        }
        for tname in traits {
            if !self.visited.insert(tname.to_string()) {
                continue;
            }
            let Some(tr) = (self.class_loader)(tname) else {
                continue;
            };
            self.push_from_class(&tr);
            self.collect_from_traits(&tr.used_traits, depth + 1);
        }
    }

    fn collect_from_interface(&mut self, iface_name: &str, depth: usize) {
        if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
            return;
        }
        if !self.visited.insert(iface_name.to_string()) {
            return;
        }
        let Some(iface) = (self.class_loader)(iface_name) else {
            return;
        };
        self.push_from_class(&iface);
        for parent_iface in &iface.interfaces {
            self.collect_from_interface(parent_iface, depth + 1);
        }
    }

    fn push_from_class(&mut self, class: &ClassInfo) {
        let declaring = class.fqn().to_string();
        for method in &class.methods {
            if method.visibility == Visibility::Private {
                continue;
            }
            if method.name.starts_with("__") {
                continue;
            }
            if method.is_virtual {
                continue;
            }
            // A `final` method inherited through the parent chain cannot be
            // redeclared at all; one reached via a directly-used trait can.
            if method.is_final && !self.from_own_trait {
                continue;
            }
            if !self.partial.is_empty()
                && !starts_with_ignore_ascii_case(&method.name, self.partial)
            {
                continue;
            }
            let lower = method.name.to_lowercase();
            if self.own_names.contains(&lower) || !self.seen.insert(lower) {
                continue;
            }
            self.results
                .push(((**method).clone(), declaring.clone(), self.from_own_trait));
        }
    }
}

/// Options for building method-override completion items.
pub(crate) struct OverrideCompletionOpts<'a> {
    pub use_map: &'a HashMap<String, String>,
    pub file_namespace: &'a Option<String>,
    pub indent: &'a str,
    pub replace_range: Range,
    pub php_version: PhpVersion,
    pub line_start: Position,
    /// Insert the full declaration (visibility, `static`, keyword) rather
    /// than only the member name.  Used at the class-body root where the
    /// user has not typed any modifier yet.
    pub include_declaration: bool,
}

fn visibility_keyword(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        // Private members are filtered out by the collectors; if one
        // slips through, a private override is redeclared as private.
        Visibility::Private => "private",
    }
}

/// Build completion items for overridable methods matching `partial`.
///
/// When `php_version >= 8.3`, each item also inserts `#[\Override]` on the
/// line above the declaration via `additional_text_edits`.
pub(crate) fn build_override_completions(
    methods: &[(MethodInfo, String, bool)],
    opts: &OverrideCompletionOpts<'_>,
) -> Vec<CompletionItem> {
    let override_edit = if opts.php_version >= METHOD_OVERRIDE_ATTR_MIN {
        Some(TextEdit {
            range: Range {
                start: opts.line_start,
                end: opts.line_start,
            },
            new_text: format!("{}#[\\Override]\n", opts.indent),
        })
    } else {
        None
    };

    let mut items = Vec::new();
    for (method, declaring, skip_override_attr) in methods {
        // PHPDoc is inherited from parent classes and interfaces, but not
        // from traits, so an override of a trait method restates the
        // docblock-only types above the new declaration.
        let doc_edit = if *skip_override_attr {
            trait_override_docblock_edit(method, opts)
        } else {
            override_edit.clone()
        };
        let params = format_params(method, opts.use_map, opts.file_namespace);
        let return_type = format_return_type(method, opts.use_map, opts.file_namespace);
        let label = if return_type.is_empty() {
            format!("{}({})", method.name, params)
        } else {
            format!("{}({}){}", method.name, params, return_type)
        };

        // Escape `$` in the signature so LSP snippet parsing does not
        // treat `$attributes` as a tabstop/variable (which drops the `$`
        // and can eat the name).  Keep a real `$0` for the final cursor.
        let params_escaped = params.replace('$', "\\$");
        let return_escaped = return_type.replace('$', "\\$");

        let declaration = if opts.include_declaration {
            let static_kw = if method.is_static { "static " } else { "" };
            format!(
                "{} {static_kw}function ",
                visibility_keyword(method.visibility)
            )
        } else {
            String::new()
        };

        // Brace lines intentionally have no leading indent.  Clients
        // re-indent multi-line snippet continuations relative to the
        // insertion line (`    public function …`), so baking in the
        // member indent here would double it (`        {`).
        let insert_text = format!(
            "{declaration}{}({}){}\n{{\n    $0\n}}",
            method.name, params_escaped, return_escaped
        );

        let sort_prefix = if method.is_abstract { "0" } else { "1" };
        let detail_kind = if *skip_override_attr {
            "trait"
        } else {
            "override"
        };
        let sort_text = format!("{sort_prefix}_{}", method.name.to_ascii_lowercase());

        items.push(CompletionItem {
            label: label.clone(),
            kind: Some(CompletionItemKind::METHOD),
            detail: Some(format!("{detail_kind} · {}", short_name(declaring))),
            filter_text: Some(method.name.to_string()),
            sort_text: Some(sort_text),
            insert_text: Some(insert_text.clone()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: opts.replace_range,
                new_text: insert_text,
            })),
            additional_text_edits: doc_edit.map(|e| vec![e]),
            label_details: Some(CompletionItemLabelDetails {
                detail: None,
                description: Some(short_name(declaring).to_string()),
            }),
            ..CompletionItem::default()
        });
    }

    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    items
}

/// Build the docblock inserted above a trait-method override, restating
/// the `@param` and `@return` types that only exist in PHPDoc.
///
/// Returns `None` when every type is already fully expressed by the
/// generated signature.
fn trait_override_docblock_edit(
    method: &MethodInfo,
    opts: &OverrideCompletionOpts<'_>,
) -> Option<TextEdit> {
    let mut lines: Vec<String> = Vec::new();

    for param in method.parameters.iter() {
        let Some(hint) = param.type_hint.as_ref() else {
            continue;
        };
        if native_param_hint(param).as_ref() == Some(hint) {
            continue;
        }
        let ty = shorten_type_display(hint, opts.use_map, opts.file_namespace);
        if ty.is_empty() {
            continue;
        }
        let name = param.name.as_str();
        let dollar = if name.starts_with('$') { "" } else { "$" };
        // A variadic's `@param` type describes one element, so the tag
        // needs the `...` back or it reads as the type of the whole
        // collected array.
        let ellipsis = if param.is_variadic { "..." } else { "" };
        lines.push(format!("@param {ty} {ellipsis}{dollar}{name}"));
    }

    if let Some(ret) = method.return_type.as_ref()
        && !native_hint_expresses_return_type(method)
    {
        let ty = shorten_type_display(ret, opts.use_map, opts.file_namespace);
        if !ty.is_empty() {
            lines.push(format!("@return {ty}"));
        }
    }

    if lines.is_empty() {
        return None;
    }

    // The restated types may reference the method's own `@template`
    // params, which are not inherited either — declare them again above
    // the tags that use them.
    let template_lines: Vec<String> = method
        .template_params
        .iter()
        .map(|tparam| match method.template_param_bounds.get(tparam) {
            Some(bound) => {
                let bound = shorten_type_display(bound, opts.use_map, opts.file_namespace);
                format!("@template {tparam} of {bound}")
            }
            None => format!("@template {tparam}"),
        })
        .collect();
    lines.splice(0..0, template_lines);

    let indent = opts.indent;
    let mut text = format!("{indent}/**\n");
    for line in &lines {
        text.push_str(indent);
        text.push_str(" * ");
        text.push_str(line);
        text.push('\n');
    }
    text.push_str(indent);
    text.push_str(" */\n");
    Some(TextEdit {
        range: Range {
            start: opts.line_start,
            end: opts.line_start,
        },
        new_text: text,
    })
}

/// Collect public/protected properties from parents that the class can still
/// redeclare.
pub(crate) fn collect_overridable_properties(
    class: &ClassInfo,
    partial: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<(PropertyInfo, String, bool)> {
    let own: HashSet<String> = class
        .properties
        .iter()
        .map(|p| p.name.to_lowercase())
        .collect();

    let mut results = Vec::new();
    let mut seen = HashSet::new();
    let mut visited = HashSet::new();
    let mut parent_name = class.parent_class;
    let mut depth = 0usize;
    while let Some(ref pname) = parent_name {
        if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
            break;
        }
        if !visited.insert(pname.to_string()) {
            break;
        }
        let Some(parent) = class_loader(pname) else {
            break;
        };
        let declaring = parent.fqn().to_string();
        for prop in &parent.properties {
            if prop.visibility == Visibility::Private || prop.is_virtual {
                continue;
            }
            if !partial.is_empty() && !starts_with_ignore_ascii_case(&prop.name, partial) {
                continue;
            }
            let lower = prop.name.to_lowercase();
            if own.contains(&lower) || !seen.insert(lower) {
                continue;
            }
            results.push(((**prop).clone(), declaring.clone(), false));
        }
        let mut collector = PropertyCollector {
            partial,
            class_loader,
            own: &own,
            seen: &mut seen,
            visited: &mut visited,
            results: &mut results,
            skip_override_attr: false,
        };
        collector.collect_from_traits(&parent.used_traits, depth + 1);
        parent_name = parent.parent_class;
        depth += 1;
    }

    let mut collector = PropertyCollector {
        partial,
        class_loader,
        own: &own,
        seen: &mut seen,
        visited: &mut visited,
        results: &mut results,
        skip_override_attr: true,
    };
    collector.collect_from_traits(&class.used_traits, 0);

    results
}

struct PropertyCollector<'a> {
    partial: &'a str,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    own: &'a HashSet<String>,
    seen: &'a mut HashSet<String>,
    visited: &'a mut HashSet<String>,
    results: &'a mut Vec<(PropertyInfo, String, bool)>,
    skip_override_attr: bool,
}

impl PropertyCollector<'_> {
    fn collect_from_traits(&mut self, traits: &[crate::atom::Atom], depth: usize) {
        if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
            return;
        }
        for tname in traits {
            if !self.visited.insert(tname.to_string()) {
                continue;
            }
            let Some(tr) = (self.class_loader)(tname) else {
                continue;
            };
            self.push_from_trait(&tr);
            self.collect_from_traits(&tr.used_traits, depth + 1);
        }
    }

    fn push_from_trait(&mut self, tr: &ClassInfo) {
        let declaring = tr.fqn().to_string();
        for prop in &tr.properties {
            if prop.visibility == Visibility::Private || prop.is_virtual {
                continue;
            }
            if !self.partial.is_empty() && !starts_with_ignore_ascii_case(&prop.name, self.partial)
            {
                continue;
            }
            let lower = prop.name.to_lowercase();
            if self.own.contains(&lower) || !self.seen.insert(lower) {
                continue;
            }
            self.results
                .push(((**prop).clone(), declaring.clone(), self.skip_override_attr));
        }
    }
}

/// Collect public/protected constants the class can still redeclare, from
/// the parent chain and from interfaces and traits.
///
/// Overriding an interface constant is only legal on PHP 8.1+, and traits
/// could not declare constants before PHP 8.2, so both sources are gated
/// on `php_version`.
pub(crate) fn collect_overridable_constants(
    class: &ClassInfo,
    partial: &str,
    php_version: PhpVersion,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<(ConstantInfo, String)> {
    let own: HashSet<String> = class
        .constants
        .iter()
        .map(|c| c.name.to_lowercase())
        .collect();

    let mut collector = ConstantCollector {
        partial,
        class_loader,
        own: &own,
        seen: HashSet::new(),
        visited: HashSet::new(),
        results: Vec::new(),
    };

    let mut parent_name = class.parent_class;
    let mut depth = 0usize;
    while let Some(ref pname) = parent_name {
        if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
            break;
        }
        if !collector.visited.insert(pname.to_string()) {
            break;
        }
        let Some(parent) = class_loader(pname) else {
            break;
        };
        collector.push_from(&parent);
        if php_version >= INTERFACE_CONST_OVERRIDE_MIN {
            for iface in &parent.interfaces {
                collector.collect_from_interface(iface, depth + 1);
            }
        }
        if php_version >= TRAIT_CONST_MIN {
            collector.collect_from_traits(&parent.used_traits, depth + 1);
        }
        parent_name = parent.parent_class;
        depth += 1;
    }

    if php_version >= INTERFACE_CONST_OVERRIDE_MIN {
        for iface in &class.interfaces {
            collector.collect_from_interface(iface, 0);
        }
    }
    if php_version >= TRAIT_CONST_MIN {
        collector.collect_from_traits(&class.used_traits, 0);
    }

    collector.results
}

struct ConstantCollector<'a> {
    partial: &'a str,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    own: &'a HashSet<String>,
    seen: HashSet<String>,
    visited: HashSet<String>,
    results: Vec<(ConstantInfo, String)>,
}

impl ConstantCollector<'_> {
    fn push_from(&mut self, owner: &ClassInfo) {
        let declaring = owner.fqn().to_string();
        for c in &owner.constants {
            if c.visibility == Visibility::Private || c.is_enum_case {
                continue;
            }
            if !self.partial.is_empty() && !starts_with_ignore_ascii_case(&c.name, self.partial) {
                continue;
            }
            let lower = c.name.to_lowercase();
            if self.own.contains(&lower) || !self.seen.insert(lower) {
                continue;
            }
            self.results.push(((**c).clone(), declaring.clone()));
        }
    }

    fn collect_from_interface(&mut self, iface_name: &str, depth: usize) {
        if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
            return;
        }
        if !self.visited.insert(iface_name.to_string()) {
            return;
        }
        let Some(iface) = (self.class_loader)(iface_name) else {
            return;
        };
        self.push_from(&iface);
        for parent_iface in &iface.interfaces {
            self.collect_from_interface(parent_iface, depth + 1);
        }
    }

    fn collect_from_traits(&mut self, traits: &[crate::atom::Atom], depth: usize) {
        if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
            return;
        }
        for tname in traits {
            if !self.visited.insert(tname.to_string()) {
                continue;
            }
            let Some(tr) = (self.class_loader)(tname) else {
                continue;
            };
            self.push_from(&tr);
            self.collect_from_traits(&tr.used_traits, depth + 1);
        }
    }
}

/// Build property-name override completions (`$title` already typed `$`).
///
/// Inserts `name = default` when the parent has an initializer so the
/// user can override `protected $attributes = []` style members in one go.
pub(crate) fn build_property_override_completions(
    props: &[(PropertyInfo, String, bool)],
    opts: &NameOverrideCompletionOpts<'_>,
) -> Vec<CompletionItem> {
    let override_edit = if opts.php_version >= PROPERTY_OVERRIDE_ATTR_MIN {
        Some(TextEdit {
            range: Range {
                start: opts.line_start,
                end: opts.line_start,
            },
            new_text: format!("{}#[\\Override]\n", opts.indent),
        })
    } else {
        None
    };
    let mut items = Vec::new();
    for (prop, declaring, skip_override_attr) in props {
        let type_str = prop
            .native_type_hint
            .as_ref()
            .or(prop.type_hint.as_ref())
            .map(|t| shorten_type_display(t, opts.use_map, opts.file_namespace))
            .filter(|s| !s.is_empty());
        let default = property_default_value(prop);
        let mut insert = match default {
            Some(d) => format!("{} = {}", prop.name, d),
            None => prop.name.to_string(),
        };
        if opts.include_declaration {
            let static_kw = if prop.is_static { "static " } else { "" };
            // Redeclaring a readonly property as non-readonly is a fatal
            // error, so the modifier has to come along with the declaration.
            let readonly_kw = if prop.is_readonly { "readonly " } else { "" };
            let type_prefix = prop
                .native_type_hint
                .as_ref()
                .map(|t| shorten_type_display(t, opts.use_map, opts.file_namespace))
                .filter(|s| !s.is_empty())
                .map(|t| format!("{t} "))
                .unwrap_or_default();
            insert = format!(
                "{} {readonly_kw}{static_kw}{type_prefix}${insert};",
                visibility_keyword(prop.visibility)
            );
        }
        let label = match (&type_str, default) {
            (Some(t), Some(d)) => format!("${}: {} = {}", prop.name, t, d),
            (Some(t), None) => format!("${}: {}", prop.name, t),
            (None, Some(d)) => format!("${} = {}", prop.name, d),
            (None, None) => format!("${}", prop.name),
        };
        let detail_kind = if *skip_override_attr {
            "trait"
        } else {
            "override"
        };
        items.push(CompletionItem {
            label,
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(format!("{detail_kind} · {}", short_name(declaring))),
            filter_text: Some(prop.name.to_string()),
            sort_text: Some(format!("0_{}", prop.name.to_ascii_lowercase())),
            insert_text: Some(insert.clone()),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: opts.replace_range,
                new_text: insert,
            })),
            additional_text_edits: if *skip_override_attr {
                None
            } else {
                override_edit.clone().map(|e| vec![e])
            },
            label_details: Some(CompletionItemLabelDetails {
                detail: None,
                description: Some(short_name(declaring).to_string()),
            }),
            ..CompletionItem::default()
        });
    }
    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    items
}

fn property_default_value(prop: &PropertyInfo) -> Option<&str> {
    let Some(PropertySource::DeclaredDefault { value }) = prop.source.as_ref() else {
        return None;
    };
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}

pub(crate) struct NameOverrideCompletionOpts<'a> {
    pub use_map: &'a HashMap<String, String>,
    pub file_namespace: &'a Option<String>,
    pub indent: &'a str,
    pub replace_range: Range,
    pub php_version: PhpVersion,
    pub line_start: Position,
    /// Insert the full declaration (visibility, `static`/`const`, `$`,
    /// trailing `;`) rather than only the member name.  Used at the
    /// class-body root where the user has not typed any modifier yet.
    pub include_declaration: bool,
}

/// Build constant-name override completions.
///
/// Inserts `NAME = value` when the parent constant has an initializer.
pub(crate) fn build_constant_override_completions(
    constants: &[(ConstantInfo, String)],
    opts: &NameOverrideCompletionOpts<'_>,
) -> Vec<CompletionItem> {
    let override_edit = if opts.php_version >= CONSTANT_OVERRIDE_ATTR_MIN {
        Some(TextEdit {
            range: Range {
                start: opts.line_start,
                end: opts.line_start,
            },
            new_text: format!("{}#[\\Override]\n", opts.indent),
        })
    } else {
        None
    };
    let mut items = Vec::new();
    for (c, declaring) in constants {
        let type_str = c
            .type_hint
            .as_ref()
            .map(|t| shorten_type_display(t, opts.use_map, opts.file_namespace))
            .filter(|s| !s.is_empty());
        let default = c.value.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let mut insert = match default {
            Some(d) => format!("{} = {}", c.name, d),
            None => c.name.to_string(),
        };
        if opts.include_declaration {
            // `type_hint` holds the native `const int FOO` hint (PHP 8.3+),
            // never an inferred type, so it is safe to re-emit verbatim.
            let type_prefix = type_str
                .as_ref()
                .map(|t| format!("{t} "))
                .unwrap_or_default();
            // A class constant must have a value; when the parent's value
            // is unknown, end at `= ` so the cursor lands where the value
            // goes.
            insert = match default {
                Some(_) => format!(
                    "{} const {type_prefix}{insert};",
                    visibility_keyword(c.visibility)
                ),
                None => format!(
                    "{} const {type_prefix}{insert} = ",
                    visibility_keyword(c.visibility)
                ),
            };
        }
        let label = match (&type_str, default) {
            (Some(t), Some(d)) => format!("{}: {} = {}", c.name, t, d),
            (Some(t), None) => format!("{}: {}", c.name, t),
            (None, Some(d)) => format!("{} = {}", c.name, d),
            (None, None) => c.name.to_string(),
        };
        items.push(CompletionItem {
            label,
            kind: Some(CompletionItemKind::CONSTANT),
            detail: Some(format!("override · {}", short_name(declaring))),
            filter_text: Some(c.name.to_string()),
            sort_text: Some(format!("0_{}", c.name.to_ascii_lowercase())),
            insert_text: Some(insert.clone()),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: opts.replace_range,
                new_text: insert,
            })),
            additional_text_edits: override_edit.clone().map(|e| vec![e]),
            label_details: Some(CompletionItemLabelDetails {
                detail: None,
                description: Some(short_name(declaring).to_string()),
            }),
            ..CompletionItem::default()
        });
    }
    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    items
}

fn shorten_type_display(
    ty: &PhpType,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    ty.resolve_names(&|name| crate::util::shorten_from_fqn(name, use_map, file_namespace))
        .to_string()
}

/// Extract the partial method name and its LSP range at the cursor.
pub(crate) fn extract_method_name_partial(
    content: &str,
    position: Position,
) -> Option<(String, Range)> {
    let offset = position_to_offset(content, position) as usize;
    if offset > content.len() {
        return None;
    }
    let bytes = content.as_bytes();
    let mut start = offset;
    while start > 0 {
        let b = bytes[start - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            start -= 1;
        } else {
            break;
        }
    }
    let partial = content[start..offset].to_string();

    let start_pos = offset_to_position(content, start);
    let end_pos = position;
    Some((
        partial,
        Range {
            start: start_pos,
            end: end_pos,
        },
    ))
}

/// Whether the cursor is after the `function` keyword (not `const`/`case`).
pub(crate) fn is_after_function_keyword(content: &str, position: Position) -> bool {
    after_keyword(content, position, "function")
}

/// Whether the cursor is after the `const` keyword (class constant name).
pub(crate) fn is_after_const_keyword(content: &str, position: Position) -> bool {
    let bytes = content.as_bytes();
    let cursor = (position_to_offset(content, position) as usize).min(bytes.len());
    let mut i = cursor;
    while i > 0 && is_ident_byte(bytes[i - 1]) {
        i -= 1;
    }
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if check_keyword_ending_at_bytes(bytes, i, b"const") {
        return !preceded_by_use_keyword_bytes(bytes, i - "const".len());
    }
    has_const_keyword_before_name(bytes, i)
}

fn after_keyword(content: &str, position: Position, keyword: &str) -> bool {
    let bytes = content.as_bytes();
    let cursor = (position_to_offset(content, position) as usize).min(bytes.len());
    let mut i = cursor;
    while i > 0 && is_ident_byte(bytes[i - 1]) {
        i -= 1;
    }
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    check_keyword_ending_at_bytes(bytes, i, keyword.as_bytes())
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn check_keyword_ending_at_bytes(bytes: &[u8], pos: usize, keyword: &[u8]) -> bool {
    if pos < keyword.len() {
        return false;
    }
    let start = pos - keyword.len();
    if &bytes[start..pos] != keyword {
        return false;
    }
    if start > 0 && is_ident_byte(bytes[start - 1]) {
        return false;
    }
    if pos < bytes.len() && is_ident_byte(bytes[pos]) {
        return false;
    }
    true
}

fn preceded_by_use_keyword_bytes(bytes: &[u8], keyword_start: usize) -> bool {
    let mut before = keyword_start;
    while before > 0 && bytes[before - 1].is_ascii_whitespace() {
        before -= 1;
    }
    check_keyword_ending_at_bytes(bytes, before, b"use")
}

pub(super) fn has_const_keyword_before_name(bytes: &[u8], pos: usize) -> bool {
    let mut line_start = pos;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    // Find the standalone `const` keyword governing this line (skipping
    // `use const` imports).
    let Some(const_end) = bytes[line_start..pos]
        .windows(b"const".len())
        .enumerate()
        .find_map(|(idx, window)| {
            if window != b"const" {
                return None;
            }
            let start = line_start + idx;
            let end = start + b"const".len();
            let is_word = (start == 0 || !is_ident_byte(bytes[start - 1]))
                && (end >= bytes.len() || !is_ident_byte(bytes[end]));
            (is_word && !preceded_by_use_keyword_bytes(bytes, start)).then_some(end)
        })
    else {
        return false;
    };
    // A declarator *name* follows `const` or a top-level `,`; the cursor is
    // in the initializer once a top-level `=` is seen (until the next
    // comma). `const MAX = PHP_INT_M|` is a value position, not a name.
    !in_const_initializer(bytes, const_end, pos)
}

/// Whether the byte range `from..pos` (starting just after a `const`
/// keyword) ends inside a declarator initializer rather than a name slot.
fn in_const_initializer(bytes: &[u8], from: usize, pos: usize) -> bool {
    let mut depth = 0i32;
    let mut in_value = false;
    for &b in &bytes[from..pos] {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => in_value = false,
            b'=' if depth == 0 => in_value = true,
            _ => {}
        }
    }
    in_value
}

/// Whether the cursor sits at the start of a new member declaration in the
/// class-like body whose opening brace is at `body_start`.
///
/// Scans forward from the brace with a PHP-aware lexer so that comments,
/// docblocks, attributes, strings, and heredocs before the cursor are
/// skipped rather than mistaken for code.  A backwards scan cannot do
/// this: `// closes with }` and `/** @var int */` both end on bytes that
/// look like ordinary code from behind.
///
/// The cursor qualifies when, at the class body's own brace depth, the
/// only thing between the last `{`/`}`/`;` boundary and the cursor is
/// skippable trivia plus an optional `$` and identifier characters.
/// Nested braces (method bodies, property hooks, trait-use adaptation
/// blocks) therefore fall out for free, as does any position inside a
/// comment or string literal.
///
/// A leading `$` before the partial is accepted too (`$on|` for a
/// property override); the caller distinguishes it via
/// [`class_body_partial_starts_with_dollar`].
pub(crate) fn is_class_body_member_start(content: &str, body_start: usize, cursor: usize) -> bool {
    let bytes = content.as_bytes();
    let cursor = cursor.min(bytes.len());
    if body_start >= cursor || bytes.get(body_start) != Some(&b'{') {
        return false;
    }

    // Offset of the first significant byte of the member being typed, or
    // `None` when nothing but trivia has followed the last boundary.
    let mut pending_start: Option<usize> = None;
    let mut depth = 0usize;
    let mut i = body_start + 1;

    while i < cursor {
        let b = bytes[i];
        match b {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'#' if bytes.get(i + 1) == Some(&b'[') => {
                // Attributes precede the member they decorate, so they
                // leave the member-start position intact.
                let end = skip_attribute(bytes, i);
                if end > cursor {
                    return false;
                }
                i = end;
                continue;
            }
            b'#' => {
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = skip_block_comment(bytes, i);
                if end > cursor {
                    return false;
                }
                i = end;
                continue;
            }
            b'\'' | b'"' => {
                let end = skip_quoted(bytes, i);
                if end > cursor {
                    return false;
                }
                if pending_start.is_none() {
                    pending_start = Some(i);
                }
                i = end;
                continue;
            }
            b'<' if bytes[i..].starts_with(b"<<<") => {
                let end = skip_heredoc(bytes, i);
                if end > cursor {
                    return false;
                }
                if pending_start.is_none() {
                    pending_start = Some(i);
                }
                i = end;
                continue;
            }
            b'{' => {
                depth += 1;
                pending_start = None;
            }
            b'}' => {
                if depth == 0 {
                    // The body's closing brace: the cursor is past the
                    // end of this class.
                    return false;
                }
                depth -= 1;
                if depth == 0 {
                    pending_start = None;
                }
            }
            b';' if depth == 0 => pending_start = None,
            _ if b.is_ascii_whitespace() => {}
            _ if depth == 0 && pending_start.is_none() => pending_start = Some(i),
            _ => {}
        }
        i += 1;
    }

    if depth != 0 {
        return false;
    }
    let Some(start) = pending_start else {
        return true;
    };
    let mut k = start;
    if bytes[k] == b'$' {
        k += 1;
    }
    bytes[k..cursor].iter().all(|&b| is_ident_byte(b))
}

/// Whether the partial identifier at `cursor` is preceded by `$`.
pub(crate) fn class_body_partial_starts_with_dollar(content: &str, cursor: usize) -> bool {
    let bytes = content.as_bytes();
    let cursor = cursor.min(bytes.len());
    let mut i = cursor;
    while i > 0 && is_ident_byte(bytes[i - 1]) {
        i -= 1;
    }
    i > 0 && bytes[i - 1] == b'$'
}

/// Offset just past the closing `]` of the `#[…]` attribute at `i`,
/// tracking nested brackets and string literals.
fn skip_attribute(bytes: &[u8], i: usize) -> usize {
    let mut k = i + 1;
    let mut brackets = 0usize;
    while k < bytes.len() {
        match bytes[k] {
            b'\'' | b'"' => {
                k = skip_quoted(bytes, k);
                continue;
            }
            b'[' => brackets += 1,
            b']' => {
                brackets -= 1;
                if brackets == 0 {
                    return k + 1;
                }
            }
            _ => {}
        }
        k += 1;
    }
    bytes.len()
}

/// Offset just past the closing quote of the string literal at `i`.
fn skip_quoted(bytes: &[u8], i: usize) -> usize {
    let quote = bytes[i];
    let mut k = i + 1;
    while k < bytes.len() {
        match bytes[k] {
            b'\\' => k += 1,
            b if b == quote => return k + 1,
            _ => {}
        }
        k += 1;
    }
    bytes.len()
}

/// Offset just past the terminator of the heredoc/nowdoc starting at `i`
/// (which points at `<<<`).
fn skip_heredoc(bytes: &[u8], i: usize) -> usize {
    let mut k = i + 3;
    while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
        k += 1;
    }
    let quote = matches!(bytes.get(k), Some(b'\'') | Some(b'"')).then(|| bytes[k]);
    if quote.is_some() {
        k += 1;
    }
    let label_start = k;
    while k < bytes.len() && is_ident_byte(bytes[k]) {
        k += 1;
    }
    let label = &bytes[label_start..k];
    if label.is_empty() {
        return bytes.len();
    }
    // Scan line by line for the closing label, which PHP 7.3+ allows to
    // be indented and followed by any non-identifier byte.
    while k < bytes.len() {
        let Some(n) = bytes[k..].iter().position(|&b| b == b'\n') else {
            return bytes.len();
        };
        k += n + 1;
        let mut line = k;
        while line < bytes.len() && (bytes[line] == b' ' || bytes[line] == b'\t') {
            line += 1;
        }
        if bytes[line..].starts_with(label)
            && !bytes
                .get(line + label.len())
                .is_some_and(|&b| is_ident_byte(b))
        {
            return line + label.len();
        }
    }
    bytes.len()
}

/// Property name after `$` on a property declaration line (not a parameter).
pub(crate) fn is_property_declaration_name_position(content: &str, position: Position) -> bool {
    let bytes = content.as_bytes();
    let cursor = (position_to_offset(content, position) as usize).min(bytes.len());
    is_property_declaration_name_position_at_offset(bytes, cursor)
}

pub(crate) fn is_member_declaration_name_position_at_offset(content: &str, cursor: usize) -> bool {
    let bytes = content.as_bytes();
    let cursor = cursor.min(bytes.len());
    is_function_or_const_name_position_at_offset(bytes, cursor)
        || is_property_declaration_name_position_at_offset(bytes, cursor)
}

fn is_function_or_const_name_position_at_offset(bytes: &[u8], cursor: usize) -> bool {
    let mut i = cursor;
    while i > 0 && is_ident_byte(bytes[i - 1]) {
        i -= 1;
    }

    let after_ident = i;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == after_ident && after_ident != cursor {
        return false;
    }
    if i == after_ident {
        return false;
    }

    if check_keyword_ending_at_bytes(bytes, i, b"fn") {
        return true;
    }
    // `case` names an enum case (a member) but also labels a `switch`
    // branch, where a class/const/enum name completion is wanted.
    if check_keyword_ending_at_bytes(bytes, i, b"case") {
        return case_is_enum_declaration(bytes, i);
    }
    if check_keyword_ending_at_bytes(bytes, i, b"function") {
        return !preceded_by_use_keyword_bytes(bytes, i - "function".len());
    }
    if check_keyword_ending_at_bytes(bytes, i, b"const") {
        return !preceded_by_use_keyword_bytes(bytes, i - "const".len());
    }
    has_const_keyword_before_name(bytes, i)
}

/// Whether the `case` keyword ending at `keyword_end` declares an enum case
/// (directly inside an `enum` body) rather than a `switch` branch label.
/// Scans back to the nearest enclosing unmatched `{` and checks whether it
/// opens an enum.
pub(super) fn case_is_enum_declaration(bytes: &[u8], keyword_end: usize) -> bool {
    let mut depth = 0i32;
    let mut k = keyword_end - "case".len();
    while k > 0 {
        k -= 1;
        match bytes[k] {
            b'}' => depth += 1,
            b'{' => {
                if depth == 0 {
                    return brace_opens_enum(bytes, k);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    false
}

/// Whether the block opened by the `{` at `brace_pos` is an `enum` body,
/// determined from the block header (the text back to the previous
/// statement boundary).
fn brace_opens_enum(bytes: &[u8], brace_pos: usize) -> bool {
    let mut start = brace_pos;
    while start > 0 && !matches!(bytes[start - 1], b';' | b'{' | b'}') {
        start -= 1;
    }
    contains_ascii_word(&bytes[start..brace_pos], b"enum")
}

fn is_property_declaration_name_position_at_offset(bytes: &[u8], cursor: usize) -> bool {
    // Skip partial name.
    let mut i = cursor;
    while i > 0 && is_ident_byte(bytes[i - 1]) {
        i -= 1;
    }
    // Must be immediately after `$`.
    if i == 0 || bytes[i - 1] != b'$' {
        return false;
    }
    let dollar = i - 1;
    // Walk back on the same line looking for declaration context.
    let mut j = dollar;
    while j > 0 && bytes[j - 1] != b'\n' {
        j -= 1;
    }
    // Parameters live after `function` on the same line / signature.
    let line = &bytes[j..dollar];
    if contains_ascii_word(line, b"function") {
        return false;
    }
    // Property declarations have a visibility/static/readonly/var keyword.
    const MARKERS: &[&str] = &[
        "public",
        "protected",
        "private",
        "static",
        "readonly",
        "var",
    ];
    MARKERS
        .iter()
        .any(|m| contains_ascii_word(line, m.as_bytes()))
}

fn contains_ascii_word(bytes: &[u8], word: &[u8]) -> bool {
    if word.is_empty() || bytes.len() < word.len() {
        return false;
    }
    bytes.windows(word.len()).enumerate().any(|(idx, window)| {
        window.eq_ignore_ascii_case(word)
            && (idx == 0 || !is_ident_byte(bytes[idx - 1]))
            && (idx + word.len() == bytes.len() || !is_ident_byte(bytes[idx + word.len()]))
    })
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// Byte offset of the start of the line containing `position`.
pub(crate) fn line_start_position(content: &str, position: Position) -> Position {
    let offset = position_to_offset(content, position) as usize;
    let line_start = content[..offset.min(content.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    offset_to_position(content, line_start)
}

/// Resolve the enclosing class at the cursor, if any.
pub(crate) fn enclosing_class_at_position<'a>(
    classes: &'a [Arc<ClassInfo>],
    content: &str,
    position: Position,
) -> Option<&'a ClassInfo> {
    let offset = position_to_offset(content, position);
    find_class_at_offset(classes, offset)
}

/// Indent string for the current declaration line (member indent).
pub(crate) fn indent_for_position(content: &str, position: Position, class: &ClassInfo) -> String {
    let offset = position_to_offset(content, position) as usize;
    let line_start = content[..offset.min(content.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let line = &content[line_start..offset.min(content.len())];
    let line_indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    if !line_indent.is_empty() {
        return line_indent;
    }
    detect_class_indent(content, class)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;
    use crate::test_fixtures::make_class;
    use crate::types::{ConstantInfo, Visibility};

    #[test]
    fn collects_parent_constants() {
        let mut base = make_class("Base");
        base.constants = vec![
            Arc::new(ConstantInfo {
                name: atom("STATUS_OK"),
                name_offset: 0,
                type_hint: None,
                visibility: Visibility::Public,
                deprecation_message: None,
                deprecated_replacement: None,
                see_refs: Vec::new(),
                description: None,
                is_enum_case: false,
                enum_value: None,
                value: Some("1".into()),
                is_virtual: false,
            }),
            Arc::new(ConstantInfo {
                name: atom("SECRET"),
                name_offset: 0,
                type_hint: None,
                visibility: Visibility::Private,
                deprecation_message: None,
                deprecated_replacement: None,
                see_refs: Vec::new(),
                description: None,
                is_enum_case: false,
                enum_value: None,
                value: None,
                is_virtual: false,
            }),
        ]
        .into();

        let mut child = make_class("Child");
        child.parent_class = Some(atom("Base"));

        let loader = |name: &str| -> Option<Arc<ClassInfo>> {
            if name == "Base" {
                Some(Arc::new(base.clone()))
            } else {
                None
            }
        };
        let consts = collect_overridable_constants(&child, "", PhpVersion::new(8, 4), &loader);
        let names: Vec<_> = consts.iter().map(|(c, _)| c.name.as_str()).collect();
        assert!(names.contains(&"STATUS_OK"), "got {names:?}");
        assert!(!names.contains(&"SECRET"), "got {names:?}");
    }

    #[test]
    fn after_const_keyword_detects_class_const() {
        let src = "<?php\nclass C {\n    public const ST\n}\n";
        assert!(is_after_const_keyword(
            src,
            Position {
                line: 2,
                character: 19
            }
        ));
    }
}
