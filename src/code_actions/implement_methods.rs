//! "Implement missing methods" code action.
//!
//! When the cursor is inside a non-abstract class that extends an abstract
//! class or implements an interface but is missing required method
//! implementations, this module offers a code action to generate stubs
//! for all missing methods.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ClassLikeKind, MethodInfo, Visibility};
use crate::util::offset_to_position;

impl Backend {
    /// Collect "Implement missing methods" code actions for the cursor position.
    ///
    /// When the cursor is inside a concrete class that has unimplemented
    /// abstract or interface methods, this produces one code action that
    /// inserts stubs for all missing methods just before the class's
    /// closing brace.
    pub(crate) fn collect_implement_methods_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let ctx = self.file_context(uri);

        // Convert LSP cursor position to byte offset.
        let cursor_offset = crate::util::position_to_offset(content, params.range.start);

        // Find the class the cursor is inside.  Use keyword_offset as the
        // lower bound so the action also triggers when the cursor is on
        // the `class Foo implements Bar` declaration line (before the `{`).
        let current_class = match ctx
            .classes
            .iter()
            .filter(|c| {
                let effective_start = if c.keyword_offset > 0 {
                    c.keyword_offset
                } else {
                    c.start_offset
                };
                cursor_offset >= effective_start && cursor_offset <= c.end_offset
            })
            .min_by_key(|c| c.end_offset - c.start_offset)
        {
            Some(c) => c,
            None => return,
        };

        // Only concrete classes can implement missing methods.
        // Abstract classes, interfaces, traits, and enums are skipped.
        if current_class.kind != ClassLikeKind::Class || current_class.is_abstract {
            return;
        }

        // Resolve the full inheritance hierarchy to collect all abstract
        // and interface methods.
        let class_loader = self.class_loader(&ctx);

        let missing = collect_missing_methods(current_class, &class_loader);

        if missing.is_empty() {
            return;
        }

        // Determine the use_map so we can shorten FQNs in generated stubs.
        let use_map: HashMap<String, String> = ctx.use_map.clone();
        let file_namespace = ctx.namespace.clone();

        // Build the text for all method stubs.
        let stub_text =
            build_method_stubs(&missing, &use_map, &file_namespace, content, current_class);

        // Insert position: just before the closing brace of the class.
        // `end_offset` points one byte past the `}`, so `end_offset - 1`
        // is the `}` itself.  We insert before that.
        let insert_offset = (current_class.end_offset - 1) as usize;
        let insert_pos = offset_to_position(content, insert_offset);

        let title = if missing.len() == 1 {
            format!("Implement `{}`", missing[0].name)
        } else {
            format!("Implement {} missing methods", missing.len())
        };

        let edit = TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: stub_text,
        };

        let doc_uri: Url = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let mut changes = HashMap::new();
        changes.insert(doc_uri, vec![edit]);

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: Some(true),
            disabled: None,
            data: None,
        }));
    }
}

/// Collect abstract/interface methods that the given concrete class has
/// not yet implemented.
///
/// Walks the full inheritance chain (parent classes, interfaces, traits)
/// and returns methods that are abstract and not already defined on the
/// class itself or inherited as concrete from parent classes.
pub(crate) fn collect_missing_methods(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<MethodInfo> {
    // Build the full set of concrete method names already available on
    // this class: own methods + concrete methods from used traits +
    // concrete methods inherited from the parent chain (including
    // their traits).  This ensures that if a trait or parent implements
    // an interface method, the child doesn't re-stub it.
    let mut implemented_names: Vec<String> = class
        .methods
        .iter()
        .map(|m| m.name.to_lowercase())
        .collect();

    collect_concrete_trait_methods_atoms(
        &class.used_traits,
        class_loader,
        &mut implemented_names,
        0,
    );
    collect_concrete_parent_methods_atom(
        &class.parent_class,
        class_loader,
        &mut implemented_names,
        0,
    );

    let mut missing: Vec<MethodInfo> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    // ── Interfaces ──────────────────────────────────────────────────────
    for iface_name in &class.interfaces {
        // Skip implicit BackedEnum/UnitEnum interfaces on enums.
        // PHP provides from(), tryFrom(), and cases() automatically
        // at runtime — they don't need to be declared in the enum body.
        if class.kind == crate::types::ClassLikeKind::Enum {
            let iface_str: &str = iface_name;
            let stripped = iface_str.strip_prefix('\\').unwrap_or(iface_str);
            if stripped == "BackedEnum" || stripped == "UnitEnum" {
                continue;
            }
        }
        collect_from_interface(
            iface_name,
            class_loader,
            &implemented_names,
            &mut missing,
            &mut seen,
            0,
        );
    }

    // ── Parent chain (abstract methods) ─────────────────────────────────
    collect_from_parent_chain_atom(
        &class.parent_class,
        class_loader,
        &implemented_names,
        &mut missing,
        &mut seen,
        0,
    );

    missing
}

/// Walk the parent chain and collect names of concrete (non-abstract)
/// methods into `implemented`.  This lets us know which interface or
/// abstract methods are already satisfied by a parent class.
///
/// Also collects concrete methods from traits used by each parent,
/// since trait methods are effectively part of the class in PHP.
fn collect_concrete_parent_methods_atom(
    parent_name: &Option<crate::atom::Atom>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    implemented: &mut Vec<String>,
    depth: usize,
) {
    if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
        return;
    }

    let parent_name = match parent_name {
        Some(n) => n,
        None => return,
    };

    let parent = match class_loader(parent_name) {
        Some(c) => c,
        None => return,
    };

    for method in &parent.methods {
        if !method.is_abstract {
            let lower = method.name.to_lowercase();
            if !implemented.contains(&lower) {
                implemented.push(lower);
            }
        }
    }

    // Traits used by the parent also provide concrete methods.
    collect_concrete_trait_methods_atoms(&parent.used_traits, class_loader, implemented, depth + 1);

    collect_concrete_parent_methods_atom(
        &parent.parent_class,
        class_loader,
        implemented,
        depth + 1,
    );
}

/// Walk a list of used traits (and their sub-traits and parent classes)
/// and collect names of concrete (non-abstract) methods into
/// `implemented`.  In PHP, trait methods are effectively part of the
/// class that uses them, so they satisfy interface and abstract-method
/// requirements.
fn collect_concrete_trait_methods_atoms(
    trait_names: &[crate::atom::Atom],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    implemented: &mut Vec<String>,
    depth: usize,
) {
    if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
        return;
    }

    for trait_name in trait_names {
        let trait_info = match class_loader(trait_name) {
            Some(c) => c,
            None => continue,
        };

        for method in &trait_info.methods {
            if !method.is_abstract {
                let lower = method.name.to_lowercase();
                if !implemented.contains(&lower) {
                    implemented.push(lower);
                }
            }
        }

        // Traits can use other traits — recurse into sub-traits.
        if !trait_info.used_traits.is_empty() {
            collect_concrete_trait_methods_atoms(
                &trait_info.used_traits,
                class_loader,
                implemented,
                depth + 1,
            );
        }

        // Traits can also extend a parent class (rare but valid in
        // the class model — e.g. stubs may model this).
        collect_concrete_parent_methods_atom(
            &trait_info.parent_class,
            class_loader,
            implemented,
            depth + 1,
        );
    }
}

/// Recursively collect unimplemented methods from an interface and its
/// parent interfaces.
fn collect_from_interface(
    iface_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    own_methods: &[String],
    missing: &mut Vec<MethodInfo>,
    seen: &mut Vec<String>,
    depth: usize,
) {
    if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
        return;
    }

    let iface = match class_loader(iface_name) {
        Some(c) if c.kind == ClassLikeKind::Interface => c,
        _ => return,
    };

    for method in &iface.methods {
        let lower = method.name.to_lowercase();
        if own_methods.contains(&lower) || seen.contains(&lower) {
            continue;
        }
        seen.push(lower);
        missing.push((**method).clone());
    }

    // Recurse into parent interfaces.
    for parent_iface in &iface.interfaces {
        collect_from_interface(
            parent_iface,
            class_loader,
            own_methods,
            missing,
            seen,
            depth + 1,
        );
    }

    // Interfaces can also extend other interfaces via parent_class in
    // some parser representations.
    if let Some(parent) = iface.parent_class {
        collect_from_interface(&parent, class_loader, own_methods, missing, seen, depth + 1);
    }
}

/// Walk the parent class chain and collect abstract methods that need
/// implementation.
fn collect_from_parent_chain_atom(
    parent_name: &Option<crate::atom::Atom>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    own_methods: &[String],
    missing: &mut Vec<MethodInfo>,
    seen: &mut Vec<String>,
    depth: usize,
) {
    if depth > crate::types::MAX_INHERITANCE_DEPTH as usize {
        return;
    }

    let parent_name = match parent_name {
        Some(n) => n,
        None => return,
    };

    let parent = match class_loader(parent_name) {
        Some(c) => c,
        None => return,
    };

    // Collect interfaces from the parent that the child inherits
    // transitively.
    for iface_name in &parent.interfaces {
        collect_from_interface(
            iface_name,
            class_loader,
            own_methods,
            missing,
            seen,
            depth + 1,
        );
    }

    // Collect abstract methods from the parent itself.
    for method in &parent.methods {
        if !method.is_abstract {
            continue;
        }
        let lower = method.name.to_lowercase();
        if own_methods.contains(&lower) || seen.contains(&lower) {
            continue;
        }
        seen.push(lower);
        missing.push((**method).clone());
    }

    // Continue up the chain.
    collect_from_parent_chain_atom(
        &parent.parent_class,
        class_loader,
        own_methods,
        missing,
        seen,
        depth + 1,
    );
}

/// Build the source text for all missing method stubs.
///
/// Each stub includes visibility, static modifier, parameter list with
/// type hints and defaults, return type, and an empty body.
fn build_method_stubs(
    methods: &[MethodInfo],
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    content: &str,
    class: &ClassInfo,
) -> String {
    // Detect the indentation used inside the class body.
    let indent = detect_class_indent(content, class);

    let mut result = String::new();

    for method in methods {
        result.push('\n');

        // Visibility — interface methods become public, abstract methods
        // keep their declared visibility (but never private).
        let vis = match method.visibility {
            Visibility::Private => "public",
            Visibility::Protected => "protected",
            Visibility::Public => "public",
        };

        let static_kw = if method.is_static { "static " } else { "" };

        // Build parameter list.
        let params = format_params(method, use_map, file_namespace);

        // Build return type.
        let return_type = format_return_type(method, use_map, file_namespace);

        // Write the method stub.
        result.push_str(&indent);
        result.push_str(&format!(
            "{} {}function {}({}){}\n",
            vis, static_kw, method.name, params, return_type,
        ));
        result.push_str(&indent);
        result.push_str("{\n");
        result.push_str(&indent);
        result.push_str("}\n");
    }

    result
}

/// Format the parameter list for a method stub.
fn format_params(
    method: &MethodInfo,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    let mut parts = Vec::new();

    for param in &method.parameters {
        let mut s = String::new();

        // Type hint — prefer native type hint (what appears in PHP source)
        // over the docblock-enriched one.
        if let Some(ref hint) = param.native_type_hint {
            let shortened = shorten_php_type_direct(hint, use_map, file_namespace);
            s.push_str(&shortened);
            s.push(' ');
        }

        // Variadic and reference markers.
        if param.is_reference {
            s.push('&');
        }
        if param.is_variadic {
            s.push_str("...");
        }

        s.push_str(&param.name);

        // Default value.
        if let Some(ref default) = param.default_value {
            s.push_str(" = ");
            s.push_str(default);
        }

        parts.push(s);
    }

    parts.join(", ")
}

/// Returns `true` if `ty` can be written as a native PHP return type hint.
///
/// Docblock-only types (generics, array shapes, callables with signatures,
/// conditional types, template variables, etc.) must not be emitted as
/// native hints in generated method stubs.
fn is_valid_native_hint(ty: &PhpType) -> bool {
    match ty {
        // Plain named types and their nullable wrappers are always valid.
        PhpType::Named(_) => true,
        PhpType::Nullable(inner) => is_valid_native_hint(inner),
        // Union types are valid only when every member is valid (PHP 8+
        // union return types like `int|string|null` are legal).
        PhpType::Union(members) => members.iter().all(is_valid_native_hint),
        // Intersection types (`A&B`) are valid PHP 8.1+ hints.
        PhpType::Intersection(members) => members.iter().all(is_valid_native_hint),
        // Everything else is a docblock-only construct and must not be
        // used as a native return type hint.
        _ => false,
    }
}

/// Format the return type hint for a method stub.
fn format_return_type(
    method: &MethodInfo,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    // Prefer native return type (the actual PHP source-level type hint).
    if let Some(ref native) = method.native_return_type {
        let shortened = shorten_php_type_direct(native, use_map, file_namespace);
        if !shortened.is_empty() {
            return format!(": {}", shortened);
        }
    }

    // Fall back to the docblock return type, but only when it is valid PHP
    // syntax. Docblock types may contain generic annotations (e.g.
    // `array<TKey, TValue>`) that are not legal as native return type hints.
    if let Some(ref ret) = method.return_type {
        let shortened = shorten_php_type_direct(ret, use_map, file_namespace);
        if is_valid_native_hint(ret) && !shortened.is_empty() {
            return format!(": {}", shortened);
        }
    }

    String::new()
}

/// Shorten a fully-qualified type name using the file's use-map and
/// namespace so that generated stubs match the file's import style.
///
/// For example, if the file has `use App\Models\User;`, then
/// `App\Models\User` becomes `User`.  If the class is in the same
/// namespace, the namespace prefix is dropped.
#[cfg(test)]
fn shorten_type(
    type_str: &str,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    let parsed = PhpType::parse(type_str);
    parsed
        .resolve_names(&|name| shorten_single_type(name, use_map, file_namespace))
        .to_string()
}

/// Shorten a [`PhpType`] directly, without round-tripping through a string.
fn shorten_php_type_direct(
    ty: &PhpType,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    ty.resolve_names(&|name| shorten_single_type(name, use_map, file_namespace))
        .to_string()
}

/// Shorten a single named type string.
pub(super) fn shorten_single_type(
    type_str: &str,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    // Check the use map: if any imported short name maps to this FQN,
    // use the short name.
    for (short, fqn) in use_map {
        if fqn.trim_start_matches('\\') == type_str {
            return short.clone();
        }
    }

    // If the type is in the same namespace, strip the namespace prefix.
    if let Some(ns) = file_namespace {
        let prefix = format!("{}\\", ns);
        if let Some(rest) = type_str.strip_prefix(&prefix)
            && !rest.contains('\\')
        {
            return rest.to_string();
        }
    }

    // Return as-is.
    type_str.to_string()
}

/// Detect the indentation level used inside a class body.
///
/// Looks at the first method or property in the class to determine the
/// indent string.  Falls back to four spaces.
pub(crate) fn detect_class_indent(content: &str, class: &ClassInfo) -> String {
    // Look at the line where the class opening brace is and use the
    // next non-empty line's indentation as the member indentation.
    let brace_offset = class.start_offset as usize;
    if brace_offset < content.len() {
        // Find the first non-empty line after the opening brace.
        let after_brace = &content[brace_offset..];
        for line in after_brace.lines().skip(1) {
            if line.trim().is_empty() {
                continue;
            }
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            if !indent.is_empty() {
                return indent;
            }
        }
    }

    // Fallback: four spaces.
    "    ".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::php_type::PhpType;
    use crate::types::{ParameterInfo, Visibility};

    // ── shorten_type tests ──────────────────────────────────────────────

    #[test]
    fn shorten_type_with_use_map() {
        let mut use_map = HashMap::new();
        use_map.insert("User".to_string(), "App\\Models\\User".to_string());
        let ns = Some("App\\Http\\Controllers".to_string());

        assert_eq!(shorten_type("App\\Models\\User", &use_map, &ns), "User");
    }

    #[test]
    fn shorten_type_same_namespace() {
        let use_map = HashMap::new();
        let ns = Some("App\\Models".to_string());

        assert_eq!(shorten_type("App\\Models\\User", &use_map, &ns), "User");
    }

    #[test]
    fn shorten_type_nullable() {
        let mut use_map = HashMap::new();
        use_map.insert("User".to_string(), "App\\Models\\User".to_string());
        let ns = None;

        assert_eq!(shorten_type("?App\\Models\\User", &use_map, &ns), "?User");
    }

    #[test]
    fn shorten_type_union() {
        let mut use_map = HashMap::new();
        use_map.insert("User".to_string(), "App\\Models\\User".to_string());
        let ns = None;

        assert_eq!(
            shorten_type("App\\Models\\User|null", &use_map, &ns),
            "User|null"
        );
    }

    #[test]
    fn shorten_scalar_types_unchanged() {
        let use_map = HashMap::new();
        let ns = None;

        assert_eq!(shorten_type("string", &use_map, &ns), "string");
        assert_eq!(shorten_type("int", &use_map, &ns), "int");
        assert_eq!(shorten_type("bool", &use_map, &ns), "bool");
        assert_eq!(shorten_type("void", &use_map, &ns), "void");
    }

    // ── format_params tests ─────────────────────────────────────────────

    #[test]
    fn format_params_basic() {
        let method = MethodInfo {
            parameters: vec![
                ParameterInfo {
                    name: crate::atom::atom("$name"),
                    is_required: true,
                    type_hint: Some(PhpType::parse("string")),
                    native_type_hint: Some(PhpType::parse("string")),
                    description: None,
                    default_value: None,
                    is_variadic: false,
                    is_reference: false,
                    closure_this_type: None,
                },
                ParameterInfo {
                    name: crate::atom::atom("$age"),
                    is_required: false,
                    type_hint: Some(PhpType::parse("int")),
                    native_type_hint: Some(PhpType::parse("int")),
                    description: None,
                    default_value: Some("0".to_string()),
                    is_variadic: false,
                    is_reference: false,
                    closure_this_type: None,
                },
            ],
            ..MethodInfo::virtual_method("test", None)
        };

        let result = format_params(&method, &HashMap::new(), &None);
        assert_eq!(result, "string $name, int $age = 0");
    }

    #[test]
    fn format_params_variadic_and_reference() {
        let method = MethodInfo {
            parameters: vec![
                ParameterInfo {
                    name: crate::atom::atom("$items"),
                    is_required: true,
                    type_hint: Some(PhpType::parse("string")),
                    native_type_hint: Some(PhpType::parse("string")),
                    description: None,
                    default_value: None,
                    is_variadic: true,
                    is_reference: false,
                    closure_this_type: None,
                },
                ParameterInfo {
                    name: crate::atom::atom("$out"),
                    is_required: true,
                    type_hint: Some(PhpType::parse("array")),
                    native_type_hint: Some(PhpType::parse("array")),
                    description: None,
                    default_value: None,
                    is_variadic: false,
                    is_reference: true,
                    closure_this_type: None,
                },
            ],
            ..MethodInfo::virtual_method("test", None)
        };

        let result = format_params(&method, &HashMap::new(), &None);
        assert_eq!(result, "string ...$items, array &$out");
    }

    // ── format_return_type tests ────────────────────────────────────────

    #[test]
    fn format_return_type_with_native() {
        let method = MethodInfo {
            native_return_type: Some(PhpType::parse("string")),
            return_type: Some(PhpType::parse("string")),
            ..MethodInfo::virtual_method("test", Some("string"))
        };

        assert_eq!(
            format_return_type(&method, &HashMap::new(), &None),
            ": string"
        );
    }

    #[test]
    fn format_return_type_void() {
        let method = MethodInfo {
            native_return_type: Some(PhpType::parse("void")),
            ..MethodInfo::virtual_method("test", Some("void"))
        };

        assert_eq!(
            format_return_type(&method, &HashMap::new(), &None),
            ": void"
        );
    }

    #[test]
    fn format_return_type_none() {
        let method = MethodInfo {
            native_return_type: None,
            return_type: None,
            ..MethodInfo::virtual_method("test", None)
        };

        assert_eq!(format_return_type(&method, &HashMap::new(), &None), "");
    }

    // ── detect_class_indent tests ───────────────────────────────────────

    #[test]
    fn detect_indent_from_class_body() {
        let content = "<?php\nclass Foo {\n    public function bar() {}\n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        assert_eq!(detect_class_indent(content, &class), "    ");
    }

    #[test]
    fn detect_indent_tabs() {
        let content = "<?php\nclass Foo {\n\tpublic function bar() {}\n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        assert_eq!(detect_class_indent(content, &class), "\t");
    }

    // ── collect_missing_methods tests ───────────────────────────────────

    #[test]
    fn collects_interface_methods() {
        let interface = ClassInfo {
            kind: ClassLikeKind::Interface,
            name: crate::atom::atom("Renderable"),
            methods: vec![Arc::new(MethodInfo::virtual_method(
                "render",
                Some("string"),
            ))]
            .into(),
            ..Default::default()
        };

        let class = ClassInfo {
            kind: ClassLikeKind::Class,
            name: crate::atom::atom("Page"),
            interfaces: vec![crate::atom::atom("Renderable")],
            methods: Default::default(),
            ..Default::default()
        };

        let loader = |name: &str| -> Option<Arc<ClassInfo>> {
            if name == "Renderable" {
                Some(Arc::new(interface.clone()))
            } else {
                None
            }
        };

        let missing = collect_missing_methods(&class, &loader);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "render");
    }

    #[test]
    fn skips_already_implemented_methods() {
        let interface = ClassInfo {
            kind: ClassLikeKind::Interface,
            name: crate::atom::atom("Renderable"),
            methods: vec![Arc::new(MethodInfo::virtual_method(
                "render",
                Some("string"),
            ))]
            .into(),
            ..Default::default()
        };

        let class = ClassInfo {
            kind: ClassLikeKind::Class,
            name: crate::atom::atom("Page"),
            interfaces: vec![crate::atom::atom("Renderable")],
            methods: vec![Arc::new(MethodInfo::virtual_method(
                "render",
                Some("string"),
            ))]
            .into(),
            ..Default::default()
        };

        let loader = |name: &str| -> Option<Arc<ClassInfo>> {
            if name == "Renderable" {
                Some(Arc::new(interface.clone()))
            } else {
                None
            }
        };

        let missing = collect_missing_methods(&class, &loader);
        assert!(missing.is_empty());
    }

    #[test]
    fn collects_abstract_parent_methods() {
        let parent = ClassInfo {
            kind: ClassLikeKind::Class,
            name: crate::atom::atom("AbstractBase"),
            is_abstract: true,
            methods: vec![
                Arc::new(MethodInfo {
                    is_abstract: true,
                    ..MethodInfo::virtual_method("doWork", None)
                }),
                // Concrete method — should NOT be in missing list.
                Arc::new(MethodInfo::virtual_method("helper", Some("void"))),
            ]
            .into(),
            ..Default::default()
        };

        let class = ClassInfo {
            kind: ClassLikeKind::Class,
            name: crate::atom::atom("ConcreteChild"),
            parent_class: Some(crate::atom::atom("AbstractBase")),
            methods: Default::default(),
            ..Default::default()
        };

        let loader = |name: &str| -> Option<Arc<ClassInfo>> {
            if name == "AbstractBase" {
                Some(Arc::new(parent.clone()))
            } else {
                None
            }
        };

        let missing = collect_missing_methods(&class, &loader);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "doWork");
    }

    #[test]
    fn case_insensitive_method_matching() {
        let interface = ClassInfo {
            kind: ClassLikeKind::Interface,
            name: crate::atom::atom("Renderable"),
            methods: vec![Arc::new(MethodInfo::virtual_method(
                "Render",
                Some("string"),
            ))]
            .into(),
            ..Default::default()
        };

        let class = ClassInfo {
            kind: ClassLikeKind::Class,
            name: crate::atom::atom("Page"),
            interfaces: vec![crate::atom::atom("Renderable")],
            methods: vec![Arc::new(MethodInfo::virtual_method(
                "render",
                Some("string"),
            ))]
            .into(),
            ..Default::default()
        };

        let loader = |name: &str| -> Option<Arc<ClassInfo>> {
            if name == "Renderable" {
                Some(Arc::new(interface.clone()))
            } else {
                None
            }
        };

        let missing = collect_missing_methods(&class, &loader);
        assert!(missing.is_empty(), "PHP method names are case-insensitive");
    }

    #[test]
    fn collects_from_parent_interfaces() {
        let parent = ClassInfo {
            kind: ClassLikeKind::Class,
            name: crate::atom::atom("AbstractBase"),
            is_abstract: true,
            interfaces: vec![crate::atom::atom("Serializable")],
            methods: Default::default(),
            ..Default::default()
        };

        let serializable = ClassInfo {
            kind: ClassLikeKind::Interface,
            name: crate::atom::atom("Serializable"),
            methods: vec![
                Arc::new(MethodInfo::virtual_method("serialize", Some("string"))),
                Arc::new(MethodInfo::virtual_method("unserialize", None)),
            ]
            .into(),
            ..Default::default()
        };

        let class = ClassInfo {
            kind: ClassLikeKind::Class,
            name: crate::atom::atom("ConcreteChild"),
            parent_class: Some(crate::atom::atom("AbstractBase")),
            methods: Default::default(),
            ..Default::default()
        };

        let loader = |name: &str| -> Option<Arc<ClassInfo>> {
            match name {
                "AbstractBase" => Some(Arc::new(parent.clone())),
                "Serializable" => Some(Arc::new(serializable.clone())),
                _ => None,
            }
        };

        let missing = collect_missing_methods(&class, &loader);
        assert_eq!(missing.len(), 2);
        let names: Vec<&str> = missing.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"serialize"));
        assert!(names.contains(&"unserialize"));
    }

    // ── build_method_stubs tests ────────────────────────────────────────

    #[test]
    fn stub_includes_return_type() {
        let methods = vec![MethodInfo {
            native_return_type: Some(PhpType::parse("string")),
            visibility: Visibility::Public,
            ..MethodInfo::virtual_method("render", Some("string"))
        }];

        let content = "<?php\nclass Foo {\n    \n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        let result = build_method_stubs(&methods, &HashMap::new(), &None, content, &class);
        assert!(result.contains("public function render(): string"));
    }

    #[test]
    fn stub_preserves_static_modifier() {
        let methods = vec![MethodInfo {
            is_static: true,
            native_return_type: Some(PhpType::parse("void")),
            visibility: Visibility::Public,
            ..MethodInfo::virtual_method("init", Some("void"))
        }];

        let content = "<?php\nclass Foo {\n    \n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        let result = build_method_stubs(&methods, &HashMap::new(), &None, content, &class);
        assert!(result.contains("public static function init(): void"));
    }

    #[test]
    fn stub_omits_generic_docblock_return_type() {
        // When a method has no native return type but only a @return with
        // generic syntax (array<K,V>), the stub must NOT emit the generic
        // type as a native PHP return type hint because that is a syntax
        // error in PHP.
        let methods = vec![MethodInfo {
            native_return_type: None,
            return_type: Some(PhpType::parse("array<TKey, TValue>")),
            visibility: Visibility::Public,
            ..MethodInfo::virtual_method("toArray", None)
        }];

        let content = "<?php\nclass Foo {\n    \n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        let result = build_method_stubs(&methods, &HashMap::new(), &None, content, &class);
        // No return type hint should be present.
        assert!(
            !result.contains(": array<"),
            "expected no generic return type hint, got: {result}"
        );
        assert!(
            result.contains("public function toArray()"),
            "expected stub without return type, got: {result}"
        );
    }

    #[test]
    fn shorten_type_generic_with_nested_union() {
        let mut use_map = HashMap::new();
        use_map.insert("User".to_string(), "App\\Models\\User".to_string());
        let ns = None;

        // The `|` inside `<…>` must NOT be treated as a union separator.
        assert_eq!(
            shorten_type("Collection<App\\Models\\User|null>", &use_map, &ns),
            "Collection<User|null>"
        );
    }

    #[test]
    fn stub_keeps_protected_visibility() {
        let methods = vec![MethodInfo {
            visibility: Visibility::Protected,
            ..MethodInfo::virtual_method("doWork", None)
        }];

        let content = "<?php\nclass Foo {\n    \n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        let result = build_method_stubs(&methods, &HashMap::new(), &None, content, &class);
        assert!(result.contains("protected function doWork()"));
    }

    #[test]
    fn stub_promotes_private_to_public() {
        let methods = vec![MethodInfo {
            visibility: Visibility::Private,
            ..MethodInfo::virtual_method("doWork", None)
        }];

        let content = "<?php\nclass Foo {\n    \n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        let result = build_method_stubs(&methods, &HashMap::new(), &None, content, &class);
        assert!(result.contains("public function doWork()"));
    }

    #[test]
    fn stub_with_parameters_and_defaults() {
        let methods = vec![MethodInfo {
            parameters: vec![
                ParameterInfo {
                    name: crate::atom::atom("$name"),
                    is_required: true,
                    type_hint: Some(PhpType::parse("string")),
                    native_type_hint: Some(PhpType::parse("string")),
                    description: None,
                    default_value: None,
                    is_variadic: false,
                    is_reference: false,
                    closure_this_type: None,
                },
                ParameterInfo {
                    name: crate::atom::atom("$options"),
                    is_required: false,
                    type_hint: Some(PhpType::parse("array")),
                    native_type_hint: Some(PhpType::parse("array")),
                    description: None,
                    default_value: Some("[]".to_string()),
                    is_variadic: false,
                    is_reference: false,
                    closure_this_type: None,
                },
            ],
            native_return_type: Some(PhpType::parse("void")),
            visibility: Visibility::Public,
            ..MethodInfo::virtual_method("process", Some("void"))
        }];

        let content = "<?php\nclass Foo {\n    \n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        let result = build_method_stubs(&methods, &HashMap::new(), &None, content, &class);
        assert!(
            result.contains("public function process(string $name, array $options = []): void")
        );
    }
}
