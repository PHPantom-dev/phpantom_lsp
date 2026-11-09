//! The parameters a Yii render call passes its view, checked against the
//! `@var` block the template opens with (see
//! [`crate::yii::view_parameters`]).
//!
//! A template that declares its parameters is treated like a function
//! signature, the way a Blade template's is (see `blade_call_site.rs`):
//!
//! * a passed value whose type the declaration does not accept is a
//!   **mismatch**;
//! * a passed name the template neither declares nor reads is **unknown**;
//! * a declared name the call does not pass, and that no view is handed by
//!   anything else, is **missing**.
//!
//! A template with no `@var` block is not checked at all.  A template that
//! reads its scope wholesale accepts any name, and a call whose parameters
//! are not all written out, or a project whose shared parameters cannot all
//! be read, leaves nothing to say about what is missing.

use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::symbol_map::extraction::yii_view_parameters_argument;
use crate::symbol_map::{ReceiverClass, StringKeyKind, SymbolKind};
use crate::virtual_members::laravel::helpers::resolved_string_literal;
use crate::yii::view_parameters::ViewTemplate;
use crate::yii::views::is_view_render_method;

use super::helpers::{FileDiagnosticContext, make_diagnostic, shown_type};
use super::type_errors::is_type_compatible;

/// A passed name the template has no use for.
pub(crate) const UNKNOWN_CODE: &str = "invalid_yii_view_parameter";
/// A declared parameter the call does not pass.
pub(crate) const MISSING_CODE: &str = "missing_yii_view_parameter";
/// A passed value whose type the declaration does not accept.
pub(crate) const TYPE_MISMATCH_CODE: &str = "type_mismatch_yii_view_parameter";

/// One parameter a render call writes out.
struct PassedParameter {
    name: String,
    key: (u32, u32),
    value: (u32, u32),
    ty: Option<PhpType>,
}

/// One render call of a view the file names.
struct RenderSite {
    /// The view's key, and the byte range of the string naming it.
    view: String,
    name: (u32, u32),
    passed: Vec<PassedParameter>,
    /// Whether every parameter the call passes is written out.
    complete: bool,
}

impl Backend {
    /// Check the parameters each render call in `uri` passes against the
    /// template it renders.
    pub(crate) fn collect_yii_view_parameter_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if !self.is_yii_project() {
            return;
        }
        let map = &ctx.symbol_map;
        let names_view = |kind: &SymbolKind| {
            matches!(
                kind,
                SymbolKind::StringKey {
                    kind: StringKeyKind::YiiView,
                    is_write: false,
                    ..
                }
            )
        };
        let has_typed = map
            .typed_receiver_sites
            .iter()
            .any(|site| site.receiver == ReceiverClass::YiiViewRenderer);
        if !has_typed && !map.spans.iter().any(|span| names_view(&span.kind)) {
            return;
        }
        let typed = if has_typed {
            self.typed_receiver_spans_for(uri, map)
        } else {
            Default::default()
        };
        let views: HashMap<u32, (u32, &str)> = map
            .spans
            .iter()
            .chain(typed.iter())
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::YiiView,
                    key,
                    is_write: false,
                    ..
                } => Some((span.start, (span.end, key.as_str()))),
                _ => None,
            })
            .collect();
        if views.is_empty() {
            return;
        }

        let sites = self.yii_render_sites(ctx, content, &views);
        if sites.is_empty() {
            return;
        }

        let class_loader = self.resolved_class_loader(&ctx.file);
        let mut templates: HashMap<&str, Option<ViewTemplate>> = HashMap::new();
        // What every view is handed besides its render's parameters, read
        // only once a declared name is short of the render.
        let mut shared: Option<Option<HashSet<String>>> = None;
        for site in &sites {
            let Some(template) = templates
                .entry(site.view.as_str())
                .or_insert_with(|| self.yii_view_template(&site.view))
                .as_ref()
            else {
                continue;
            };
            let view = self.yii_view_display(&site.view);
            let mut push = |(start, end): (u32, u32), severity, code, message: String| {
                if !super::incremental::reports_site(start) {
                    return;
                }
                if let Some(range) =
                    self.offset_range_to_lsp_range(uri, content, start as usize, end as usize)
                {
                    out.push(make_diagnostic(range, severity, code, message));
                }
            };
            for passed in &site.passed {
                if let Some(declared) = template.declared(&passed.name) {
                    let Some(actual) = &passed.ty else {
                        continue;
                    };
                    if is_type_compatible(actual, &declared.ty, &class_loader, true) {
                        continue;
                    }
                    push(
                        passed.value,
                        DiagnosticSeverity::Error,
                        TYPE_MISMATCH_CODE,
                        format!(
                            "View '{view}' expects ${} of type {}, got {}",
                            passed.name,
                            shown_type(&declared.ty.conditionals_as_branch_unions()),
                            shown_type(actual),
                        ),
                    );
                } else if template.accepts(&passed.name) == Some(false) {
                    push(
                        passed.key,
                        DiagnosticSeverity::Warning,
                        UNKNOWN_CODE,
                        format!("View '{view}' has no parameter ${}", passed.name),
                    );
                }
            }
            if !site.complete {
                continue;
            }
            for declared in &template.declared {
                if site
                    .passed
                    .iter()
                    .any(|passed| passed.name == declared.name)
                {
                    continue;
                }
                let Some(shared) = shared
                    .get_or_insert_with(|| self.yii_view_shared_parameters(&class_loader))
                    .as_ref()
                else {
                    break;
                };
                if shared.contains(&declared.name) {
                    continue;
                }
                push(
                    site.name,
                    DiagnosticSeverity::Warning,
                    MISSING_CODE,
                    format!(
                        "View '{view}' expects ${} of type {}, which is not passed",
                        declared.name,
                        shown_type(&declared.ty),
                    ),
                );
            }
        }
    }

    /// The render calls in `content` whose view's name starts at one of the
    /// offsets of `views`, with the parameters each one passes, typed where
    /// they are written.
    fn yii_render_sites(
        &self,
        ctx: &FileDiagnosticContext,
        content: &str,
        views: &HashMap<u32, (u32, &str)>,
    ) -> Vec<RenderSite> {
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();
        let _cache_guard =
            crate::virtual_members::with_active_resolved_class_cache(&self.resolved_class_cache);
        let loaders = self.file_loaders(&ctx.file);
        let resolved_class_loader = loaders.resolved_class_loader();
        let _parse_guard = crate::parser::with_parse_cache(content);
        crate::parser::with_parsed_program(content, "yii_view_parameters", |program, content| {
            let mut calls: Vec<&MethodCall<'_>> = Vec::new();
            let walker = RenderWalker { views };
            for statement in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&walker, statement, &mut calls);
            }
            let type_of = |expression: &Expression<'_>| {
                let offset = expression.span().start.offset;
                let var_ctx = loaders.var_ctx_at(content, offset);
                let ty = match expression {
                    Expression::Variable(Variable::Direct(variable)) => {
                        crate::type_engine::variable::resolution::resolve_variable_php_type(
                            var_ctx.var_query(bytes_to_str(variable.name), offset),
                        )
                    }
                    _ => crate::type_engine::variable::iterable_elements::resolve_expression_type(
                        expression, &var_ctx,
                    ),
                }?;
                Some(crate::blade::call_site_inference::qualify_class_names(
                    ty,
                    resolved_class_loader,
                ))
            };
            calls
                .into_iter()
                .filter_map(|call| {
                    let start = view_name_start(&call.argument_list, content)?;
                    let (end, view) = views.get(&start)?;
                    let mut site = RenderSite {
                        view: view.to_string(),
                        name: (start, *end),
                        passed: Vec::new(),
                        complete: true,
                    };
                    let Some(parameters) = yii_view_parameters_argument(&call.argument_list) else {
                        return Some(site);
                    };
                    let Some(elements) = crate::parser::array_literal_elements(parameters) else {
                        site.complete = false;
                        return Some(site);
                    };
                    for element in elements.iter() {
                        let ArrayElement::KeyValue(entry) = element else {
                            site.complete = false;
                            continue;
                        };
                        let Some((name, key_start, key_end)) =
                            resolved_string_literal(entry.key, content)
                        else {
                            site.complete = false;
                            continue;
                        };
                        let value = entry.value.span();
                        site.passed.push(PassedParameter {
                            name: name.to_string(),
                            key: (key_start as u32, key_end as u32),
                            value: (value.start.offset, value.end.offset),
                            ty: type_of(entry.value),
                        });
                    }
                    Some(site)
                })
                .collect()
        })
    }
}

/// Where the contents of the string naming a render call's view start: its
/// first argument, or the string a `__DIR__ .` path ends with.
fn view_name_start(argument_list: &ArgumentList<'_>, content: &str) -> Option<u32> {
    let first = argument_list.arguments.first()?.value();
    let literal = match first {
        Expression::Binary(binary) if binary.operator.is_concatenation() => binary.rhs,
        value => value,
    };
    resolved_string_literal(literal, content).map(|(_, start, _)| start as u32)
}

/// Finds the render calls whose view's name starts at one of the offsets
/// asked about.
struct RenderWalker<'v, 'k> {
    views: &'v HashMap<u32, (u32, &'k str)>,
}

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Vec<&'ast MethodCall<'arena>>>
    for RenderWalker<'_, '_>
{
    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        calls: &mut Vec<&'ast MethodCall<'arena>>,
    ) {
        let ClassLikeMemberSelector::Identifier(method) = &node.method else {
            return;
        };
        if !is_view_render_method(bytes_to_str(method.value)) {
            return;
        }
        let Some(first) = node.argument_list.arguments.first() else {
            return;
        };
        let span = match first.value() {
            Expression::Binary(binary) if binary.operator.is_concatenation() => binary.rhs.span(),
            value => value.span(),
        };
        // A string's contents start after its quote.
        if self.views.contains_key(&(span.start.offset + 1)) {
            calls.push(node);
        }
    }
}
