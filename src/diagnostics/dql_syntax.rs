//! Doctrine Query Language that does not parse, or uses a variable nothing
//! declares.
//!
//! The symbol map records where each query, and each fragment a query
//! builder joins into one, stops following DQL's grammar (see
//! [`crate::doctrine::dql_syntax`]), and each variable a query, or the
//! builders a function body creates, uses without declaring.  Doctrine
//! throws on such a query when it runs, so each is an error, once the type
//! engine confirms that the call taking the text is Doctrine's: `where()`
//! and `select()` are common names, and a DBAL query builder takes SQL
//! through the same ones.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::symbol_map::DqlSyntaxSite;

use super::helpers::{FileDiagnosticContext, make_diagnostic};

/// Diagnostic code used for DQL that does not parse.
pub(crate) const DQL_SYNTAX_ERROR_CODE: &str = "dql_syntax_error";

/// Diagnostic code used for a DQL variable nothing declares.
pub(crate) const UNKNOWN_DQL_VARIABLE_CODE: &str = "unknown_dql_variable";

impl Backend {
    /// Report each piece of DQL in the file at `uri` that does not follow
    /// the grammar, when the call that takes it is Doctrine's.
    pub(crate) fn collect_dql_syntax_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let sites: Vec<&DqlSyntaxSite> = ctx
            .symbol_map
            .dql_syntax_errors
            .iter()
            .filter(|site| super::incremental::reports_site(site.start))
            .collect();
        if sites.is_empty() || !ctx.symbol_map.matches_source(content) {
            return;
        }
        let mut by_call: HashMap<u32, Vec<&DqlSyntaxSite>> = HashMap::new();
        for site in sites {
            by_call.entry(site.call).or_default().push(site);
        }

        let _parse_guard = with_parse_cache(content);
        let loaders = self.file_loaders(&ctx.file);
        let confirmed: Vec<u32> = with_parsed_program(content, "dql_syntax", |program, content| {
            let mut calls = Calls {
                wanted: &by_call,
                found: Vec::new(),
            };
            for statement in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&CallFinder, statement, &mut calls);
            }
            calls
                .found
                .into_iter()
                .filter(|&(method_start, receiver)| {
                    let Some(site) = by_call.get(&method_start).and_then(|s| s.first()) else {
                        return false;
                    };
                    let var_ctx = loaders.var_ctx_at(content, receiver.span().start.offset);
                    crate::type_engine::variable::iterable_elements::resolve_expression_type(
                        receiver, &var_ctx,
                    )
                    .is_some_and(|ty| {
                        crate::class_lookup::is_subtype_of_named(
                            &ty,
                            site.receiver,
                            loaders.resolved_class_loader(),
                        )
                    })
                })
                .map(|(method_start, _)| method_start)
                .collect()
        });

        let mut reported: Vec<&DqlSyntaxSite> = confirmed
            .iter()
            .filter_map(|method_start| by_call.get(method_start))
            .flatten()
            .copied()
            .collect();
        reported.sort_by_key(|site| (site.start, site.end));
        for site in reported {
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                site.start as usize,
                site.end as usize,
            ) else {
                continue;
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Error,
                site.code,
                site.message.to_string(),
            ));
        }
    }
}

/// The calls the walk is after, and the receivers of the ones it found.
struct Calls<'w, 'ast, 'arena> {
    wanted: &'w HashMap<u32, Vec<&'w DqlSyntaxSite>>,
    found: Vec<(u32, &'ast Expression<'arena>)>,
}

struct CallFinder;

impl CallFinder {
    fn visit<'ast, 'arena>(
        object: &'ast Expression<'arena>,
        method: &'ast ClassLikeMemberSelector<'arena>,
        calls: &mut Calls<'_, 'ast, 'arena>,
    ) {
        if let ClassLikeMemberSelector::Identifier(name) = method {
            let start = name.span.start.offset;
            if calls.wanted.contains_key(&start) {
                calls.found.push((start, object));
            }
        }
    }
}

impl<'ast, 'arena, 'w> mago_syntax::walker::Walker<'ast, 'arena, Calls<'w, 'ast, 'arena>>
    for CallFinder
{
    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        calls: &mut Calls<'w, 'ast, 'arena>,
    ) {
        Self::visit(node.object, &node.method, calls);
    }

    fn walk_in_null_safe_method_call(
        &self,
        node: &'ast NullSafeMethodCall<'arena>,
        calls: &mut Calls<'w, 'ast, 'arena>,
    ) {
        Self::visit(node.object, &node.method, calls);
    }
}
