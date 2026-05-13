use std::collections::HashMap;

use crate::hover::shorten_php_type;

/// Member completion item building.
///
/// This module contains the logic for constructing LSP `CompletionItem`s from
/// resolved `ClassInfo`, filtered by the `AccessKind` (arrow, double-colon,
/// or parent double-colon).
///
/// The union-merge pipeline ([`build_union_completion_items`] and
/// [`merge_union_completion_items`]) handles the case where a variable has
/// multiple candidate types (e.g. `User|AdminUser`).  It deduplicates
/// completion items across candidates, partitions them into intersection
/// members (present on all types) and branch-only members, and assigns
/// sort tiers so intersection members appear first.
///
/// Use-statement insertion helpers live in the sibling [`super::use_edit`]
/// module and are re-exported here for backward compatibility.
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use super::resolve::CompletionItemData;
use crate::hover::{MemberKindForOrigin, find_declaring_class};
use crate::types::Visibility;
use crate::types::*;

/// Return a user-friendly class name for display in completion item details.
///
/// Anonymous classes have synthetic names like `__anonymous@156` which are
/// meaningless to the user. This replaces them with `"anonymous class"`.
fn display_class_name(name: &str) -> &str {
    if name.starts_with("__anonymous@") {
        "anonymous class"
    } else {
        name
    }
}

/// Build an LSP snippet string for a callable (function, method, or constructor).
///
/// Required parameters are included as numbered tab stops with their
/// PHP variable name as placeholder text.  Optional and variadic
/// parameters are omitted — they can be filled in via signature help.
///
/// The returned string uses LSP snippet syntax and **must** be paired
/// with `InsertTextFormat::SNIPPET` on the `CompletionItem`.
///
/// # Examples
///
/// | call                                       | result                              |
/// |--------------------------------------------|-------------------------------------|
/// | `("reset", &[])`                           | `"reset()$0"`                       |
/// | `("makeText", &[req($text), opt($long)])`  | `"makeText(${1:\\$text})$0"`        |
/// | `("add", &[req($a), req($b)])`             | `"add(${1:\\$a}, ${2:\\$b})$0"`     |
pub(crate) fn build_callable_snippet(name: &str, params: &[ParameterInfo]) -> String {
    let required: Vec<&ParameterInfo> = params.iter().filter(|p| p.is_required).collect();

    if required.is_empty() {
        format!("{name}()$0")
    } else {
        let placeholders: Vec<String> = required
            .iter()
            .enumerate()
            .map(|(i, p)| {
                // Escape `$` in parameter names so it is treated as a
                // literal character rather than a snippet tab-stop /
                // variable reference.
                let escaped_name = p.name.replace('$', "\\$");
                format!("${{{}:{}}}", i + 1, escaped_name)
            })
            .collect();
        format!("{name}({})$0", placeholders.join(", "))
    }
}

/// Build an LSP snippet string for a PHP attribute constructor.
///
/// Attributes are syntactic sugar for `new ClassName(...)` with the `new`
/// dropped, so they take the same constructor parameters.  Unlike regular
/// `new` snippets, attribute snippets use **named arguments** for every
/// parameter (both required and optional with non-trivial defaults).
/// Named arguments are safe here because both attributes and named
/// arguments were introduced in PHP 8.0, so there is no risk of
/// generating code incompatible with older PHP versions.
///
/// Placeholder values are chosen to be valid PHP literals:
///
/// | Type hint    | Default placeholder |
/// |--------------|---------------------|
/// | `string`     | `'value'`           |
/// | `bool`       | `false`             |
/// | `int`        | `0`                 |
/// | `float`      | `0.0`              |
/// | `array`      | `[]`                |
/// | (other/none) | the `$name` without `$` |
///
/// When a parameter has an explicit default value in source, that value
/// is used as the snippet placeholder instead of the type-based guess.
///
/// # Examples
///
/// | call                                                    | result                                                |
/// |---------------------------------------------------------|-------------------------------------------------------|
/// | `("Override", &[])`                                     | `"Override"`                                          |
/// | `("DataProvider", &[req(string $methodName)])`          | `"DataProvider(${1:'methodName'})$0"`                 |
/// | `("Route", &[req(string $path), opt(array $methods)])` | `"Route(${1:'path'})$0"`                              |
pub(crate) fn build_attribute_snippet(name: &str, params: &[ParameterInfo]) -> String {
    // Collect parameters worth including: all required ones, plus
    // optional ones only when they have no default (rare but possible).
    // Parameters with defaults are omitted — the user can add them via
    // signature help if needed.
    let required: Vec<&ParameterInfo> = params.iter().filter(|p| p.is_required).collect();

    if required.is_empty() {
        // No required constructor params — insert the bare attribute
        // name without parentheses.  `#[Override]` not `#[Override()]`.
        name.to_string()
    } else {
        let placeholders: Vec<String> = required
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let arg_name = p.name.strip_prefix('$').unwrap_or(&*p.name);
                let (prefix, placeholder, suffix) = attribute_placeholder(p);
                format!("{arg_name}: {prefix}${{{}:{}}}{suffix}", i + 1, placeholder)
            })
            .collect();
        format!("{name}({})$0", placeholders.join(", "))
    }
}

/// Choose a sensible placeholder value for an attribute constructor parameter.
///
/// Returns `(prefix, placeholder, suffix)` where `prefix` and `suffix`
/// are literal characters that surround the snippet tab stop.  For
/// string parameters this produces `'${n:methodName}'` so that typing
/// replaces only the inner text while preserving the quotes.
///
/// If the parameter has an explicit default value in source code, that
/// is used directly as the placeholder with no wrapping.  Otherwise,
/// the type hint is inspected to produce a valid PHP literal.
fn attribute_placeholder(param: &ParameterInfo) -> (String, String, String) {
    // Use the explicit default when available.
    if let Some(ref default) = param.default_value {
        return (String::new(), default.clone(), String::new());
    }

    // Infer from the type hint.  Prefer the native hint over the
    // docblock hint, and unwrap nullable wrappers (`?string` or
    // `string|null` both yield `string`) via `PhpType::non_null_type()`
    // instead of manual `?`-prefix stripping on strings.
    let hint = param.native_type_hint.as_ref().or(param.type_hint.as_ref());
    let base_type = match hint {
        Some(t) => match t.non_null_type() {
            Some(inner) => inner,
            None => t.clone(),
        },
        None => {
            // No type hint — use the bare parameter name.
            let name = param.name.strip_prefix('$').unwrap_or(&*param.name);
            return (String::new(), name.to_string(), String::new());
        }
    };

    if base_type.is_string_type() {
        // Use the parameter name (without $) as a descriptive
        // placeholder.  Quotes sit outside the tab stop so typing
        // replaces only the inner text: `'${1:methodName}'`.
        let name = param.name.strip_prefix('$').unwrap_or(&*param.name);
        return ("'".to_string(), name.to_string(), "'".to_string());
    }
    if base_type.is_bool() {
        return (String::new(), "false".to_string(), String::new());
    }
    if base_type.is_int() {
        return (String::new(), "0".to_string(), String::new());
    }
    if base_type.is_float() {
        return (String::new(), "0.0".to_string(), String::new());
    }
    if base_type.is_bare_array() {
        return (String::new(), "[]".to_string(), String::new());
    }
    // Unknown or complex type — use the bare parameter name.
    let name = param.name.strip_prefix('$').unwrap_or(&*param.name);
    (String::new(), name.to_string(), String::new())
}

// Re-export use-statement helpers so existing `use crate::completion::builder::{…}`
// imports continue to work.
pub(crate) use super::use_edit::{analyze_use_block, build_use_edit, use_import_conflicts};

/// PHP magic methods that should not appear in completion results.
/// These are invoked implicitly by the language runtime rather than
/// called directly by user code.
const MAGIC_METHODS: &[&str] = &[
    "__construct",
    "__destruct",
    "__clone",
    "__get",
    "__set",
    "__isset",
    "__unset",
    "__call",
    "__callStatic",
    "__invoke",
    "__toString",
    "__sleep",
    "__wakeup",
    "__serialize",
    "__unserialize",
    "__set_state",
    "__debugInfo",
];

/// Check whether a method name is a PHP magic method that should be
/// excluded from completion results.
fn is_magic_method(name: &str) -> bool {
    MAGIC_METHODS.iter().any(|&m| m.eq_ignore_ascii_case(name))
}

/// Format a parameter list into a display string.
///
/// Example output: `$text, $frogs = ..., &$ref, ...$rest`
///
/// This is the shared core used by both method and function label
/// builders so the formatting stays consistent everywhere.
pub(crate) fn format_param_list(params: &[ParameterInfo]) -> String {
    params
        .iter()
        .map(|p| {
            let name = if p.is_reference {
                format!("&{}", p.name)
            } else if p.is_variadic {
                format!("...{}", p.name)
            } else {
                p.name.to_string()
            };
            if !p.is_required && !p.is_variadic {
                format!("{} = ...", name)
            } else {
                name
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build a label showing a callable name and its parameter names.
///
/// Works for both methods and standalone functions.
///
/// Example: `regularCode($text, $frogs = ...)`
pub(crate) fn build_callable_label(name: &str, params: &[ParameterInfo]) -> String {
    format!("{}({})", name, format_param_list(params))
}

/// Build the label showing the method name and parameter names.
///
/// Thin wrapper around [`build_callable_label`] for backward
/// compatibility with callers that pass a `&MethodInfo`.
pub(crate) fn build_method_label(method: &MethodInfo) -> String {
    build_callable_label(&method.name, &method.parameters)
}

/// Build completion items for a resolved class, filtered by access kind
/// and visibility scope.
///
/// - `Arrow` access: returns only non-static methods and properties.
/// - `DoubleColon` access: returns only static methods, static properties, and constants.
/// - `ParentDoubleColon` access: returns both static and non-static methods,
///   static properties, and constants — but excludes private members.
/// - `Other` access: returns all members.
///
/// Visibility filtering based on `current_class_name` and `is_self_or_ancestor`:
/// - `None` (top-level code): only **public** members are shown.
/// - `Some(name)` where `name == target_class.name`: all members are shown
///   (same-class access, e.g. `$this->`).
/// - `is_self_or_ancestor == true`: **public** and **protected** members
///   are shown (the cursor is inside the target class or a subclass).
/// - Otherwise: only **public** members are shown.
///
/// `is_self_or_ancestor` should be `true` when the cursor is inside the
/// target class itself or inside a class that (transitively) extends the
/// target.  When `true`, `__construct` is offered for `::` access
/// (e.g. `self::__construct()`, `static::__construct()`,
/// `parent::__construct()`, `ClassName::__construct()` from within a
/// subclass).  When `false`, magic methods are suppressed entirely.
pub(crate) fn build_completion_items(
    target_class: &ClassInfo,
    access_kind: AccessKind,
    current_class_name: Option<&str>,
    is_self_or_ancestor: bool,
    uri: &str,
) -> Vec<CompletionItem> {
    // Determine whether we are inside the same class as the target.
    let same_class = current_class_name.is_some_and(|name| name == target_class.name);
    let mut items: Vec<CompletionItem> = Vec::new();

    // Methods — filtered by static / instance, excluding magic methods
    for method in &target_class.methods {
        // `__construct` is meaningful to call explicitly via `::` when
        // inside the same class or a subclass (e.g.
        // `parent::__construct(...)`, `self::__construct()`).
        // Outside of that relationship, magic methods are suppressed.
        let is_constructor = method.name.eq_ignore_ascii_case("__construct");
        if is_magic_method(&method.name) {
            let allow = is_constructor
                && is_self_or_ancestor
                && matches!(
                    access_kind,
                    AccessKind::DoubleColon | AccessKind::ParentDoubleColon
                );
            if !allow {
                continue;
            }
        }

        // Visibility filtering:
        // - private: only visible from within the same class
        // - protected: visible from the same class or a subclass
        //   (we approximate by allowing when inside any class)
        if method.visibility == Visibility::Private && !same_class {
            continue;
        }
        if method.visibility == Visibility::Protected && !same_class && !is_self_or_ancestor {
            continue;
        }

        let include = match access_kind {
            AccessKind::Arrow => !method.is_static,
            // External `ClassName::` shows only static methods, but
            // `__construct` is an exception — it's an instance method
            // that is routinely called via `ClassName::__construct()`
            // from within a subclass.
            AccessKind::DoubleColon => method.is_static || is_constructor,
            // `self::`, `static::`, and `parent::` show both static and
            // non-static methods (PHP allows calling instance methods
            // via `::` from within the class hierarchy).
            AccessKind::ParentDoubleColon => true,
            AccessKind::Other => true,
        };
        if !include {
            continue;
        }

        let label = build_method_label(method);

        // Show the return type inline after the label so the user sees
        // e.g. `getUser($id): User` in the completion popup.
        let return_type = method
            .return_type
            .as_ref()
            .or(method.native_return_type.as_ref())
            .map(shorten_php_type);

        let data = serde_json::to_value(CompletionItemData {
            class_name: target_class.name.to_string(),
            member_name: method.name.to_string(),
            kind: "method".to_string(),
            uri: uri.to_string(),
            extra_class_names: vec![],
        })
        .ok();
        let class_description = Some(display_class_name(&target_class.name).to_string());
        items.push(CompletionItem {
            label,
            label_details: Some(CompletionItemLabelDetails {
                detail: None,
                description: class_description,
            }),
            kind: Some(CompletionItemKind::METHOD),
            detail: return_type,
            insert_text: Some(build_callable_snippet(&method.name, &method.parameters)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            filter_text: Some(method.name.to_string()),
            tags: deprecation_tag(method.deprecation_message.is_some()),
            commit_characters: Some(METHOD_COMMIT_CHARS.iter().map(|s| s.to_string()).collect()),
            data,
            ..CompletionItem::default()
        });
    }

    // Properties — filtered by static / instance
    for property in &target_class.properties {
        if property.visibility == Visibility::Private && !same_class {
            continue;
        }
        if property.visibility == Visibility::Protected && !same_class && !is_self_or_ancestor {
            continue;
        }

        let include = match access_kind {
            AccessKind::Arrow => !property.is_static,
            AccessKind::DoubleColon | AccessKind::ParentDoubleColon => property.is_static,
            AccessKind::Other => true,
        };
        if !include {
            continue;
        }

        // Static properties accessed via `::` need the `$` prefix
        // (e.g. `self::$path`, `ClassName::$path`), while instance
        // properties via `->` use the bare name (e.g. `$this->path`).
        let display_name = if access_kind == AccessKind::DoubleColon
            || access_kind == AccessKind::ParentDoubleColon
        {
            format!("${}", property.name)
        } else {
            property.name.to_string()
        };

        let detail = property.type_hint.as_ref().map(shorten_php_type);

        let data = serde_json::to_value(CompletionItemData {
            class_name: target_class.name.to_string(),
            member_name: property.name.to_string(),
            kind: "property".to_string(),
            uri: uri.to_string(),
            extra_class_names: vec![],
        })
        .ok();
        let class_description = Some(display_class_name(&target_class.name).to_string());
        items.push(CompletionItem {
            label: display_name.clone(),
            label_details: Some(CompletionItemLabelDetails {
                detail: None,
                description: class_description,
            }),
            kind: Some(CompletionItemKind::PROPERTY),
            detail,
            insert_text: Some(display_name.clone()),
            filter_text: Some(display_name),
            tags: deprecation_tag(property.deprecation_message.is_some()),
            data,
            ..CompletionItem::default()
        });
    }

    // Constants — only for `::`, `parent::`, or unqualified access
    if access_kind == AccessKind::DoubleColon
        || access_kind == AccessKind::ParentDoubleColon
        || access_kind == AccessKind::Other
    {
        for constant in &target_class.constants {
            if constant.visibility == Visibility::Private && !same_class {
                continue;
            }
            if constant.visibility == Visibility::Protected && !same_class && !is_self_or_ancestor {
                continue;
            }

            let detail = constant
                .value
                .clone()
                .or_else(|| constant.type_hint.as_ref().map(shorten_php_type));

            let data = serde_json::to_value(CompletionItemData {
                class_name: target_class.name.to_string(),
                member_name: constant.name.to_string(),
                kind: "constant".to_string(),
                uri: uri.to_string(),
                extra_class_names: vec![],
            })
            .ok();
            let class_description = Some(display_class_name(&target_class.name).to_string());
            items.push(CompletionItem {
                label: constant.name.to_string(),
                label_details: Some(CompletionItemLabelDetails {
                    detail: None,
                    description: class_description,
                }),
                kind: Some(CompletionItemKind::CONSTANT),
                detail,
                insert_text: Some(constant.name.to_string()),
                filter_text: Some(constant.name.to_string()),
                tags: deprecation_tag(constant.deprecation_message.is_some()),
                data,
                ..CompletionItem::default()
            });
        }
    }

    // `::class` keyword — returns the fully qualified class name as a string.
    // Available on any class, interface, or enum via `::` access.
    if access_kind == AccessKind::DoubleColon || access_kind == AccessKind::ParentDoubleColon {
        items.push(CompletionItem {
            label: "class".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("class-string".to_string()),
            insert_text: Some("class".to_string()),
            filter_text: Some("class".to_string()),
            ..CompletionItem::default()
        });
    }

    // Sort by member kind (constants → properties → methods) then
    // alphabetically within each kind group.
    items.sort_by(|a, b| {
        let ka = kind_sort_tier(a.kind);
        let kb = kind_sort_tier(b.kind);
        ka.cmp(&kb).then_with(|| {
            a.filter_text
                .as_deref()
                .unwrap_or(&a.label)
                .to_lowercase()
                .cmp(&b.filter_text.as_deref().unwrap_or(&b.label).to_lowercase())
        })
    });

    for (i, item) in items.iter_mut().enumerate() {
        item.sort_text = Some(format!("{:05}", i));
    }

    items
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Characters that auto-accept a method completion item.
///
/// `(` is the natural next character after a method name and lets the
/// user flow directly into typing arguments.
const METHOD_COMMIT_CHARS: &[&str] = &["("];

/// Return the sort tier for a `CompletionItemKind`.
///
/// Lower values sort first.  The order is:
/// 0 — constants and keywords (`::class`)
/// 1 — properties
/// 2 — methods
fn kind_sort_tier(kind: Option<CompletionItemKind>) -> u8 {
    match kind {
        Some(CompletionItemKind::CONSTANT) | Some(CompletionItemKind::KEYWORD) => 0,
        Some(CompletionItemKind::PROPERTY) => 1,
        Some(CompletionItemKind::METHOD) => 2,
        _ => 3,
    }
}

/// Build a `tags` vec with the `DEPRECATED` tag when the member is deprecated.
pub(crate) fn deprecation_tag(is_deprecated: bool) -> Option<Vec<CompletionItemTag>> {
    if is_deprecated {
        Some(vec![CompletionItemTag::DEPRECATED])
    } else {
        None
    }
}

// ─── Union-merge pipeline ───────────────────────────────────────────────────

/// Check whether `target_class` is the same class as, or an ancestor of,
/// the class the cursor is inside.
///
/// Returns `true` when:
/// - `current_class.name == target_class.name` (same class), or
/// - walking the parent chain of `current_class` reaches `target_class`.
///
/// This controls visibility filtering: when `true`, `__construct` is
/// offered via `::` access and protected members are visible.
pub(crate) fn is_ancestor_of(
    current_class: Option<&ClassInfo>,
    target_class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    let Some(cc) = current_class else {
        return false;
    };
    if cc.name == target_class.name {
        return true;
    }
    // Walk the parent chain of the current class to see if the target
    // is an ancestor.
    let mut ancestor_name = cc.parent_class;
    let mut depth = 0u32;
    while let Some(ref name) = ancestor_name {
        depth += 1;
        if depth > 20 {
            break;
        }
        // ClassInfo.name stores the short name (e.g. "BaseService")
        // while parent_class stores the FQN (e.g. "App\\BaseService").
        // Compare against both the full name and the short (last segment)
        // so that cross-file inheritance is detected correctly.
        let short = name.rsplit('\\').next().unwrap_or(name);
        if target_class.name == *name || target_class.name == short {
            return true;
        }
        ancestor_name = class_loader(name).and_then(|ci| ci.parent_class);
    }
    false
}

/// Build completion items from multiple candidate classes (union types),
/// resolving each through full inheritance and deduplicating across them.
///
/// This is the high-level entry point that combines per-candidate item
/// building with union-aware merging.  For each candidate:
/// 1. Resolves the class fully (own + traits + parents + virtual members).
/// 2. Determines whether the cursor is inside the target class or a
///    subclass (for visibility and `__construct` filtering).
/// 3. Builds raw completion items via [`build_completion_items`].
///
/// The collected items are then passed to [`merge_union_completion_items`]
/// for deduplication and sort-tier assignment.
pub(crate) fn build_union_completion_items(
    candidates: &[Arc<ClassInfo>],
    effective_access: AccessKind,
    current_class: Option<&ClassInfo>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: &crate::virtual_members::ResolvedClassCache,
    uri: &str,
) -> Vec<CompletionItem> {
    let current_class_name = current_class.map(|cc| cc.name.as_str());
    let num_candidates = candidates.len();

    // Track how many candidate classes contributed each label so we can
    // distinguish intersection vs branch-only members.
    let mut all_items: Vec<CompletionItem> = Vec::new();
    let mut occurrence_count: HashMap<String, usize> = HashMap::new();

    for target_class in candidates {
        let resolved =
            crate::virtual_members::resolve_class_fully_cached(target_class, class_loader, cache);

        // Scope methods (and @method virtual methods from the model) are
        // injected onto the candidate ClassInfo by `resolve_named_type`
        // after generic substitution.  `resolve_class_fully_cached` uses
        // a cache key without generic args, so a prior cache entry for
        // the same class without generics will lack those injected
        // methods.  Merge back any instance methods from the candidate
        // that are missing from the resolved result so that scopes
        // survive the re-resolution.
        let merged = if target_class.methods.len() > resolved.methods.len() {
            let mut patched = (*resolved).clone();
            for method in target_class.methods.iter() {
                if !patched
                    .methods
                    .iter()
                    .any(|m| m.name == method.name && m.is_static == method.is_static)
                {
                    patched.methods.push(method.clone());
                }
            }
            std::sync::Arc::new(patched)
        } else {
            resolved
        };

        let self_or_ancestor = is_ancestor_of(current_class, target_class, class_loader);

        let items = build_completion_items(
            &merged,
            effective_access,
            current_class_name,
            self_or_ancestor,
            uri,
        );

        for mut item in items {
            apply_declaring_class_label(&mut item, &merged, class_loader);
            if let Some(existing) = all_items
                .iter_mut()
                .find(|existing| existing.label == item.label)
            {
                *occurrence_count.entry(existing.label.clone()).or_insert(1) += 1;
                // Merge the class name into `data.extra_class_names`
                // so that `completionItem/resolve` can build hover
                // content for all union branches, and so that the
                // branch-only label can list contributing classes.
                merge_data_class_names(existing, &item);
            } else {
                occurrence_count.insert(item.label.clone(), 1);
                all_items.push(item);
            }
        }
    }

    merge_union_completion_items(all_items, occurrence_count, num_candidates)
}

fn apply_declaring_class_label(
    item: &mut CompletionItem,
    owner: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    let Some(data_value) = item.data.as_ref() else {
        return;
    };
    let Ok(mut data) = serde_json::from_value::<CompletionItemData>(data_value.clone()) else {
        return;
    };
    let member_kind = match data.kind.as_str() {
        "method" => MemberKindForOrigin::Method,
        "property" => MemberKindForOrigin::Property,
        "constant" => MemberKindForOrigin::Constant,
        _ => return,
    };

    let declaring = find_declaring_class(owner, &data.member_name, &member_kind, class_loader);
    let declaring_name = declaring.name.to_string();
    if declaring_name == data.class_name {
        return;
    }

    data.class_name = declaring_name.clone();
    data.extra_class_names.clear();
    if let Ok(value) = serde_json::to_value(&data) {
        item.data = Some(value);
    }

    let description = Some(display_class_name(&declaring_name).to_string());
    if let Some(ref mut label_details) = item.label_details {
        label_details.description = description;
    } else {
        item.label_details = Some(CompletionItemLabelDetails {
            detail: None,
            description,
        });
    }
}

/// Merge the class name from a new item's `data` into the existing item's
/// `data.extra_class_names` so that `completionItem/resolve` can iterate
/// all union branches when building hover documentation.
fn merge_data_class_names(existing: &mut CompletionItem, new_item: &CompletionItem) {
    let (Some(existing_data), Some(new_data)) = (&existing.data, &new_item.data) else {
        return;
    };
    let (Ok(mut ed), Ok(nd)) = (
        serde_json::from_value::<CompletionItemData>(existing_data.clone()),
        serde_json::from_value::<CompletionItemData>(new_data.clone()),
    ) else {
        return;
    };
    // Skip if the class name is already recorded.
    if ed.class_name == nd.class_name || ed.extra_class_names.contains(&nd.class_name) {
        return;
    }
    ed.extra_class_names.push(nd.class_name.clone());
    if let Ok(v) = serde_json::to_value(&ed) {
        existing.data = Some(v);
    }
}

/// Extract the class name(s) from a `CompletionItem`'s `data` field.
///
/// Returns a pipe-separated string of all class names (primary +
/// extras), e.g. `"Lamp|Faucet"`.  Returns `None` when the data
/// field is absent or cannot be deserialized.
fn class_names_from_data(item: &CompletionItem) -> Option<String> {
    let data_value = item.data.as_ref()?;
    let data: CompletionItemData = serde_json::from_value(data_value.clone()).ok()?;
    let mut names = vec![display_class_name(&data.class_name).to_string()];
    for extra in &data.extra_class_names {
        names.push(display_class_name(extra).to_string());
    }
    Some(names.join("|"))
}

/// Partition and sort completion items by union membership.
///
/// When a variable has a union type (`num_candidates > 1`), members
/// present on **all** candidate types (intersection members) are more
/// likely to be type-safe.  This function:
///
/// 1. Partitions items into intersection and branch-only based on
///    `occurrence_count` vs `num_candidates`.
/// 2. Sorts each partition alphabetically by `filter_text` / `label`.
/// 3. Assigns `sort_text` prefixes (`"0_"` for intersection, `"1_"` for
///    branch-only) so intersection members appear first in the popup.
/// 4. Adds `label_details` to branch-only items showing which class(es)
///    provide them.
///
/// When `num_candidates <= 1`, returns `items` unchanged (the items
/// already have correct `sort_text` from [`build_completion_items`]).
pub(crate) fn merge_union_completion_items(
    items: Vec<CompletionItem>,
    occurrence_count: HashMap<String, usize>,
    num_candidates: usize,
) -> Vec<CompletionItem> {
    if num_candidates <= 1 {
        return items;
    }

    let sort_key = |item: &CompletionItem| -> (u8, String) {
        (
            kind_sort_tier(item.kind),
            item.filter_text
                .as_deref()
                .unwrap_or(&item.label)
                .to_lowercase(),
        )
    };

    let mut intersection: Vec<CompletionItem> = Vec::new();
    let mut branch_only: Vec<CompletionItem> = Vec::new();

    for item in items {
        let count = occurrence_count.get(&item.label).copied().unwrap_or(1);
        if count >= num_candidates {
            intersection.push(item);
        } else {
            branch_only.push(item);
        }
    }

    intersection.sort_by_key(|item| sort_key(item));
    branch_only.sort_by_key(|item| sort_key(item));

    // Assign sort_text: "0_NNNNN" for intersection, "1_NNNNN" for
    // branch-only.
    let mut result = Vec::with_capacity(intersection.len() + branch_only.len());

    for (i, mut item) in intersection.into_iter().enumerate() {
        item.sort_text = Some(format!("0_{:05}", i));
        // Update description to show all contributing class names
        // (the initial description only has the first candidate).
        if let Some(class_names) = class_names_from_data(&item) {
            if let Some(ref mut ld) = item.label_details {
                ld.description = Some(class_names);
            } else {
                item.label_details = Some(CompletionItemLabelDetails {
                    detail: None,
                    description: Some(class_names),
                });
            }
        }
        result.push(item);
    }

    for (i, mut item) in branch_only.into_iter().enumerate() {
        item.sort_text = Some(format!("1_{:05}", i));
        // Add label_details showing the originating class(es) so the
        // user can tell at a glance which branch provides this member.
        // Merge into existing label_details (which may already have a
        // return-type `detail` set by `build_completion_items`).
        if let Some(class_names) = class_names_from_data(&item) {
            if let Some(ref mut ld) = item.label_details {
                ld.description = Some(class_names);
            } else {
                item.label_details = Some(CompletionItemLabelDetails {
                    detail: None,
                    description: Some(class_names),
                });
            }
        }
        result.push(item);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;
    use crate::types::ClassInfo;

    /// Helper to build a minimal `CompletionItem` with a label and
    /// filter_text — the fields that the merge logic inspects.
    fn item(label: &str, class_name: &str) -> CompletionItem {
        let data = serde_json::to_value(CompletionItemData {
            class_name: class_name.to_string(),
            member_name: label.to_string(),
            kind: "method".to_string(),
            uri: String::new(),
            extra_class_names: vec![],
        })
        .ok();
        CompletionItem {
            label: label.to_string(),
            filter_text: Some(label.to_string()),
            data,
            ..CompletionItem::default()
        }
    }

    // ── class_names_from_data ───────────────────────────────────────────

    #[test]
    fn class_names_from_data_single_class() {
        let i = item("foo", "User");
        assert_eq!(class_names_from_data(&i).as_deref(), Some("User"));
    }

    #[test]
    fn class_names_from_data_with_extras() {
        let mut i = item("foo", "User");
        // Simulate merge_data_class_names having added an extra class.
        if let Some(ref data_value) = i.data {
            let mut d: CompletionItemData = serde_json::from_value(data_value.clone()).unwrap();
            d.extra_class_names.push("AdminUser".to_string());
            i.data = serde_json::to_value(&d).ok();
        }
        assert_eq!(class_names_from_data(&i).as_deref(), Some("User|AdminUser"));
    }

    #[test]
    fn class_names_from_data_none_without_data() {
        let i = CompletionItem {
            label: "foo".to_string(),
            ..CompletionItem::default()
        };
        assert!(class_names_from_data(&i).is_none());
    }

    // ── merge_union_completion_items ─────────────────────────────────────

    #[test]
    fn single_candidate_returns_items_unchanged() {
        let items = vec![item("foo", "A"), item("bar", "A")];
        let mut counts = std::collections::HashMap::new();
        counts.insert("foo".to_string(), 1);
        counts.insert("bar".to_string(), 1);

        let result = merge_union_completion_items(items.clone(), counts, 1);
        // With a single candidate, items pass through unchanged.
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].label, items[0].label);
        assert_eq!(result[1].label, items[1].label);
    }

    #[test]
    fn intersection_members_sorted_before_branch_only() {
        // Two candidates: both have "shared", only one has "unique_a",
        // only one has "unique_b".
        let items = vec![
            item("shared", "A"),
            item("unique_a", "A"),
            item("unique_b", "B"),
        ];
        let mut counts = std::collections::HashMap::new();
        counts.insert("shared".to_string(), 2);
        counts.insert("unique_a".to_string(), 1);
        counts.insert("unique_b".to_string(), 1);

        let result = merge_union_completion_items(items, counts, 2);
        assert_eq!(result.len(), 3);

        // Intersection member first (sort_text starts with "0_").
        assert_eq!(result[0].label, "shared");
        assert!(result[0].sort_text.as_deref().unwrap().starts_with("0_"));

        // Branch-only members after (sort_text starts with "1_").
        assert!(result[1].sort_text.as_deref().unwrap().starts_with("1_"));
        assert!(result[2].sort_text.as_deref().unwrap().starts_with("1_"));
    }

    #[test]
    fn branch_only_items_get_label_details() {
        let items = vec![item("only_a", "A")];
        let mut counts = std::collections::HashMap::new();
        counts.insert("only_a".to_string(), 1);

        let result = merge_union_completion_items(items, counts, 2);
        assert_eq!(result.len(), 1);
        let ld = result[0]
            .label_details
            .as_ref()
            .expect("should have label_details");
        assert_eq!(ld.description.as_deref(), Some("A"));
    }

    #[test]
    fn intersection_items_get_class_description() {
        let items = vec![item("shared", "A")];
        let mut counts = std::collections::HashMap::new();
        counts.insert("shared".to_string(), 2);

        let result = merge_union_completion_items(items, counts, 2);
        assert_eq!(result.len(), 1);
        let ld = result[0]
            .label_details
            .as_ref()
            .expect("should have label_details");
        assert_eq!(ld.description.as_deref(), Some("A"));
    }

    #[test]
    fn branch_only_items_sorted_alphabetically() {
        let items = vec![item("zebra", "A"), item("alpha", "A"), item("middle", "A")];
        let mut counts = std::collections::HashMap::new();
        counts.insert("zebra".to_string(), 1);
        counts.insert("alpha".to_string(), 1);
        counts.insert("middle".to_string(), 1);

        let result = merge_union_completion_items(items, counts, 2);
        assert_eq!(result[0].label, "alpha");
        assert_eq!(result[1].label, "middle");
        assert_eq!(result[2].label, "zebra");
    }

    // ── is_ancestor_of ──────────────────────────────────────────────────

    #[test]
    fn same_class_is_ancestor() {
        let cls = ClassInfo {
            name: atom("Foo"),
            ..ClassInfo::default()
        };
        let loader = |_: &str| -> Option<Arc<ClassInfo>> { None };
        assert!(is_ancestor_of(Some(&cls), &cls, &loader));
    }

    #[test]
    fn no_current_class_is_not_ancestor() {
        let target = ClassInfo {
            name: atom("Foo"),
            ..ClassInfo::default()
        };
        let loader = |_: &str| -> Option<Arc<ClassInfo>> { None };
        assert!(!is_ancestor_of(None, &target, &loader));
    }

    #[test]
    fn direct_parent_is_ancestor() {
        let parent = ClassInfo {
            name: atom("Parent"),
            ..ClassInfo::default()
        };
        let child = ClassInfo {
            name: atom("Child"),
            parent_class: Some(atom("Parent")),
            ..ClassInfo::default()
        };
        let loader = |_: &str| -> Option<Arc<ClassInfo>> { None };
        assert!(is_ancestor_of(Some(&child), &parent, &loader));
    }

    #[test]
    fn grandparent_is_ancestor_via_loader() {
        let grandparent = ClassInfo {
            name: atom("GrandParent"),
            ..ClassInfo::default()
        };
        let child = ClassInfo {
            name: atom("Child"),
            parent_class: Some(atom("Parent")),
            ..ClassInfo::default()
        };
        let loader = |name: &str| -> Option<Arc<ClassInfo>> {
            if name == "Parent" {
                Some(Arc::new(ClassInfo {
                    name: atom("Parent"),
                    parent_class: Some(atom("GrandParent")),
                    ..ClassInfo::default()
                }))
            } else {
                None
            }
        };
        assert!(is_ancestor_of(Some(&child), &grandparent, &loader));
    }

    #[test]
    fn unrelated_class_is_not_ancestor() {
        let current = ClassInfo {
            name: atom("Foo"),
            parent_class: Some(atom("Bar")),
            ..ClassInfo::default()
        };
        let target = ClassInfo {
            name: atom("Baz"),
            ..ClassInfo::default()
        };
        let loader = |_: &str| -> Option<Arc<ClassInfo>> { None };
        assert!(!is_ancestor_of(Some(&current), &target, &loader));
    }

    #[test]
    fn fqn_parent_matches_short_name_target() {
        let parent_target = ClassInfo {
            name: atom("BaseService"),
            ..ClassInfo::default()
        };
        let child = ClassInfo {
            name: atom("MyService"),
            parent_class: Some(atom("App\\BaseService")),
            ..ClassInfo::default()
        };
        let loader = |_: &str| -> Option<Arc<ClassInfo>> { None };
        assert!(is_ancestor_of(Some(&child), &parent_target, &loader));
    }

    // ── kind_sort_tier ──────────────────────────────────────────────────

    #[test]
    fn kind_sort_tier_constants_before_properties_before_methods() {
        let constant = kind_sort_tier(Some(CompletionItemKind::CONSTANT));
        let keyword = kind_sort_tier(Some(CompletionItemKind::KEYWORD));
        let property = kind_sort_tier(Some(CompletionItemKind::PROPERTY));
        let method = kind_sort_tier(Some(CompletionItemKind::METHOD));

        assert_eq!(
            constant, keyword,
            "constants and keywords share the same tier"
        );
        assert!(
            constant < property,
            "constants should sort before properties"
        );
        assert!(property < method, "properties should sort before methods");
    }

    #[test]
    fn kind_sort_tier_none_sorts_last() {
        let method = kind_sort_tier(Some(CompletionItemKind::METHOD));
        let none = kind_sort_tier(None);
        assert!(method < none, "None kind should sort after methods");
    }

    // ── kind-based sorting in merge pipeline ────────────────────────────

    fn item_with_kind(label: &str, class_name: &str, kind: CompletionItemKind) -> CompletionItem {
        let data = serde_json::to_value(CompletionItemData {
            class_name: class_name.to_string(),
            member_name: label.to_string(),
            kind: "method".to_string(),
            uri: String::new(),
            extra_class_names: vec![],
        })
        .ok();
        CompletionItem {
            label: label.to_string(),
            filter_text: Some(label.to_string()),
            kind: Some(kind),
            data,
            ..CompletionItem::default()
        }
    }

    #[test]
    fn merge_sorts_by_kind_then_alphabetically() {
        let items = vec![
            item_with_kind("alpha", "A", CompletionItemKind::METHOD),
            item_with_kind("NAME", "A", CompletionItemKind::CONSTANT),
            item_with_kind("color", "A", CompletionItemKind::PROPERTY),
        ];
        let mut counts = std::collections::HashMap::new();
        counts.insert("alpha".to_string(), 2);
        counts.insert("NAME".to_string(), 2);
        counts.insert("color".to_string(), 2);

        let result = merge_union_completion_items(items, counts, 2);
        assert_eq!(result.len(), 3);
        // constant → property → method
        assert_eq!(result[0].label, "NAME");
        assert_eq!(result[1].label, "color");
        assert_eq!(result[2].label, "alpha");
    }

    #[test]
    fn merge_branch_only_sorted_by_kind_then_alphabetically() {
        let items = vec![
            item_with_kind("zebra", "A", CompletionItemKind::METHOD),
            item_with_kind("STATUS", "A", CompletionItemKind::CONSTANT),
            item_with_kind("active", "A", CompletionItemKind::PROPERTY),
            item_with_kind("beta", "A", CompletionItemKind::METHOD),
        ];
        let mut counts = std::collections::HashMap::new();
        counts.insert("zebra".to_string(), 1);
        counts.insert("STATUS".to_string(), 1);
        counts.insert("active".to_string(), 1);
        counts.insert("beta".to_string(), 1);

        let result = merge_union_completion_items(items, counts, 2);
        assert_eq!(result.len(), 4);
        // All branch-only, sorted: constant → property → methods alphabetically
        assert_eq!(result[0].label, "STATUS");
        assert_eq!(result[1].label, "active");
        assert_eq!(result[2].label, "beta");
        assert_eq!(result[3].label, "zebra");
    }

    #[test]
    fn intersection_kind_sorts_before_branch_only_same_kind() {
        // An intersection method should sort before a branch-only constant.
        let items = vec![
            item_with_kind("shared", "A", CompletionItemKind::METHOD),
            item_with_kind("ONLY_A", "A", CompletionItemKind::CONSTANT),
        ];
        let mut counts = std::collections::HashMap::new();
        counts.insert("shared".to_string(), 2);
        counts.insert("ONLY_A".to_string(), 1);

        let result = merge_union_completion_items(items, counts, 2);
        assert_eq!(result.len(), 2);
        // Intersection tier ("0_") always before branch-only ("1_").
        assert!(result[0].sort_text.as_deref().unwrap().starts_with("0_"));
        assert!(result[1].sort_text.as_deref().unwrap().starts_with("1_"));
        assert_eq!(result[0].label, "shared");
        assert_eq!(result[1].label, "ONLY_A");
    }

    // ── deprecation_tag ─────────────────────────────────────────────────

    #[test]
    fn deprecation_tag_returns_tag_when_deprecated() {
        let tags = deprecation_tag(true);
        assert!(tags.is_some());
        assert!(tags.unwrap().contains(&CompletionItemTag::DEPRECATED));
    }

    #[test]
    fn deprecation_tag_returns_none_when_not_deprecated() {
        assert!(deprecation_tag(false).is_none());
    }
}
