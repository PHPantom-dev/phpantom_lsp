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
//! through the same ones.  The DQL an expression object is constructed
//! with (`new Expr\OrderBy('u.name')`) is confirmed by the class of the
//! object.
//!
//! A variable used on a builder another method returns, or that is passed
//! to one, is reported only once the type engine finds that method, and
//! the methods it hands the builder on to in turn, and their bodies are
//! known not to declare the variable on some path to the query.
//! A variable such a method uses on the builder and leaves undeclared is
//! reported at the `getQuery()` call of a caller that keeps the builder to
//! itself, when some path through that caller never declares it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::Atom;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::resolution::FileLoaders;
use crate::symbol_map::extraction::{PathEvent, undeclared_on_some_path};
use crate::symbol_map::{DqlProvider, DqlRun, DqlSyntaxSite, SymbolMap};

use super::helpers::{FileDiagnosticContext, make_diagnostic};

/// Diagnostic code used for DQL that does not parse.
pub(crate) const DQL_SYNTAX_ERROR_CODE: &str = "invalid_dql_syntax";

/// Diagnostic code used for a DQL variable nothing declares.
pub(crate) const UNKNOWN_DQL_VARIABLE_CODE: &str = "invalid_dql_alias";

/// The class a `getQuery()` call is made on for the variables of the
/// builder it runs to be judged.
const QUERY_BUILDER: &str = "Doctrine\\ORM\\QueryBuilder";

/// A diagnostic to report: the range it covers, its message and its code.
type Report<'s> = (u32, u32, std::borrow::Cow<'s, str>, &'static str);

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
        let runs: Vec<&DqlRun> = ctx
            .symbol_map
            .dql_runs
            .iter()
            .filter(|run| super::incremental::reports_site(run.start))
            .collect();
        if (sites.is_empty() && runs.is_empty()) || !ctx.symbol_map.matches_source(content) {
            return;
        }
        let mut by_call: HashMap<u32, Vec<&DqlSyntaxSite>> = HashMap::new();
        let mut receivers: HashMap<u32, &'static str> = HashMap::new();
        let mut wanted: HashSet<u32> = HashSet::new();
        let mut bodies: HashSet<u32> = HashSet::new();
        for site in &sites {
            by_call.entry(site.call).or_default().push(site);
            receivers.insert(site.call, site.receiver);
            wanted.insert(site.call);
            if !site.requires.is_empty() {
                wanted.extend(site.requires.iter().map(|provider| provider.call));
                bodies.extend(site.body);
            }
        }
        for run in &runs {
            receivers.insert(run.start, QUERY_BUILDER);
            wanted.insert(run.start);
            wanted.extend(run.requires.iter().map(|provider| provider.call));
            bodies.insert(run.body);
        }
        if !bodies.is_empty() {
            // The file's own methods may hand the builder on in turn.
            wanted.extend(required_calls(&ctx.symbol_map));
        }

        let _parse_guard = with_parse_cache(content);
        let loaders = self.file_loaders(&ctx.file);
        let own: Arc<str> = Arc::from(uri);
        let mut reported: Vec<Report<'_>> =
            with_parsed_program(content, "dql_syntax", |program, content| {
                let mut calls = Calls {
                    wanted: &wanted,
                    bodies: &bodies,
                    found: Vec::new(),
                    statements: Vec::new(),
                };
                if bodies.contains(&0) {
                    calls.statements.push((0, program.statements.as_slice()));
                }
                for statement in program.statements.iter() {
                    mago_syntax::walker::Walker::walk_statement(&CallFinder, statement, &mut calls);
                }
                let confirmed: HashSet<u32> = calls
                    .found
                    .iter()
                    .filter(|found| {
                        let Receiver::Object(receiver) = found.receiver else {
                            return false;
                        };
                        let Some(class) = receivers.get(&found.call) else {
                            return false;
                        };
                        let var_ctx = loaders.var_ctx_at(content, receiver.span().start.offset);
                        crate::type_engine::variable::iterable_elements::resolve_expression_type(
                            receiver, &var_ctx,
                        )
                        .is_some_and(|ty| {
                            crate::class_lookup::is_subtype_of_named(
                                &ty,
                                class,
                                loaders.resolved_class_loader(),
                            )
                        })
                    })
                    .map(|found| found.call)
                    .collect();

                let mut providers = Providers {
                    backend: self,
                    calls: HashMap::new(),
                    maps: HashMap::from([(Arc::clone(&own), Arc::clone(&ctx.symbol_map))]),
                    read: HashSet::from([Arc::clone(&own)]),
                    open: HashMap::new(),
                    opening: HashSet::new(),
                };
                if !bodies.is_empty() {
                    for found in &calls.found {
                        let methods =
                            self.call_targets(found, &loaders, content, uri, &ctx.symbol_map);
                        providers.insert(&own, found.call, methods);
                    }
                }
                let mut known: HashMap<DqlProvider, Option<Vec<Atom>>> = HashMap::new();
                let mut settled: Vec<(u32, Atom)> = Vec::new();
                let mut reported: Vec<Report<'_>> = Vec::new();
                let mut confirmed_sites: Vec<&DqlSyntaxSite> = confirmed
                    .iter()
                    .filter_map(|call| by_call.get(call))
                    .flatten()
                    .copied()
                    .collect();
                confirmed_sites.sort_by_key(|site| (site.start, site.end));
                for site in confirmed_sites {
                    if site.requires.is_empty() {
                        reported.push(report(site));
                        continue;
                    }
                    // An undeclared variable's site covers the variable.
                    let alias = content
                        .get(site.start as usize..site.end as usize)
                        .unwrap_or_default();
                    // Each use some path may leave undeclared was kept, so
                    // only the first one that is goes out.
                    let alias_atom = crate::atom::atom(alias);
                    if site
                        .body
                        .is_some_and(|body| settled.contains(&(body, alias_atom)))
                    {
                        continue;
                    }
                    let declaring: Vec<u32> = site
                        .requires
                        .iter()
                        .filter(|provider| {
                            known
                                .entry(**provider)
                                .or_insert_with(|| providers.aliases(&own, **provider))
                                .as_ref()
                                .is_none_or(|aliases| aliases.iter().any(|a| a.as_str() == alias))
                        })
                        .map(|provider| provider.call)
                        .collect();
                    let undeclared = declaring.is_empty()
                        || site.body.is_some_and(|body| {
                            undeclared_along_paths(
                                site.start,
                                alias,
                                &declaring,
                                body,
                                &calls.statements,
                                &ctx.symbol_map,
                            )
                        });
                    if undeclared {
                        settled.extend(site.body.map(|body| (body, alias_atom)));
                        reported.push(report(site));
                    }
                }

                // The variables the methods a run builder comes from or is
                // passed to use and leave undeclared.
                for run in &runs {
                    if !confirmed.contains(&run.start) {
                        continue;
                    }
                    let mut open: Vec<(Atom, u32)> = Vec::new();
                    let mut known_all = true;
                    for provider in run.requires.iter() {
                        let Some(uses) = providers.open_uses(&own, *provider) else {
                            known_all = false;
                            break;
                        };
                        for alias in uses {
                            if !open.iter().any(|(a, _)| *a == alias) {
                                open.push((alias, provider.call));
                            }
                        }
                    }
                    if !known_all {
                        continue;
                    }
                    for (alias, from) in open {
                        if settled.contains(&(run.body, alias)) {
                            continue;
                        }
                        let declaring: Vec<u32> = run
                            .requires
                            .iter()
                            .filter(|provider| {
                                known
                                    .entry(**provider)
                                    .or_insert_with(|| providers.aliases(&own, **provider))
                                    .as_ref()
                                    .is_none_or(|aliases| aliases.contains(&alias))
                            })
                            .map(|provider| provider.call)
                            .collect();
                        if undeclared_along_paths(
                            from,
                            alias.as_str(),
                            &declaring,
                            run.body,
                            &calls.statements,
                            &ctx.symbol_map,
                        ) {
                            settled.push((run.body, alias));
                            let method = method_name_at(content, from);
                            reported.push((
                                run.start,
                                run.end,
                                format!("Undefined DQL variable '{alias}' in {method}()").into(),
                                UNKNOWN_DQL_VARIABLE_CODE,
                            ));
                        }
                    }
                }
                reported
            });

        reported.sort_by_key(|(start, end, _, _)| (*start, *end));
        for (start, end, message, code) in reported {
            let Some(range) =
                self.offset_range_to_lsp_range(uri, content, start as usize, end as usize)
            else {
                continue;
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Error,
                code,
                message.into_owned(),
            ));
        }
    }
}

/// The diagnostic `site` reports.
fn report(site: &DqlSyntaxSite) -> Report<'_> {
    (site.start, site.end, (&*site.message).into(), site.code)
}

/// The name of the method whose name starts at `offset` of `content`.
fn method_name_at(content: &str, offset: u32) -> &str {
    let rest = content.get(offset as usize..).unwrap_or_default();
    let len = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    &rest[..len]
}

/// Whether some path through the function body starting at `body` uses
/// the variable `alias` at `use_at` and never declares it, counting the
/// calls `declaring` of other methods as declaring it.
fn undeclared_along_paths(
    use_at: u32,
    alias: &str,
    declaring: &[u32],
    body: u32,
    statements: &[(u32, &[Statement<'_>])],
    map: &SymbolMap,
) -> bool {
    let Some(&(_, statements)) = statements.iter().find(|(start, _)| *start == body) else {
        return false;
    };
    let Some(decls) = map.dql_body(body) else {
        return false;
    };
    let mut events: Vec<(u32, PathEvent<'_>)> = std::iter::once((use_at, PathEvent::Use(0)))
        .chain(
            decls
                .decls
                .iter()
                .filter(|(_, a)| a.as_str() == alias)
                .map(|(at, _)| (*at, PathEvent::Declare(alias))),
        )
        .chain(declaring.iter().map(|&at| (at, PathEvent::Declare(alias))))
        .collect();
    undeclared_on_some_path(statements, &mut events, &[alias])
        .first()
        .copied()
        .unwrap_or(false)
}

/// The calls the methods of the file behind `map` hand a builder on with.
fn required_calls(map: &SymbolMap) -> impl Iterator<Item = u32> + '_ {
    map.dql_builder_methods
        .iter()
        .flat_map(|method| method.requires.iter().map(|provider| provider.call))
}

/// A method a call reaches: the file that declares it, the symbol map of
/// that file, and where its name starts there.
struct ProvidingMethod {
    file: Arc<str>,
    map: Arc<SymbolMap>,
    name_offset: u32,
}

/// The methods a call reaches.
type Targets = Arc<[ProvidingMethod]>;

/// The methods the calls that hand a builder on reach, found file by file
/// as the methods are followed.
struct Providers<'b> {
    backend: &'b Backend,
    /// The methods each call of each file reaches, `None` when any of them
    /// cannot be found.
    calls: HashMap<(Arc<str>, u32), Option<Targets>>,
    /// The symbol map of each file a method was found in.
    maps: HashMap<Arc<str>, Arc<SymbolMap>>,
    /// The files whose calls have been looked up.
    read: HashSet<Arc<str>>,
    /// The variables each call of each file hands a builder to methods
    /// that use and leave undeclared (see [`Providers::open_uses`]).
    open: HashMap<(Arc<str>, DqlProvider), Option<Vec<Atom>>>,
    /// The calls whose variables left undeclared are being looked up.
    opening: HashSet<(Arc<str>, DqlProvider)>,
}

impl Providers<'_> {
    fn insert(&mut self, file: &Arc<str>, call: u32, methods: Option<Vec<ProvidingMethod>>) {
        let methods: Option<Targets> = methods.map(|methods| {
            for method in &methods {
                self.maps
                    .entry(Arc::clone(&method.file))
                    .or_insert_with(|| Arc::clone(&method.map));
            }
            methods.into()
        });
        self.calls.insert((Arc::clone(file), call), methods);
    }

    /// The variables the methods the call `provider` of `file` reaches
    /// declare on the builder, and the methods they hand it on to, and so
    /// on.  `None` when any of them cannot be found or is not known to
    /// declare only what its body spells.
    fn aliases(&mut self, file: &Arc<str>, provider: DqlProvider) -> Option<Vec<Atom>> {
        let mut aliases: Vec<Atom> = Vec::new();
        let mut seen: HashSet<(Arc<str>, DqlProvider)> = HashSet::new();
        let mut pending = vec![(Arc::clone(file), provider)];
        while let Some((file, provider)) = pending.pop() {
            if !seen.insert((Arc::clone(&file), provider)) {
                continue;
            }
            let methods = self.targets(&file, provider.call)?;
            for method in methods.iter() {
                let summary = method
                    .map
                    .dql_builder_method(method.name_offset, provider.slot)?;
                for alias in summary.aliases.iter() {
                    if !aliases.contains(alias) {
                        aliases.push(*alias);
                    }
                }
                pending.extend(
                    summary
                        .requires
                        .iter()
                        .map(|required| (Arc::clone(&method.file), *required)),
                );
            }
        }
        Some(aliases)
    }

    /// The variables the methods the call `provider` of `file` reaches use
    /// on the builder and leave for their callers to declare: the ones
    /// their own bodies use, and the ones the methods they hand the builder
    /// on to leave, less the ones they and those methods declare.  `None`
    /// when any of them cannot be found or is not known to declare only
    /// what its body spells.  A call met again while its own variables are
    /// being looked up adds none.
    fn open_uses(&mut self, file: &Arc<str>, provider: DqlProvider) -> Option<Vec<Atom>> {
        let key = (Arc::clone(file), provider);
        if let Some(done) = self.open.get(&key) {
            return done.clone();
        }
        if !self.opening.insert(key.clone()) {
            return Some(Vec::new());
        }
        let result = self.find_open_uses(file, provider);
        self.opening.remove(&key);
        self.open.insert(key, result.clone());
        result
    }

    fn find_open_uses(&mut self, file: &Arc<str>, provider: DqlProvider) -> Option<Vec<Atom>> {
        let methods = self.targets(file, provider.call)?;
        let mut open: Vec<Atom> = Vec::new();
        for method in methods.iter() {
            let summary = method
                .map
                .dql_builder_method(method.name_offset, provider.slot)?;
            let mut uses: Vec<Atom> = summary.uses.to_vec();
            let mut declared: Vec<Atom> = summary.aliases.to_vec();
            for required in summary.requires.iter() {
                uses.extend(self.open_uses(&method.file, *required)?);
                declared.extend(self.aliases(&method.file, *required)?);
            }
            for alias in uses {
                if !declared.contains(&alias) && !open.contains(&alias) {
                    open.push(alias);
                }
            }
        }
        Some(open)
    }

    /// The methods the call whose method name starts at `call` in `file`
    /// reaches.
    fn targets(&mut self, file: &Arc<str>, call: u32) -> Option<Targets> {
        if !self.read.contains(file) {
            self.read_file(file);
        }
        self.calls.get(&(Arc::clone(file), call)).cloned().flatten()
    }

    /// Look up the methods each call the methods of `file` hand a builder
    /// on with reaches.  The calls are resolved in their own file, whose
    /// offsets have nothing to do with the file the diagnostic pass is
    /// checking, so neither of its caches may answer for them.
    fn read_file(&mut self, file: &Arc<str>) {
        self.read.insert(Arc::clone(file));
        let Some(map) = self.maps.get(file).cloned() else {
            return;
        };
        let wanted: HashSet<u32> = required_calls(&map).collect();
        if wanted.is_empty() {
            return;
        }
        let backend = self.backend;
        let Some(content) = backend.get_file_content_arc(file) else {
            return;
        };
        let _isolated = (
            crate::type_engine::variable::forward_walk::suspend_diagnostic_scope(),
            crate::type_engine::resolver::with_isolated_chain_cache(),
        );
        let file_ctx = backend.file_context(file);
        let loaders = backend.file_loaders(&file_ctx);
        let no_bodies = HashSet::new();
        let found: Vec<(u32, Option<Vec<ProvidingMethod>>)> =
            with_parsed_program(&content, "dql_providers", |program, content| {
                let mut calls = Calls {
                    wanted: &wanted,
                    bodies: &no_bodies,
                    found: Vec::new(),
                    statements: Vec::new(),
                };
                for statement in program.statements.iter() {
                    mago_syntax::walker::Walker::walk_statement(&CallFinder, statement, &mut calls);
                }
                calls
                    .found
                    .iter()
                    .map(|found| {
                        (
                            found.call,
                            backend.call_targets(found, &loaders, content, file, &map),
                        )
                    })
                    .collect()
            });
        for (call, methods) in found {
            self.insert(file, call, methods);
        }
    }
}

impl Backend {
    /// The methods the call `found`, read from `content` of the file at
    /// `uri`, reaches.
    fn call_targets(
        &self,
        found: &FoundCall<'_, '_>,
        loaders: &FileLoaders<'_>,
        content: &str,
        uri: &str,
        map: &Arc<SymbolMap>,
    ) -> Option<Vec<ProvidingMethod>> {
        let (expr, class) = match found.receiver {
            Receiver::Object(expr) => (expr, false),
            Receiver::Class(expr) => (expr, true),
        };
        let var_ctx = loaders.var_ctx_at(content, expr.span().start.offset);
        let ty = match expr {
            Expression::Self_(_)
            | Expression::Static(_)
            | Expression::Parent(_)
            | Expression::Identifier(_)
                if class =>
            {
                let name = crate::class_lookup::class_expression_name(
                    expr,
                    var_ctx.current_class,
                    var_ctx.file.all_classes,
                    var_ctx.file.source_class_loader,
                    var_ctx.file.source_imports,
                )?;
                crate::php_type::PhpType::named(crate::atom::atom(&name))
            }
            _ => crate::type_engine::variable::iterable_elements::resolve_expression_type(
                expr, &var_ctx,
            )?,
        };
        self.providing_methods(&ty, found.method, uri, map, loaders.resolved_class_loader())
    }

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
                Some(ProvidingMethod {
                    file: Arc::from(file),
                    map,
                    name_offset,
                })
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
            yii_params: false,
            tempest: false,
        },
        None,
        crate::drupal::hooks::DrupalFile::Other,
    );
    SymbolMap {
        dql_builder_methods: map.dql_builder_methods,
        ..SymbolMap::default()
    }
}

/// What a call is made on: an object, or a class.  A `new` is made on the
/// object it makes.
#[derive(Clone, Copy)]
enum Receiver<'ast, 'arena> {
    Object(&'ast Expression<'arena>),
    Class(&'ast Expression<'arena>),
}

/// A call the walk is after: where its method name starts, or the class
/// name of a `new`, what it is made on, and the method's name, empty for a
/// `new`.
struct FoundCall<'ast, 'arena> {
    call: u32,
    receiver: Receiver<'ast, 'arena>,
    method: &'arena str,
}

/// The calls and function bodies the walk is after, and the ones it found.
struct Calls<'w, 'ast, 'arena> {
    wanted: &'w HashSet<u32>,
    /// Where the bodies start, by their opening brace.
    bodies: &'w HashSet<u32>,
    found: Vec<FoundCall<'ast, 'arena>>,
    statements: Vec<(u32, &'ast [Statement<'arena>])>,
}

struct CallFinder;

impl CallFinder {
    fn visit<'ast, 'arena>(
        receiver: Receiver<'ast, 'arena>,
        method: &'ast ClassLikeMemberSelector<'arena>,
        calls: &mut Calls<'_, 'ast, 'arena>,
    ) {
        if let ClassLikeMemberSelector::Identifier(name) = method {
            let start = name.span.start.offset;
            if calls.wanted.contains(&start) {
                calls.found.push(FoundCall {
                    call: start,
                    receiver,
                    method: crate::atom::bytes_to_str(name.value),
                });
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
        Self::visit(Receiver::Object(node.object), &node.method, calls);
    }

    fn walk_in_null_safe_method_call(
        &self,
        node: &'ast NullSafeMethodCall<'arena>,
        calls: &mut Calls<'w, 'ast, 'arena>,
    ) {
        Self::visit(Receiver::Object(node.object), &node.method, calls);
    }

    fn walk_in_static_method_call(
        &self,
        node: &'ast StaticMethodCall<'arena>,
        calls: &mut Calls<'w, 'ast, 'arena>,
    ) {
        Self::visit(Receiver::Class(node.class), &node.method, calls);
    }

    fn walk_in_block(&self, node: &'ast Block<'arena>, calls: &mut Calls<'w, 'ast, 'arena>) {
        let start = node.left_brace.start.offset;
        if calls.bodies.contains(&start) {
            calls.statements.push((start, node.statements.as_slice()));
        }
    }

    /// An expression object constructed with DQL is confirmed by the
    /// object it makes.
    fn walk_in_expression(
        &self,
        node: &'ast Expression<'arena>,
        calls: &mut Calls<'w, 'ast, 'arena>,
    ) {
        if let Expression::Instantiation(instantiation) = node {
            let start = instantiation.class.span().start.offset;
            if calls.wanted.contains(&start) {
                calls.found.push(FoundCall {
                    call: start,
                    receiver: Receiver::Object(node),
                    method: "",
                });
            }
        }
    }
}
