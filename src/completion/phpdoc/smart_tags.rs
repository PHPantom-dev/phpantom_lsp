//! The tag items pre-filled from the declaration a docblock documents:
//! `@param` for each undocumented parameter, `@return` and `@var` from the
//! declared type, and `@throws` for each exception the body lets escape.

use std::collections::HashMap;

use mago_syntax::cst::Statement;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat, Position};

use crate::completion::source::throws_analysis::{self, ThrowsContext};
use crate::completion::use_edit::{analyze_use_block, build_use_edit};
use crate::php_type::PhpType;

use super::tags::TagDef;
use super::{
    DocblockContext, SmartContext, SymbolInfo, find_existing_param_tags, find_existing_throws_tags,
    generation, has_existing_return_tag, shorten_php_type,
};

/// The docblock a tag is completed in, and the declaration below it.
pub(super) struct TagSite<'a> {
    pub(super) content: &'a str,
    pub(super) position: Position,
    pub(super) context: DocblockContext,
    pub(super) use_map: &'a HashMap<String, String>,
    pub(super) file_namespace: &'a Option<String>,
    pub(super) smart: &'a SmartContext<'a>,
    /// The declaration the docblock documents.
    pub(super) sym: SymbolInfo,
}

/// Push the items `def` gets pre-filled from the declaration, and return
/// whether they stand in for the tag's plain item.
pub(super) fn push_smart_tag_items(
    site: &TagSite,
    def: &TagDef,
    items: &mut Vec<CompletionItem>,
) -> bool {
    match (def.tag, site.context) {
        ("@throws", DocblockContext::FunctionOrMethod | DocblockContext::Unknown) => {
            push_throws_items(site, def, items);
            // The plain `@throws ExceptionType` stays, sorted after them, so
            // the user can document what the analysis missed (external
            // calls, abstract methods).
            false
        }
        // Either each undocumented parameter got an item, or every
        // parameter is documented (or there are none), where the plain
        // tag is no use either.
        ("@param", _) => {
            push_param_items(site, def, items);
            true
        }
        ("@return", _) => push_return_item(site, def, items),
        ("@var", DocblockContext::Property | DocblockContext::Constant) => {
            push_declared_var_item(site, def, items)
        }
        ("@var", DocblockContext::Inline) => {
            items.push(inline_var_item(site, def));
            true
        }
        ("@var", DocblockContext::Unknown) => {
            // With no assignment or property below to name the variable,
            // its name is a tab stop too: `@var Type $var`.
            items.push(tag_item(
                def,
                "@var Type $var".to_string(),
                "var ${1:Type} \\$${2:var}".to_string(),
                Some(InsertTextFormat::SNIPPET),
                format!("1_{}", def.tag.to_lowercase()),
            ));
            true
        }
        _ => false,
    }
}

/// A tag item inserting `insert`, a snippet when `format` says so.
fn tag_item(
    def: &TagDef,
    label: String,
    insert: String,
    format: Option<InsertTextFormat>,
    sort_text: String,
) -> CompletionItem {
    CompletionItem {
        label,
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(def.detail.to_string()),
        insert_text: Some(insert),
        insert_text_format: format,
        filter_text: Some(def.tag.to_string()),
        sort_text: Some(sort_text),
        ..CompletionItem::default()
    }
}

/// `ty` as the docblock should spell it (the file's imports applied, and
/// `@template` arguments filled in when the class loader knows the
/// class), and the same with tab stops on those arguments when it has any.
fn enriched_type(site: &TagSite, ty: &PhpType) -> (String, Option<String>) {
    let class_loader = site.smart.class_loader;
    let label = class_loader
        .and_then(|cl| {
            generation::enrichment_plain(Some(ty), cl, site.use_map, site.file_namespace)
        })
        .unwrap_or_else(|| shorten_php_type(ty, site.use_map, site.file_namespace));
    let snippet = class_loader.and_then(|cl| {
        generation::enrichment_snippet(Some(ty), cl, site.use_map, site.file_namespace)
    });
    (label, snippet)
}

/// `@throws` for each exception the function lets escape that the
/// docblock does not document yet, importing the ones the file does not.
fn push_throws_items(site: &TagSite, def: &TagDef, items: &mut Vec<CompletionItem>) {
    let TagSite {
        content,
        position,
        use_map,
        file_namespace,
        smart,
        ..
    } = *site;
    let uncaught = match smart.class_loader {
        Some(class_loader) => throws_analysis::find_uncaught_throw_types_with_context(
            content,
            position,
            Some(&ThrowsContext {
                class_loader,
                function_loader: smart.function_loader,
                use_map,
                local_classes: smart.local_classes,
                backend: smart.import_target.map(|(backend, _)| backend),
            }),
        ),
        None => throws_analysis::find_uncaught_throw_types(content, position),
    };
    let existing_throws = find_existing_throws_tags(content, position);
    let missing: Vec<String> = uncaught
        .iter()
        .map(|t| t.to_string())
        .filter(|t| {
            let t_short = crate::util::short_name(t);
            !existing_throws
                .iter()
                .any(|e| e.eq_ignore_ascii_case(t_short))
        })
        .collect();
    if missing.is_empty() {
        return;
    }

    let use_block = match smart.import_target {
        Some((backend, uri)) => backend.use_block_at(uri, content, position),
        None => analyze_use_block(content),
    };
    for (idx, exc_type) in missing.iter().enumerate() {
        let display_name = crate::util::short_name(exc_type);
        // Exception types are already resolved to FQNs by the throws
        // analysis — do not re-resolve.
        let additional_text_edits = if throws_analysis::has_use_import(content, exc_type) {
            None
        } else {
            build_use_edit(exc_type, &use_block, file_namespace)
        };
        items.push(CompletionItem {
            additional_text_edits,
            ..tag_item(
                def,
                format!("@throws {display_name}"),
                format!("throws {display_name}"),
                None,
                format!("0a_{}_{:03}", def.tag.to_lowercase(), idx),
            )
        });
    }
}

/// `@param` for each parameter the docblock does not document yet.
fn push_param_items(site: &TagSite, def: &TagDef, items: &mut Vec<CompletionItem>) {
    if site.sym.params.is_empty() {
        return;
    }
    let existing = find_existing_param_tags(site.content, site.position);
    let undocumented = site
        .sym
        .params
        .iter()
        .filter(|(_, name)| !existing.iter().any(|e| e == name));
    for (idx, (type_hint, name)) in undocumented.enumerate() {
        let (insert, label, format) = match type_hint {
            Some(th) => {
                let (label_type, snippet) = enriched_type(site, th);
                let label = format!("@param {label_type} {name}");
                match snippet {
                    Some(snippet) => (
                        format!("param {} {}", snippet, name.replace('$', "\\$")),
                        label,
                        Some(InsertTextFormat::SNIPPET),
                    ),
                    None => (format!("param {label_type} {name}"), label, None),
                }
            }
            None => (format!("param {name}"), format!("@param {name}"), None),
        };
        items.push(tag_item(
            def,
            label,
            insert,
            format,
            format!("0a_{}_{:03}", def.tag.to_lowercase(), idx),
        ));
    }
}

/// `@return` with the declared return type, or `@return void` for a
/// function without one whose body returns no value.  Returns whether the
/// plain tag is redundant: false only when neither applies.
fn push_return_item(site: &TagSite, def: &TagDef, items: &mut Vec<CompletionItem>) -> bool {
    if has_existing_return_tag(site.content, site.position) {
        return true;
    }
    let sort_text = format!("0a_{}", def.tag.to_lowercase());
    match &site.sym.return_type {
        // An explicit `: void` speaks for itself.
        Some(ret) if ret.is_void() => true,
        Some(ret) => {
            let (label_type, snippet) = enriched_type(site, ret);
            let (insert, format) = match snippet {
                Some(snippet) => (format!("return {snippet}"), Some(InsertTextFormat::SNIPPET)),
                None => (format!("return {label_type}"), None),
            };
            items.push(tag_item(
                def,
                format!("@return {label_type}"),
                insert,
                format,
                sort_text,
            ));
            true
        }
        // A body that returns a value leaves the type for the user to
        // write, through the plain `@return Type`.
        None if body_returns_value(site.content, site.position) => false,
        None => {
            items.push(tag_item(
                def,
                "@return void".to_string(),
                "return void".to_string(),
                None,
                sort_text,
            ));
            true
        }
    }
}

/// Whether the body of the function below the cursor has a `return` with
/// a value of its own (not one in a closure it declares).
fn body_returns_value(content: &str, position: Position) -> bool {
    fn returns_value(statements: &[Statement<'_>]) -> bool {
        statements.iter().any(|statement| match statement {
            Statement::Return(ret) => ret.value.is_some(),
            _ => crate::parser::any_nested_statement(statement, |inner| {
                returns_value(std::slice::from_ref(inner))
            }),
        })
    }
    throws_analysis::with_documented_function_body(content, position, |body, _, _| {
        returns_value(body.statements.as_slice())
    })
    .unwrap_or(false)
}

/// `@var` with a property's or constant's declared type.  Returns false
/// when it declares none.
fn push_declared_var_item(site: &TagSite, def: &TagDef, items: &mut Vec<CompletionItem>) -> bool {
    let Some(th) = &site.sym.type_hint else {
        return false;
    };
    let (label_type, snippet) = enriched_type(site, th);
    let (insert, format) = match snippet {
        Some(snippet) => (format!("var {snippet}"), Some(InsertTextFormat::SNIPPET)),
        None => (format!("var {label_type}"), None),
    };
    items.push(tag_item(
        def,
        format!("@var {label_type}"),
        insert,
        format,
        format!("0a_{}", def.tag.to_lowercase()),
    ));
    true
}

/// `@var` above a variable assignment, promoted property or property in
/// code, sorted first so the narrowing type is quick to type, and
/// pre-filled when the assignment's type can be inferred.
fn inline_var_item(site: &TagSite, def: &TagDef) -> CompletionItem {
    let sort_text = format!("0a_{}", def.tag.to_lowercase());
    let snippet = Some(InsertTextFormat::SNIPPET);
    match &site.smart.inferred_inline_var_type {
        Some(ty) => {
            let (label_type, type_snippet) = enriched_type(site, ty);
            let type_snippet = type_snippet.unwrap_or_else(|| format!("${{1:{label_type}}}"));
            tag_item(
                def,
                format!("@var {label_type}"),
                format!("var {type_snippet}"),
                snippet,
                sort_text,
            )
        }
        None => tag_item(
            def,
            "@var Type".to_string(),
            "var ${1:Type}".to_string(),
            snippet,
            sort_text,
        ),
    }
}
