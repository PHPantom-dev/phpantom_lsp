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

use crate::php_type::PhpType;
use crate::signature_render::{
    format_params, format_return_type, native_hint_expresses_return_type, native_param_hint,
};
use crate::types::{
    ClassInfo, ClassLikeKind, ConstantInfo, MethodInfo, PhpVersion, PropertyInfo, PropertySource,
    Visibility,
};
use crate::util::short_name;

mod position;

pub(super) use position::is_function_or_const_name_position_at_offset;
pub(crate) use position::{
    class_body_partial_starts_with_dollar, enclosing_class_at_position,
    extract_method_name_partial, indent_for_position, is_after_const_keyword,
    is_after_function_keyword, is_class_body_member_start,
    is_member_declaration_name_position_at_offset, is_property_declaration_name_position,
    line_start_position,
};

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
    let mut collector = OverrideCollector::<MethodInfo>::new(class, partial, class_loader);

    collector.collect_from_parent_chain(class);

    for iface in &class.interfaces {
        if class.kind == ClassLikeKind::Enum {
            let s: &str = iface;
            let stripped = s.strip_prefix('\\').unwrap_or(s);
            if stripped == "BackedEnum" || stripped == "UnitEnum" {
                continue;
            }
        }
        collector.collect_from_interface(iface);
    }

    collector.from_own_trait = true;
    collector.collect_from_traits(&class.used_traits);

    collector.results
}

/// One member kind's view of the classes an override collector walks.
///
/// The walk itself (parent chain, traits, interfaces, de-duplication
/// against the editing class's own members and against members already
/// collected from a nearer ancestor) is the same for methods,
/// properties, and constants.  Only which members a class contributes,
/// which of those can still be redeclared, and what gets recorded
/// differ, so those three are all this trait carries.
trait OverridableMember: Clone + Sized {
    /// What the collector records for one accepted member.
    type Row;

    /// The members of `class` of this kind.
    fn members(class: &ClassInfo) -> &[Arc<Self>];

    fn name(&self) -> &str;

    /// Whether the editing class can redeclare this member.
    ///
    /// `from_own_trait` is `true` while walking a trait the editing
    /// class uses directly, where the trait's copy loses to the class's
    /// own declaration.
    fn is_overridable(&self, from_own_trait: bool) -> bool;

    fn row(&self, declaring: &str, from_own_trait: bool) -> Self::Row;
}

impl OverridableMember for MethodInfo {
    /// The bool (`skip_override_attr`) is `true` for methods that come
    /// from a directly-used trait, where `#[\Override]` would be a
    /// compile error.
    type Row = (MethodInfo, String, bool);

    fn members(class: &ClassInfo) -> &[Arc<Self>] {
        &class.methods
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_overridable(&self, from_own_trait: bool) -> bool {
        // A `final` method inherited through the parent chain cannot be
        // redeclared at all; one reached via a directly-used trait can.
        self.visibility != Visibility::Private
            && !self.name.starts_with("__")
            && !self.is_virtual
            && (!self.is_final || from_own_trait)
    }

    fn row(&self, declaring: &str, from_own_trait: bool) -> Self::Row {
        (self.clone(), declaring.to_string(), from_own_trait)
    }
}

impl OverridableMember for PropertyInfo {
    type Row = (PropertyInfo, String, bool);

    fn members(class: &ClassInfo) -> &[Arc<Self>] {
        &class.properties
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_overridable(&self, _from_own_trait: bool) -> bool {
        self.visibility != Visibility::Private && !self.is_virtual
    }

    fn row(&self, declaring: &str, from_own_trait: bool) -> Self::Row {
        (self.clone(), declaring.to_string(), from_own_trait)
    }
}

impl OverridableMember for ConstantInfo {
    type Row = (ConstantInfo, String);

    fn members(class: &ClassInfo) -> &[Arc<Self>] {
        &class.constants
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_overridable(&self, _from_own_trait: bool) -> bool {
        self.visibility != Visibility::Private && !self.is_enum_case
    }

    fn row(&self, declaring: &str, _from_own_trait: bool) -> Self::Row {
        (self.clone(), declaring.to_string())
    }
}

/// Walks a class's ancestry collecting the members of one kind that the
/// class can still declare itself.
struct OverrideCollector<'a, M: OverridableMember> {
    partial: &'a str,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// Lowercased names the editing class already declares.
    own_names: HashSet<String>,
    /// Lowercased names already collected, so the nearest ancestor's
    /// declaration wins.
    seen: HashSet<String>,
    /// Class-likes already walked, which also breaks inheritance cycles.
    visited: HashSet<String>,
    results: Vec<M::Row>,
    /// Whether trait members count.  Traits could not declare constants
    /// before PHP 8.2.
    walk_traits: bool,
    /// Whether interface members count.  A class may only redeclare an
    /// interface constant from PHP 8.1, and interfaces cannot declare
    /// properties at all.
    walk_interfaces: bool,
    /// Whether the members currently being collected come from a trait
    /// the editing class uses directly.  Recorded on each row, since
    /// `#[\Override]` on such a member is a compile error.
    from_own_trait: bool,
}

impl<'a, M: OverridableMember> OverrideCollector<'a, M> {
    fn new(
        class: &ClassInfo,
        partial: &'a str,
        class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Self {
        Self {
            partial,
            class_loader,
            own_names: M::members(class)
                .iter()
                .map(|m| m.name().to_lowercase())
                .collect(),
            seen: HashSet::new(),
            visited: HashSet::new(),
            results: Vec::new(),
            walk_traits: true,
            walk_interfaces: true,
            from_own_trait: false,
        }
    }

    /// Each parent of `class`, nearest first, with the traits and
    /// interfaces it brings.
    fn collect_from_parent_chain(&mut self, class: &ClassInfo) {
        for (name, parent) in crate::inheritance::ancestors(class, self.class_loader) {
            if !self.visited.insert(name.to_string()) {
                break;
            }
            self.push_from_class(&parent);
            self.collect_from_traits(&parent.used_traits);
            for iface in &parent.interfaces {
                self.collect_from_interface(iface);
            }
        }
    }

    /// `traits` and every trait they use.
    fn collect_from_traits(&mut self, traits: &[crate::atom::Atom]) {
        if !self.walk_traits {
            return;
        }
        for (name, used) in crate::inheritance::used_traits_deep(traits, self.class_loader) {
            if self.visited.insert(name.to_string()) {
                self.push_from_class(&used);
            }
        }
    }

    /// `iface_name` and every interface it extends.
    fn collect_from_interface(&mut self, iface_name: &str) {
        if !self.walk_interfaces || !self.visited.insert(iface_name.to_string()) {
            return;
        }
        let Some(iface) = (self.class_loader)(iface_name) else {
            return;
        };
        self.push_from_class(&iface);
        for (name, parent) in crate::inheritance::supertypes(&iface, self.class_loader) {
            if self.visited.insert(name.to_string()) {
                self.push_from_class(&parent);
            }
        }
    }

    fn push_from_class(&mut self, class: &ClassInfo) {
        let declaring = class.fqn().to_string();
        for member in M::members(class) {
            if !member.is_overridable(self.from_own_trait) {
                continue;
            }
            if !self.partial.is_empty()
                && !starts_with_ignore_ascii_case(member.name(), self.partial)
            {
                continue;
            }
            let lower = member.name().to_lowercase();
            if self.own_names.contains(&lower) || !self.seen.insert(lower) {
                continue;
            }
            self.results
                .push(member.row(&declaring, self.from_own_trait));
        }
    }
}

/// Options for building method, property, and constant override
/// completion items.
pub(crate) struct OverrideCompletionOpts<'a> {
    pub use_map: &'a HashMap<String, String>,
    pub file_namespace: &'a Option<String>,
    pub indent: &'a str,
    pub replace_range: Range,
    pub php_version: PhpVersion,
    pub line_start: Position,
    /// Insert the full declaration (visibility, `static`, the `function`
    /// or `const` keyword, a property's `$` and trailing `;`) rather than
    /// only the member name.  Used at the class-body root where the user
    /// has not typed any modifier yet.
    pub include_declaration: bool,
}

/// The `#[\Override]` insertion edit an override completion adds above
/// its declaration, once the target PHP version supports the attribute
/// for the member kind being overridden — `None` below that version.
///
/// Shared by the method, property, and constant builders, which only
/// differ in which version threshold they gate on.
fn override_attribute_edit(
    php_version: PhpVersion,
    min_version: PhpVersion,
    line_start: Position,
    indent: &str,
) -> Option<TextEdit> {
    (php_version >= min_version).then(|| TextEdit {
        range: Range {
            start: line_start,
            end: line_start,
        },
        new_text: format!("{indent}#[\\Override]\n"),
    })
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
    let override_edit = override_attribute_edit(
        opts.php_version,
        METHOD_OVERRIDE_ATTR_MIN,
        opts.line_start,
        opts.indent,
    );

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
        .template_params()
        .iter()
        .map(|tparam| match method.template_param_bounds().get(tparam) {
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
    let mut collector = OverrideCollector::<PropertyInfo>::new(class, partial, class_loader);
    // Interfaces cannot declare properties.
    collector.walk_interfaces = false;

    collector.collect_from_parent_chain(class);

    collector.from_own_trait = true;
    collector.collect_from_traits(&class.used_traits);

    collector.results
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
    let mut collector = OverrideCollector::<ConstantInfo>::new(class, partial, class_loader);
    collector.walk_traits = php_version >= TRAIT_CONST_MIN;
    collector.walk_interfaces = php_version >= INTERFACE_CONST_OVERRIDE_MIN;

    collector.collect_from_parent_chain(class);

    for iface in &class.interfaces {
        collector.collect_from_interface(iface);
    }
    collector.collect_from_traits(&class.used_traits);

    collector.results
}

/// Build property-name override completions (`$title` already typed `$`).
///
/// Inserts `name = default` when the parent has an initializer so the
/// user can override `protected $attributes = []` style members in one go.
pub(crate) fn build_property_override_completions(
    props: &[(PropertyInfo, String, bool)],
    opts: &OverrideCompletionOpts<'_>,
) -> Vec<CompletionItem> {
    let override_edit = override_attribute_edit(
        opts.php_version,
        PROPERTY_OVERRIDE_ATTR_MIN,
        opts.line_start,
        opts.indent,
    );
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
        items.push(build_name_override_item(
            NameOverrideItem {
                name: &prop.name,
                sigil: "$",
                type_str: type_str.as_deref(),
                default,
                declaring,
                kind: CompletionItemKind::PROPERTY,
                detail_kind: if *skip_override_attr {
                    "trait"
                } else {
                    "override"
                },
                insert,
                // A trait property is not overridden, so `#[\Override]`
                // on the redeclaration would be a compile error.
                override_edit: if *skip_override_attr {
                    None
                } else {
                    override_edit.clone()
                },
            },
            opts.replace_range,
        ));
    }
    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    items
}

/// A name-style override completion: a property or a constant, which
/// differ only in the sigil their label carries, the item kind, and how
/// their declaration is spelled.  Methods are not one of these — their
/// item is a multi-line snippet with a parameter list.
struct NameOverrideItem<'a> {
    /// Member name, without the `$` a property label carries.
    name: &'a str,
    /// `"$"` for a property, empty for a constant.
    sigil: &'a str,
    type_str: Option<&'a str>,
    default: Option<&'a str>,
    declaring: &'a str,
    kind: CompletionItemKind,
    /// The word before the declaring class on the item's detail line.
    detail_kind: &'a str,
    /// The text to insert, already spelled for the cursor position.
    insert: String,
    override_edit: Option<TextEdit>,
}

fn build_name_override_item(item: NameOverrideItem<'_>, replace_range: Range) -> CompletionItem {
    let NameOverrideItem {
        name,
        sigil,
        type_str,
        default,
        declaring,
        kind,
        detail_kind,
        insert,
        override_edit,
    } = item;
    let label = match (type_str, default) {
        (Some(t), Some(d)) => format!("{sigil}{name}: {t} = {d}"),
        (Some(t), None) => format!("{sigil}{name}: {t}"),
        (None, Some(d)) => format!("{sigil}{name} = {d}"),
        (None, None) => format!("{sigil}{name}"),
    };
    CompletionItem {
        label,
        kind: Some(kind),
        detail: Some(format!("{detail_kind} · {}", short_name(declaring))),
        filter_text: Some(name.to_string()),
        sort_text: Some(format!("0_{}", name.to_ascii_lowercase())),
        insert_text: Some(insert.clone()),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
            range: replace_range,
            new_text: insert,
        })),
        additional_text_edits: override_edit.map(|e| vec![e]),
        label_details: Some(CompletionItemLabelDetails {
            detail: None,
            description: Some(short_name(declaring).to_string()),
        }),
        ..CompletionItem::default()
    }
}

fn property_default_value(prop: &PropertyInfo) -> Option<&str> {
    let Some(PropertySource::DeclaredDefault { value }) = prop.source.as_ref() else {
        return None;
    };
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}

/// Build constant-name override completions.
///
/// Inserts `NAME = value` when the parent constant has an initializer.
pub(crate) fn build_constant_override_completions(
    constants: &[(ConstantInfo, String)],
    opts: &OverrideCompletionOpts<'_>,
) -> Vec<CompletionItem> {
    let override_edit = override_attribute_edit(
        opts.php_version,
        CONSTANT_OVERRIDE_ATTR_MIN,
        opts.line_start,
        opts.indent,
    );
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
        items.push(build_name_override_item(
            NameOverrideItem {
                name: &c.name,
                sigil: "",
                type_str: type_str.as_deref(),
                default,
                declaring,
                kind: CompletionItemKind::CONSTANT,
                detail_kind: "override",
                insert,
                override_edit: override_edit.clone(),
            },
            opts.replace_range,
        ));
    }
    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    items
}

fn shorten_type_display(
    ty: &PhpType,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    ty.resolve_names(&|name| crate::names::shorten_from_fqn(name, use_map, file_namespace))
        .to_string()
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
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
}
