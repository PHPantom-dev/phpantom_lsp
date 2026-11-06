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
//!
//! A variable used on a builder another method returns, or that is passed
//! to one, is reported only once the type engine finds that method and its
//! body is known not to declare the variable.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::Atom;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::symbol_map::{DqlProvider, DqlSyntaxSite, SymbolMap};

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
        let mut providers: HashSet<u32> = HashSet::new();
        for site in sites {
            by_call.entry(site.call).or_default().push(site);
            providers.extend(site.requires.iter().map(|provider| provider.call));
        }
        let mut wanted: HashSet<u32> = by_call.keys().copied().collect();
        wanted.extend(&providers);

        let _parse_guard = with_parse_cache(content);
        let loaders = self.file_loaders(&ctx.file);
        let (confirmed, provided) =
            with_parsed_program(content, "dql_syntax", |program, content| {
                let mut calls = Calls {
                    wanted: &wanted,
                    found: Vec::new(),
                };
                for statement in program.statements.iter() {
                    mago_syntax::walker::Walker::walk_statement(&CallFinder, statement, &mut calls);
                }
                let resolve = |receiver: &Expression<'_>| {
                    let var_ctx = loaders.var_ctx_at(content, receiver.span().start.offset);
                    crate::type_engine::variable::iterable_elements::resolve_expression_type(
                        receiver, &var_ctx,
                    )
                };
                let confirmed: Vec<u32> = calls
                    .found
                    .iter()
                    .filter(|&&(method_start, receiver, _)| {
                        let Some(site) = by_call.get(&method_start).and_then(|s| s.first()) else {
                            return false;
                        };
                        resolve(receiver).is_some_and(|ty| {
                            crate::class_lookup::is_subtype_of_named(
                                &ty,
                                site.receiver,
                                loaders.resolved_class_loader(),
                            )
                        })
                    })
                    .map(|&(method_start, _, _)| method_start)
                    .collect();
                // The other methods each call reaches, for the variables
                // their bodies declare.
                let provided: HashMap<u32, Vec<ProvidingMethod>> = calls
                    .found
                    .iter()
                    .filter(|(method_start, _, _)| providers.contains(method_start))
                    .filter_map(|&(method_start, receiver, method)| {
                        let ty = resolve(receiver)?;
                        let methods = self.providing_methods(
                            &ty,
                            method,
                            uri,
                            &ctx.symbol_map,
                            loaders.resolved_class_loader(),
                        )?;
                        Some((method_start, methods))
                    })
                    .collect();
                (confirmed, provided)
            });

        let mut reported: Vec<&DqlSyntaxSite> = confirmed
            .iter()
            .filter_map(|method_start| by_call.get(method_start))
            .flatten()
            .copied()
            .filter(|site| {
                // An undeclared variable's site covers the variable.
                let alias = content
                    .get(site.start as usize..site.end as usize)
                    .unwrap_or_default();
                site.requires
                    .iter()
                    .all(|provider| leaves_undeclared(provider, alias, &provided))
            })
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

/// A method a call reaches: the symbol map of the file that declares it,
/// and where its name starts there.
struct ProvidingMethod {
    map: Arc<SymbolMap>,
    name_offset: u32,
}

/// Whether the call `provider` reaches only methods known to declare no
/// variable but the ones their bodies spell, and none of them `alias`.
fn leaves_undeclared(
    provider: &DqlProvider,
    alias: &str,
    provided: &HashMap<u32, Vec<ProvidingMethod>>,
) -> bool {
    let Some(methods) = provided.get(&provider.call) else {
        return false;
    };
    methods.iter().all(|method| {
        method
            .map
            .dql_builder_aliases(method.name_offset, provider.slot)
            .is_some_and(|aliases: &[Atom]| !aliases.iter().any(|a| a.as_str() == alias))
    })
}

impl Backend {
    /// The method `method` of each class `receiver` may be, with the symbol
    /// map of the file that declares it.  `None` when any of them cannot
    /// be found.
    fn providing_methods(
        &self,
        receiver: &crate::php_type::PhpType,
        method: &str,
        uri: &str,
        own_map: &Arc<SymbolMap>,
        class_loader: &crate::class_lookup::ResolvedClassLoader<'_>,
    ) -> Option<Vec<ProvidingMethod>> {
        let classes = crate::type_engine::type_ops::resolution::type_hint_to_classes_typed(
            receiver,
            "",
            &[],
            class_loader,
        );
        if classes.is_empty() {
            return None;
        }
        classes
            .iter()
            .map(|class| {
                let declaring = crate::hover::find_declaring_class(
                    class,
                    method,
                    &crate::hover::MemberKindForOrigin::Method,
                    class_loader,
                );
                let name_offset = declaring
                    .methods
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(method))?
                    .name_offset;
                if name_offset == 0 {
                    return None;
                }
                let file = self.find_class_file_uri(&declaring.fqn(), uri)?;
                let map = if file == uri {
                    Arc::clone(own_map)
                } else {
                    self.wake_dormant_files(std::iter::once(file.as_str()));
                    match self.symbol_map_for(&file) {
                        Some(map) => map,
                        // A file loaded only for its classes is read for
                        // its methods' builders alone.
                        None => Arc::new(read_dql_builder_methods(
                            &file,
                            &self.get_file_content_arc(&file)?,
                        )),
                    }
                };
                Some(ProvidingMethod { map, name_offset })
            })
            .collect()
    }
}

/// What the methods of the PHP file at `uri` declare on the query builders
/// they hand on.
fn read_dql_builder_methods(uri: &str, content: &str) -> SymbolMap {
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(uri.as_bytes());
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let resolver = mago_names::resolver::NameResolver::new(&arena);
    let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
    let map = crate::symbol_map::extraction::extract_symbol_map_for_file(
        program,
        content,
        &names,
        crate::symbol_map::extraction::Frameworks {
            symfony: false,
            service_container: false,
            drupal: false,
            twig: false,
            doctrine: true,
        },
        None,
        crate::drupal::hooks::DrupalFile::Other,
    );
    SymbolMap {
        dql_builder_methods: map.dql_builder_methods,
        ..SymbolMap::default()
    }
}

/// The calls the walk is after, and the receiver and method name of the
/// ones it found.
struct Calls<'w, 'ast, 'arena> {
    wanted: &'w HashSet<u32>,
    found: Vec<(u32, &'ast Expression<'arena>, &'arena str)>,
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
            if calls.wanted.contains(&start) {
                calls
                    .found
                    .push((start, object, crate::atom::bytes_to_str(name.value)));
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
