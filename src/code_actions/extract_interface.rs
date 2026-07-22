//! "Extract interface" code action.
//!
//! When the cursor is on a concrete class declaration, this action
//! generates an interface containing all public method signatures and
//! updates the class to implement it.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use super::cursor_context::{ClassLikeContextKind, CursorContext, find_cursor_context};
use super::implement_methods::{detect_class_indent, shorten_single_type};
use super::make_code_action_data;
use crate::Backend;
use crate::atom::atom;
use crate::types::{ClassInfo, ClassLikeKind, MethodInfo, Visibility};
use crate::util::offset_to_position;

impl Backend {
    /// Collect "Extract interface" code actions.
    ///
    /// Offered when the cursor is inside a concrete (non-abstract) class
    /// declaration. Returns a deferred code action that is resolved lazily.
    pub(crate) fn collect_extract_interface_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let cursor_offset = crate::util::position_to_offset(content, params.range.start);

        // Only offer on concrete class declarations.
        let in_class_context =
            crate::parser::with_parsed_program(content, "extract_interface", |program, _| {
                let ctx = find_cursor_context(&program.statements, cursor_offset);
                matches!(
                    &ctx,
                    CursorContext::InClassLike {
                        kind: ClassLikeContextKind::Class,
                        ..
                    }
                )
            });
        if !in_class_context {
            return;
        }

        // Look up the ClassInfo for the class the cursor is in.
        let file_ctx = self.file_context(uri);
        let current_class = match file_ctx
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

        // Only concrete classes (not abstract, not interfaces/traits/enums).
        if current_class.kind != ClassLikeKind::Class {
            return;
        }

        // Must have at least one public non-constructor method.
        let has_public_methods = current_class.methods.iter().any(|m| {
            m.visibility == Visibility::Public && m.name.as_str() != "__construct" && !m.is_virtual
        });

        if !has_public_methods {
            return;
        }

        // Build the action as deferred.
        let data = make_code_action_data(
            "refactor.extractInterface",
            uri,
            &params.range,
            serde_json::Value::Null,
        );

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Extract interface".to_string(),
            kind: Some(CodeActionKind::REFACTOR_EXTRACT),
            diagnostics: None,
            edit: None,
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: Some(data),
        }));
    }

    /// Resolve the "Extract interface" deferred code action.
    pub(crate) fn resolve_extract_interface(
        &self,
        data: &super::CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;
        let cursor_offset = crate::util::position_to_offset(content, data.range.start);

        let file_ctx = self.file_context(uri);

        let current_class = file_ctx
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
            .min_by_key(|c| c.end_offset - c.start_offset)?;

        if current_class.kind != ClassLikeKind::Class {
            return None;
        }

        let class_name: &str = current_class.name.as_ref();
        let interface_name = format!("{}Interface", class_name);

        // Collect public methods (excluding constructor and virtual members).
        let public_methods: Vec<&Arc<MethodInfo>> = current_class
            .methods
            .iter()
            .filter(|m| {
                m.visibility == Visibility::Public
                    && m.name.as_str() != "__construct"
                    && !m.is_virtual
            })
            .collect();

        if public_methods.is_empty() {
            return None;
        }

        // Determine namespace and use-map for type shortening.
        let file_namespace = file_ctx.namespace.clone();
        let use_map: HashMap<String, String> = file_ctx.use_map.clone();

        // Collect class-level template params referenced by extracted methods.
        let class_templates = collect_referenced_templates(current_class, &public_methods);

        // Detect indentation from class.
        let indent = detect_class_indent(content, current_class);

        // Generate interface source.
        let interface_source = generate_interface_source(&InterfaceGenParams {
            interface_name: &interface_name,
            file_namespace: &file_namespace,
            methods: &public_methods,
            use_map: &use_map,
            class_templates: &class_templates,
            indent: &indent,
            class: current_class,
        });

        // Determine new file path (same directory as original).
        let doc_url = Url::parse(uri).ok()?;
        let file_path = doc_url.to_file_path().ok()?;
        let dir = file_path.parent()?;
        let new_file_path = dir.join(format!("{}.php", interface_name));
        let new_file_uri = Url::from_file_path(&new_file_path).ok()?;

        // Build the implements clause edit on the original class.
        let implements_edit = build_implements_edit(content, current_class, &interface_name)?;

        // Build document_changes with CreateFile + edits.
        let ops: Vec<DocumentChangeOperation> = vec![
            // 1. Create the new interface file.
            DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                uri: new_file_uri.clone(),
                options: Some(CreateFileOptions {
                    overwrite: Some(false),
                    ignore_if_exists: Some(true),
                }),
                annotation_id: None,
            })),
            // 2. Write content to the new file.
            DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: new_file_uri,
                    version: None,
                },
                edits: vec![OneOf::Left(TextEdit {
                    range: Range {
                        start: Position::new(0, 0),
                        end: Position::new(0, 0),
                    },
                    new_text: interface_source,
                })],
            }),
            // 3. Edit the original file to add `implements`.
            DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: doc_url,
                    version: None,
                },
                edits: vec![OneOf::Left(implements_edit)],
            }),
        ];

        Some(WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Operations(ops)),
            change_annotations: None,
        })
    }
}

/// Collect class-level `@template` parameters that are referenced by
/// the extracted methods (in return types or parameter types).
fn collect_referenced_templates(class: &ClassInfo, methods: &[&Arc<MethodInfo>]) -> Vec<String> {
    if class.template_params.is_empty() {
        return Vec::new();
    }

    let mut referenced = Vec::new();

    for template in &class.template_params {
        let template_str = template.as_str();
        let is_used = methods.iter().any(|m| {
            // Check return type.
            let in_return = m
                .return_type
                .as_ref()
                .is_some_and(|rt| rt.to_string().contains(template_str));
            // Check parameter types.
            let in_params = m.parameters.iter().any(|p| {
                p.type_hint
                    .as_ref()
                    .is_some_and(|t| t.to_string().contains(template_str))
            });
            in_return || in_params
        });

        if is_used {
            referenced.push(template_str.to_string());
        }
    }

    referenced
}

/// Parameters for interface source generation.
struct InterfaceGenParams<'a> {
    interface_name: &'a str,
    file_namespace: &'a Option<String>,
    methods: &'a [&'a Arc<MethodInfo>],
    use_map: &'a HashMap<String, String>,
    class_templates: &'a [String],
    indent: &'a str,
    class: &'a ClassInfo,
}

/// Generate the full PHP source for the interface file.
fn generate_interface_source(params: &InterfaceGenParams<'_>) -> String {
    let InterfaceGenParams {
        interface_name,
        file_namespace,
        methods,
        use_map,
        class_templates,
        indent,
        class,
    } = params;
    let mut src = String::new();
    src.push_str("<?php\n\n");

    // Namespace declaration.
    if let Some(ns) = file_namespace {
        src.push_str(&format!("namespace {};\n\n", ns));
    }

    // Class-level docblock with @template tags.
    if !class_templates.is_empty() {
        src.push_str("/**\n");
        for tmpl in *class_templates {
            // Include the bound if available.
            if let Some(bound) = class.template_param_bounds.get(&atom(tmpl)) {
                let bound_str = shorten_single_type(&bound.to_string(), use_map, file_namespace);
                src.push_str(&format!(" * @template {} of {}\n", tmpl, bound_str));
            } else {
                src.push_str(&format!(" * @template {}\n", tmpl));
            }
        }
        src.push_str(" */\n");
    }

    src.push_str(&format!("interface {}\n{{\n", interface_name));

    // Method signatures.
    for (i, method) in methods.iter().enumerate() {
        if i > 0 {
            src.push('\n');
        }

        // Method-level docblock.
        let method_doc = build_method_docblock(method, use_map, file_namespace, indent);
        if !method_doc.is_empty() {
            src.push_str(&method_doc);
        }

        let static_kw = if method.is_static { "static " } else { "" };
        let params = format_interface_params(method, use_map, file_namespace);
        let return_type = format_interface_return_type(method, use_map, file_namespace);

        src.push_str(indent);
        src.push_str(&format!(
            "public {}function {}({}){};\n",
            static_kw, method.name, params, return_type
        ));
    }

    src.push_str("}\n");
    src
}

/// Build a PHPDoc block for a method in the interface.
fn build_method_docblock(
    method: &MethodInfo,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    indent: &str,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    // Description.
    if let Some(ref desc) = method.description {
        lines.push(format!(" * {}", desc));
    }

    // @template tags for method-level templates.
    for tmpl in &method.template_params {
        if let Some(bound) = method.template_param_bounds.get(tmpl) {
            let bound_str = shorten_single_type(&bound.to_string(), use_map, file_namespace);
            lines.push(format!(" * @template {} of {}", tmpl, bound_str));
        } else {
            lines.push(format!(" * @template {}", tmpl));
        }
    }

    // @param tags.
    for param in &method.parameters {
        if let Some(ref hint) = param.type_hint {
            let hint_str = shorten_single_type(&hint.to_string(), use_map, file_namespace);
            let native_matches = param
                .native_type_hint
                .as_ref()
                .is_some_and(|n| n.to_string() == hint.to_string());
            // Only emit @param if docblock type differs from native type.
            if !native_matches {
                lines.push(format!(" * @param {} {}", hint_str, param.name));
            }
        }
    }

    // @return tag (only if docblock type differs from native).
    if let Some(ref ret) = method.return_type {
        let native_matches = method
            .native_return_type
            .as_ref()
            .is_some_and(|n| n.to_string() == ret.to_string());
        if !native_matches {
            let ret_str = shorten_single_type(&ret.to_string(), use_map, file_namespace);
            lines.push(format!(" * @return {}", ret_str));
        }
    }

    if lines.is_empty() {
        return String::new();
    }

    let mut doc = String::new();
    doc.push_str(indent);
    doc.push_str("/**\n");
    for line in &lines {
        doc.push_str(indent);
        doc.push_str(line);
        doc.push('\n');
    }
    doc.push_str(indent);
    doc.push_str(" */\n");
    doc
}

/// Format parameter list for interface method signature.
fn format_interface_params(
    method: &MethodInfo,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    let mut parts = Vec::new();

    for param in &method.parameters {
        let mut s = String::new();

        if let Some(ref hint) = param.native_type_hint {
            let shortened = hint
                .resolve_names(&|name| shorten_single_type(name, use_map, file_namespace))
                .to_string();
            s.push_str(&shortened);
            s.push(' ');
        }

        if param.is_reference {
            s.push('&');
        }
        if param.is_variadic {
            s.push_str("...");
        }

        s.push_str(&param.name);

        if let Some(ref default) = param.default_value {
            s.push_str(" = ");
            s.push_str(default);
        }

        parts.push(s);
    }

    parts.join(", ")
}

/// Format return type for interface method signature.
fn format_interface_return_type(
    method: &MethodInfo,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    if let Some(ref native) = method.native_return_type {
        let shortened = native
            .resolve_names(&|name| shorten_single_type(name, use_map, file_namespace))
            .to_string();
        if !shortened.is_empty() {
            return format!(": {}", shortened);
        }
    }
    String::new()
}

/// Build a TextEdit that adds `implements InterfaceName` (or appends to
/// an existing implements clause) on the class declaration.
fn build_implements_edit(
    content: &str,
    class: &ClassInfo,
    interface_name: &str,
) -> Option<TextEdit> {
    // Find the opening brace of the class body.
    let brace_offset = class.start_offset as usize;

    if class.interfaces.is_empty() {
        // No existing implements clause — insert before the `{`.
        // Find the `{` character by scanning backwards from start_offset
        // to skip whitespace/newlines.
        let before_brace = content[..brace_offset].trim_end();
        let insert_offset = before_brace.len();
        let insert_pos = offset_to_position(content, insert_offset);

        let new_text = format!(" implements {}", interface_name);

        Some(TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text,
        })
    } else {
        // Already has implements — append after the last interface name.
        // We need to find the position just before the `{` but after
        // the last interface name in the implements list.
        // Strategy: find the `{` and look backwards for text before it.
        let before_brace = &content[..brace_offset];
        let trimmed = before_brace.trim_end();
        let insert_offset = trimmed.len();
        let insert_pos = offset_to_position(content, insert_offset);

        let new_text = format!(", {}", interface_name);

        Some(TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::Atom;
    use crate::types::MethodInfo;

    fn make_method(name: &str, is_static: bool) -> Arc<MethodInfo> {
        Arc::new(MethodInfo {
            name: Atom::from(name),
            name_offset: 0,
            parameters: vec![],
            return_type: None,
            native_return_type: None,
            description: None,
            return_description: None,
            links: vec![],
            see_refs: vec![],
            is_static,
            visibility: Visibility::Public,
            conditional_return: None,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: vec![],
            template_param_bounds: Default::default(),
            template_bindings: vec![],
            has_scope_attribute: false,
            is_abstract: false,
            is_virtual: false,
            is_macro: false,
            is_inferred_return: false,
            type_assertions: vec![],
            throws: vec![],
            if_this_is: None,
        })
    }

    #[test]
    fn generates_simple_interface() {
        let m1 = make_method("foo", false);
        let m2 = make_method("bar", true);
        let methods = vec![&m1, &m2];
        let use_map = HashMap::new();
        let file_namespace = Some("App\\Models".to_string());

        let class = ClassInfo {
            kind: ClassLikeKind::Class,
            name: Atom::from("User"),
            methods: Default::default(),
            method_index: Default::default(),
            indexed_method_count: 0,
            properties: Default::default(),
            constants: Default::default(),
            start_offset: 0,
            end_offset: 0,
            keyword_offset: 0,
            decl_start_offset: 0,
            parent_class: None,
            interfaces: vec![],
            used_traits: vec![],
            mixins: vec![],
            mixin_generics: vec![],
            require_extends: None,
            require_implements: Vec::new(),
            is_final: false,
            is_abstract: false,
            deprecation_message: None,
            deprecated_replacement: None,
            links: vec![],
            see_refs: vec![],
            template_params: vec![],
            template_param_bounds: Default::default(),
            template_param_defaults: Default::default(),
            extends_generics: vec![],
            implements_generics: vec![],
            use_generics: vec![],
            type_aliases: Default::default(),
            trait_precedences: vec![],
            trait_aliases: vec![],
            class_docblock: None,
            file_namespace: Some(Atom::from("App\\Models")),
            backed_type: None,
            attribute_targets: 0,
            laravel: Default::default(),
        };

        let src = generate_interface_source(&InterfaceGenParams {
            interface_name: "UserInterface",
            file_namespace: &file_namespace,
            methods: &methods,
            use_map: &use_map,
            class_templates: &[],
            indent: "    ",
            class: &class,
        });

        assert!(src.contains("namespace App\\Models;"));
        assert!(src.contains("interface UserInterface"));
        assert!(src.contains("public function foo();"));
        assert!(src.contains("public static function bar();"));
    }

    #[test]
    fn build_implements_no_existing() {
        let content = "<?php\nclass User\n{\n}";
        let class = ClassInfo {
            kind: ClassLikeKind::Class,
            name: Atom::from("User"),
            methods: Default::default(),
            method_index: Default::default(),
            indexed_method_count: 0,
            properties: Default::default(),
            constants: Default::default(),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.len() as u32,
            keyword_offset: content.find("class").unwrap() as u32,
            decl_start_offset: content.find("class").unwrap() as u32,
            parent_class: None,
            interfaces: vec![],
            used_traits: vec![],
            mixins: vec![],
            mixin_generics: vec![],
            require_extends: None,
            require_implements: Vec::new(),
            is_final: false,
            is_abstract: false,
            deprecation_message: None,
            deprecated_replacement: None,
            links: vec![],
            see_refs: vec![],
            template_params: vec![],
            template_param_bounds: Default::default(),
            template_param_defaults: Default::default(),
            extends_generics: vec![],
            implements_generics: vec![],
            use_generics: vec![],
            type_aliases: Default::default(),
            trait_precedences: vec![],
            trait_aliases: vec![],
            class_docblock: None,
            file_namespace: None,
            backed_type: None,
            attribute_targets: 0,
            laravel: Default::default(),
        };

        let edit = build_implements_edit(content, &class, "UserInterface").unwrap();
        assert!(edit.new_text.contains("implements UserInterface"));
    }

    #[test]
    fn build_implements_existing() {
        let content = "<?php\nclass User implements Serializable\n{\n}";
        let class = ClassInfo {
            kind: ClassLikeKind::Class,
            name: Atom::from("User"),
            methods: Default::default(),
            method_index: Default::default(),
            indexed_method_count: 0,
            properties: Default::default(),
            constants: Default::default(),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.len() as u32,
            keyword_offset: content.find("class").unwrap() as u32,
            decl_start_offset: content.find("class").unwrap() as u32,
            parent_class: None,
            interfaces: vec![Atom::from("Serializable")],
            used_traits: vec![],
            mixins: vec![],
            mixin_generics: vec![],
            require_extends: None,
            require_implements: Vec::new(),
            is_final: false,
            is_abstract: false,
            deprecation_message: None,
            deprecated_replacement: None,
            links: vec![],
            see_refs: vec![],
            template_params: vec![],
            template_param_bounds: Default::default(),
            template_param_defaults: Default::default(),
            extends_generics: vec![],
            implements_generics: vec![],
            use_generics: vec![],
            type_aliases: Default::default(),
            trait_precedences: vec![],
            trait_aliases: vec![],
            class_docblock: None,
            file_namespace: None,
            backed_type: None,
            attribute_targets: 0,
            laravel: Default::default(),
        };

        let edit = build_implements_edit(content, &class, "UserInterface").unwrap();
        assert_eq!(edit.new_text, ", UserInterface");
    }
}
