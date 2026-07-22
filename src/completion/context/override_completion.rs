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

use crate::code_actions::implement_methods::{
    detect_class_indent, format_params, format_return_type,
};
use crate::php_type::PhpType;
use crate::types::{
    ClassInfo, ClassLikeKind, ConstantInfo, MethodInfo, PhpVersion, PropertyInfo, PropertySource,
    Visibility,
};
use crate::util::{find_class_at_offset, position_to_offset, short_name};

const METHOD_OVERRIDE_ATTR_MIN: PhpVersion = PhpVersion::new(8, 3);
const PROPERTY_OVERRIDE_ATTR_MIN: PhpVersion = PhpVersion::new(8, 5);
const CONSTANT_OVERRIDE_ATTR_MIN: PhpVersion = PhpVersion::new(8, 6);

/// Collect public/protected methods from parents and interfaces that the
/// current class can still override or implement.
pub(crate) fn collect_overridable_methods(
    class: &ClassInfo,
    partial: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<(MethodInfo, String)> {
    let mut own_names: HashSet<String> = class
        .methods
        .iter()
        .map(|m| m.name.to_lowercase())
        .collect();

    // Trait methods already on this class count as implemented.
    collect_trait_method_names(&class.used_traits, class_loader, &mut own_names, 0);

    let mut results: Vec<(MethodInfo, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut visited: HashSet<String> = HashSet::new();

    let mut collector = MethodCollector {
        partial,
        class_loader,
        own_names: &own_names,
        seen: &mut seen,
        visited: &mut visited,
        results: &mut results,
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

    results
}

fn collect_trait_method_names(
    traits: &[crate::atom::Atom],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    names: &mut HashSet<String>,
    depth: usize,
) {
    if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
        return;
    }
    for tname in traits {
        let Some(tr) = class_loader(tname) else {
            continue;
        };
        for m in &tr.methods {
            names.insert(m.name.to_lowercase());
        }
        collect_trait_method_names(&tr.used_traits, class_loader, names, depth + 1);
    }
}

struct MethodCollector<'a> {
    partial: &'a str,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    own_names: &'a HashSet<String>,
    seen: &'a mut HashSet<String>,
    visited: &'a mut HashSet<String>,
    results: &'a mut Vec<(MethodInfo, String)>,
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
            if !self.partial.is_empty()
                && !starts_with_ignore_ascii_case(&method.name, self.partial)
            {
                continue;
            }
            let lower = method.name.to_lowercase();
            if self.own_names.contains(&lower) || !self.seen.insert(lower) {
                continue;
            }
            self.results.push(((**method).clone(), declaring.clone()));
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
}

/// Build completion items for overridable methods matching `partial`.
///
/// When `php_version >= 8.3`, each item also inserts `#[\Override]` on the
/// line above the declaration via `additional_text_edits`.
pub(crate) fn build_override_completions(
    methods: &[(MethodInfo, String)],
    opts: &OverrideCompletionOpts<'_>,
) -> Vec<CompletionItem> {
    let add_override = opts.php_version >= METHOD_OVERRIDE_ATTR_MIN;
    // Insert the attribute at the start of the declaration line.  The
    // indent is included here because this edit is at column 0 of the
    // line (absolute), not mid-line like the name snippet.
    let override_edit = if add_override {
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
    for (method, declaring) in methods {
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

        // Brace lines intentionally have no leading indent.  Clients
        // re-indent multi-line snippet continuations relative to the
        // insertion line (`    public function …`), so baking in the
        // member indent here would double it (`        {`).
        let insert_text = format!(
            "{}({}){}\n{{\n    $0\n}}",
            method.name, params_escaped, return_escaped
        );

        let sort_prefix = if method.is_abstract { "0" } else { "1" };
        let sort_text = format!("{sort_prefix}_{}", method.name.to_ascii_lowercase());

        items.push(CompletionItem {
            label: label.clone(),
            kind: Some(CompletionItemKind::METHOD),
            detail: Some(format!("override · {}", short_name(declaring))),
            filter_text: Some(method.name.to_string()),
            sort_text: Some(sort_text),
            insert_text: Some(insert_text.clone()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: opts.replace_range,
                new_text: insert_text,
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

/// Collect public/protected properties from parents that the class can still
/// redeclare.
pub(crate) fn collect_overridable_properties(
    class: &ClassInfo,
    partial: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<(PropertyInfo, String)> {
    let mut own: HashSet<String> = class
        .properties
        .iter()
        .map(|p| p.name.to_lowercase())
        .collect();
    collect_trait_property_names(&class.used_traits, class_loader, &mut own, 0);

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
            results.push((prop.clone(), declaring.clone()));
        }
        let mut collector = PropertyCollector {
            partial,
            class_loader,
            own: &own,
            seen: &mut seen,
            visited: &mut visited,
            results: &mut results,
        };
        collector.collect_from_traits(&parent.used_traits, depth + 1);
        parent_name = parent.parent_class;
        depth += 1;
    }
    results
}

fn collect_trait_property_names(
    traits: &[crate::atom::Atom],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    names: &mut HashSet<String>,
    depth: usize,
) {
    if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
        return;
    }
    for tname in traits {
        let Some(tr) = class_loader(tname) else {
            continue;
        };
        for p in &tr.properties {
            names.insert(p.name.to_lowercase());
        }
        collect_trait_property_names(&tr.used_traits, class_loader, names, depth + 1);
    }
}

struct PropertyCollector<'a> {
    partial: &'a str,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    own: &'a HashSet<String>,
    seen: &'a mut HashSet<String>,
    visited: &'a mut HashSet<String>,
    results: &'a mut Vec<(PropertyInfo, String)>,
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
            self.results.push((prop.clone(), declaring.clone()));
        }
    }
}

/// Collect public/protected constants from parents.
pub(crate) fn collect_overridable_constants(
    class: &ClassInfo,
    partial: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<(ConstantInfo, String)> {
    let own: HashSet<String> = class
        .constants
        .iter()
        .map(|c| c.name.to_lowercase())
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
        for c in &parent.constants {
            if c.visibility == Visibility::Private || c.is_enum_case {
                continue;
            }
            if !partial.is_empty() && !starts_with_ignore_ascii_case(&c.name, partial) {
                continue;
            }
            let lower = c.name.to_lowercase();
            if own.contains(&lower) || !seen.insert(lower) {
                continue;
            }
            results.push((c.clone(), declaring.clone()));
        }
        parent_name = parent.parent_class;
        depth += 1;
    }
    results
}

/// Build property-name override completions (`$title` already typed `$`).
///
/// Inserts `name = default` when the parent has an initializer so the
/// user can override `protected $attributes = []` style members in one go.
pub(crate) fn build_property_override_completions(
    props: &[(PropertyInfo, String)],
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
    for (prop, declaring) in props {
        let type_str = prop
            .native_type_hint
            .as_ref()
            .or(prop.type_hint.as_ref())
            .map(|t| shorten_type_display(t, opts.use_map, opts.file_namespace))
            .filter(|s| !s.is_empty());
        let default = property_default_value(prop);
        let insert = match default {
            Some(d) => format!("{} = {}", prop.name, d),
            None => prop.name.to_string(),
        };
        let label = match (&type_str, default) {
            (Some(t), Some(d)) => format!("${}: {} = {}", prop.name, t, d),
            (Some(t), None) => format!("${}: {}", prop.name, t),
            (None, Some(d)) => format!("${} = {}", prop.name, d),
            (None, None) => format!("${}", prop.name),
        };
        items.push(CompletionItem {
            label,
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(format!("override · {}", short_name(declaring))),
            filter_text: Some(prop.name.to_string()),
            sort_text: Some(format!("0_{}", prop.name.to_ascii_lowercase())),
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
        let insert = match default {
            Some(d) => format!("{} = {}", c.name, d),
            None => c.name.to_string(),
        };
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
    ty.resolve_names(&|name| {
        for (short, fqn) in use_map {
            if fqn.trim_start_matches('\\') == name {
                return short.clone();
            }
        }
        if let Some(ns) = file_namespace {
            let prefix = format!("{ns}\\");
            if let Some(rest) = name.strip_prefix(&prefix)
                && !rest.contains('\\')
            {
                return rest.to_string();
            }
        }
        name.to_string()
    })
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

fn offset_to_position(content: &str, byte_offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in content.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position {
        line,
        character: col,
    }
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

fn has_const_keyword_before_name(bytes: &[u8], pos: usize) -> bool {
    let mut line_start = pos;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    bytes[line_start..pos]
        .windows(b"const".len())
        .enumerate()
        .any(|(idx, window)| {
            if window != b"const" {
                return false;
            }
            let start = line_start + idx;
            let end = start + b"const".len();
            (start == 0 || !is_ident_byte(bytes[start - 1]))
                && (end >= bytes.len() || !is_ident_byte(bytes[end]))
                && !preceded_by_use_keyword_bytes(bytes, start)
        })
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

    if check_keyword_ending_at_bytes(bytes, i, b"fn")
        || check_keyword_ending_at_bytes(bytes, i, b"case")
    {
        return true;
    }
    if check_keyword_ending_at_bytes(bytes, i, b"function") {
        return !preceded_by_use_keyword_bytes(bytes, i - "function".len());
    }
    if check_keyword_ending_at_bytes(bytes, i, b"const") {
        return !preceded_by_use_keyword_bytes(bytes, i - "const".len());
    }
    has_const_keyword_before_name(bytes, i)
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
            ConstantInfo {
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
            },
            ConstantInfo {
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
            },
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
        let consts = collect_overridable_constants(&child, "", &loader);
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
