//! Variable hover.
//!
//! Resolves the type of a variable at the cursor through the shared
//! variable-resolution pipeline and renders it, preserving generic
//! parameters and scalar types.  Union types are shown as separate code
//! blocks divided by a horizontal rule.

use tower_lsp::lsp_types::Hover;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::PhpType;
use crate::symbol_map::{SymbolSpan, VarDefKind};
use crate::type_engine::resolver::FileCtx;
use crate::type_engine::variable::resolution::VarQuery;
use crate::types::{ClassInfo, FileContext, FunctionInfo};

use super::HoverSite;
use super::formatting::{make_hover, namespace_line};

impl Backend {
    /// Hover for a `$variable` the symbol map found, except at a
    /// definition whose type is already on screen.
    pub(super) fn hover_variable_symbol(
        &self,
        site: &HoverSite,
        symbol: &SymbolSpan,
        name: &str,
    ) -> Option<Hover> {
        let HoverSite {
            uri,
            content,
            offset: cursor_offset,
            ctx,
            ..
        } = *site;
        // Suppress hover when the cursor is on a variable at its
        // definition site where the type is already visible in
        // the signature (properties, static/global declarations).
        // For parameters, assignments, foreach bindings, and catch
        // bindings, hover is useful to show the resolved type and
        // any docblock descriptions.
        if let Some(def_kind) = self.lookup_var_def_kind_at(uri, name, cursor_offset)
            && (!matches!(
                def_kind,
                VarDefKind::Assignment
                    | VarDefKind::CompoundAssignment
                    | VarDefKind::Parameter
                    | VarDefKind::Foreach
                    | VarDefKind::Catch
                    | VarDefKind::ArrayDestructuring
                    | VarDefKind::ListDestructuring
            ) || (def_kind == VarDefKind::Parameter
                && self.is_promoted_property_param(uri, symbol.start)))
        {
            // A constructor-promoted parameter declares a property,
            // not a local variable, so it follows the same
            // "already visible on screen" rule as any other
            // property declaration.
            return None;
        }
        self.hover_variable(name, uri, content, cursor_offset, site.current_class(), ctx)
    }

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
                .symbol_map_for(uri)
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

        let resolved_class_loader = self.resolved_class_loader(ctx);
        let function_loader = self.function_loader(ctx);
        let constant_loader = self.constant_loader(ctx);
        let config_resolver = |key: &str| self.resolve_config_type(key);
        let trans_resolver = |key: &str| self.resolve_trans_type(key);
        let loaders = crate::type_engine::resolver::Loaders {
            function_loader: Some(&function_loader as &dyn Fn(&str, u32) -> Option<FunctionInfo>),
            constant_loader: Some(&constant_loader),
            config_resolver: Some(&config_resolver),
            trans_resolver: Some(&trans_resolver),
        };

        // Try the type-string path first.  This preserves generic
        // parameters (e.g. `Generator<int, Pencil>`) and scalar types
        // (e.g. `int`) that the ClassInfo-based path would lose.
        if let Some(resolved_type) =
            crate::type_engine::variable::resolution::resolve_variable_php_type(VarQuery {
                loaders,
                ..VarQuery::new(
                    &var_name,
                    FileCtx::new(content, &ctx.classes, &resolved_class_loader, &class_loader)
                        .with_imports(Some(&ctx.use_map))
                        .with_backend(Some(self)),
                    current_class,
                    cursor_offset,
                )
            })
        {
            // When the type is a template parameter, show its variance
            // and bound (e.g. "**template-covariant** `TNode` of `AstNode`")
            // above the code block so the user sees the constraint.
            let template_line =
                self.find_template_info_for_type(&resolved_type, uri, cursor_offset);

            let hover_body = build_variable_hover_body(
                &var_name,
                &resolved_type,
                &resolved_class_loader,
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
    class_loader: &ResolvedClassLoader<'_>,
    template_line: Option<&str>,
) -> String {
    // A template bounded by a union is still the one template.
    let members = match ty.as_template_param() {
        Some(_) => vec![ty],
        None => ty.union_members(),
    };

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
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<String> {
    // A template's name is not in any namespace, whatever its bound is.
    if ty.as_template_param().is_some() {
        return None;
    }
    let base = ty.base_name()?;

    if let Some(cls) = class_loader(Fqn::new(base)) {
        return cls
            .file_namespace
            .as_ref()
            .filter(|ns| !ns.is_empty() && !ns.starts_with("___"))
            .map(|ns| ns.to_string());
    }

    // Fallback: parse the namespace from the FQN string itself.
    // E.g. `App\Models\User` → `App\Models`.
    if let Some(pos) = base.rfind('\\') {
        let ns = &base[..pos];
        if !ns.is_empty() {
            return Some(ns.to_string());
        }
    }

    None
}
