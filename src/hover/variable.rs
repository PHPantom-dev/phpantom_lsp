//! Variable hover.
//!
//! Resolves the type of a variable at the cursor through the shared
//! variable-resolution pipeline and renders it, preserving generic
//! parameters and scalar types.  Union types are shown as separate code
//! blocks divided by a horizontal rule.

use std::sync::Arc;

use tower_lsp::lsp_types::Hover;

use crate::Backend;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FileContext, FunctionInfo};
use crate::util::strip_fqn_prefix;

use super::formatting::{make_hover, namespace_line};

impl Backend {
    /// Produce hover information for a variable.
    pub(super) fn hover_variable(
        &self,
        name: &str,
        uri: &str,
        content: &str,
        cursor_offset: u32,
        current_class: Option<&ClassInfo>,
        ctx: &FileContext,
    ) -> Option<Hover> {
        let var_name = format!("${}", name);

        // When the cursor is on the `$` of an assignment like
        // `$x = new Foo()`, the cursor offset equals the assignment
        // statement's start offset.  The variable resolution pipeline
        // skips statements whose start is at or after the cursor
        // (`stmt.start >= cursor_offset`), so the assignment is
        // excluded.  Nudge the offset by one byte so the statement's
        // start is strictly less than the cursor, allowing the
        // assignment to be included.  We only do this for assignments
        // (not parameters, foreach, etc.) where `effective_from`
        // differs from the definition offset.
        let mut cursor_offset = cursor_offset;
        if self
            .lookup_var_def_effective_from(uri, name, cursor_offset)
            .is_some()
        {
            let offset = cursor_offset as usize;
            if let Some(ch) = content.get(offset..).and_then(|s| s.chars().next()) {
                cursor_offset += ch.len_utf8() as u32;
            }
        }

        // $this resolves to the enclosing class, but not inside static methods.
        if name == "this" {
            let in_static = self
                .symbol_maps
                .read()
                .get(uri)
                .is_some_and(|map| map.is_in_static_method(cursor_offset));
            if !in_static && let Some(cc) = current_class {
                let ns_line = namespace_line(cc.file_namespace.as_deref());
                return Some(make_hover(format!(
                    "```php\n<?php\n{}$this = {}\n```",
                    ns_line, cc.name
                )));
            }
            return Some(make_hover("```php\n<?php\n$this\n```".to_string()));
        }

        let class_loader = self.class_loader(ctx);
        let function_loader = self.function_loader(ctx);
        let constant_loader = self.constant_loader();
        let loaders = crate::completion::resolver::Loaders {
            function_loader: Some(&function_loader as &dyn Fn(&str, u32) -> Option<FunctionInfo>),
            constant_loader: Some(&constant_loader),
        };

        // Try the type-string path first.  This preserves generic
        // parameters (e.g. `Generator<int, Pencil>`) and scalar types
        // (e.g. `int`) that the ClassInfo-based path would lose.
        if let Some(resolved_type) =
            crate::completion::variable::resolution::resolve_variable_php_type(
                &var_name,
                content,
                cursor_offset,
                current_class,
                &ctx.classes,
                &class_loader,
                loaders,
            )
        {
            // When the type is a template parameter, show its variance
            // and bound (e.g. "**template-covariant** `TNode` of `AstNode`")
            // above the code block so the user sees the constraint.
            let template_line =
                self.find_template_info_for_type(&resolved_type, uri, cursor_offset);

            let hover_body = build_variable_hover_body(
                &var_name,
                &resolved_type,
                &class_loader,
                template_line.as_deref(),
            );
            return Some(make_hover(hover_body));
        }

        // `resolve_variable_php_type` already calls `resolve_variable_types`
        // internally, so if it returned `None` the variable is truly
        // unresolved.  Show a plain variable hover.
        Some(make_hover(format!("```php\n<?php\n{}\n```", var_name)))
    }
}

/// Build the hover body for a variable, rendering union types as
/// separate code blocks separated by a horizontal rule (`---`).
///
/// For a single type (or scalar/generic) this produces one code block
/// showing e.g. `$user = User`.
///
/// For a union like `Lamp|Faucet` it produces two code blocks
/// (`$ambiguous = Lamp` and `$ambiguous = Faucet`) joined by a
/// markdown horizontal rule so the editor renders a visible divider.
pub(super) fn build_variable_hover_body(
    var_name: &str,
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    template_line: Option<&str>,
) -> String {
    let members = ty.union_members();

    // Count how many members are non-trivial class types (not scalars,
    // not `null`, not `void`, etc.).  Only render separate blocks when
    // there are 2+ class-like types; a simple `Foo|null` should stay
    // in one block.
    let class_like_count = members.iter().filter(|m| !m.is_scalar()).count();

    // When there is only one component, or only one class-like type
    // (the rest being scalars / null), render a single code block.
    if members.len() <= 1 || class_like_count < 2 {
        let short_type = ty.shorten().to_string();
        let ns = resolve_type_namespace_structured(ty, class_loader);
        let ns_line = namespace_line(ns.as_deref());
        let code_block = format!(
            "```php\n<?php\n{}{} = {}\n```",
            ns_line, var_name, short_type
        );
        return if let Some(tpl) = template_line {
            format!("{}\n\n{}", tpl, code_block)
        } else {
            code_block
        };
    }

    // Multiple union branches — render each as its own code block
    // separated by a markdown horizontal rule.
    let mut blocks: Vec<String> = Vec::with_capacity(members.len());
    for member in &members {
        let short = member.shorten().to_string();
        let ns = resolve_type_namespace_structured(member, class_loader);
        let ns_line = namespace_line(ns.as_deref());
        blocks.push(format!(
            "```php\n<?php\n{}{} = {}\n```",
            ns_line, var_name, short
        ));
    }

    let body = blocks.join("\n\n---\n\n");
    if let Some(tpl) = template_line {
        format!("{}\n\n{}", tpl, body)
    } else {
        body
    }
}

/// Extract the namespace for a structured `PhpType` by looking up its
/// base class name via the class loader, or by parsing the namespace
/// from the FQN string itself.
fn resolve_type_namespace_structured(
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    let base = ty.base_name()?;

    if let Some(cls) = class_loader(base) {
        return cls
            .file_namespace
            .as_ref()
            .filter(|ns| !ns.is_empty() && !ns.starts_with("___"))
            .map(|ns| ns.to_string());
    }

    // Fallback: parse the namespace from the FQN string itself.
    // E.g. `App\Models\User` → `App\Models`.
    // Strip leading `\` — input may be a raw docblock type like
    // `\App\Models\User` that hasn't been through resolve_type_string.
    let canonical = strip_fqn_prefix(base);
    if let Some(pos) = canonical.rfind('\\') {
        let ns = &canonical[..pos];
        if !ns.is_empty() {
            return Some(ns.to_string());
        }
    }

    None
}
