//! Component arguments and slot bodies use different PHP scopes.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ops::Range;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::text_position::{offset_to_position, position_to_offset};

use super::discovery::canonical_uri;
use super::inference::{Scope, variable_name};
use super::syntax::{Token, tokens};

struct Argument {
    name: String,
    value: Option<Range<usize>>,
    expression: bool,
}

struct Call {
    start: usize,
    target: String,
    enclosing: Option<usize>,
    args: Vec<Argument>,
    foreach: Option<Range<usize>>,
    slots: Vec<(usize, String)>,
}

struct Template {
    uri: String,
    source: Arc<String>,
    calls: Vec<Call>,
    outlets: BTreeMap<String, Vec<usize>>,
}

impl Template {
    fn read(
        uri: String,
        source: Arc<String>,
        components: &BTreeMap<String, std::path::PathBuf>,
    ) -> Self {
        let mut calls: Vec<Call> = Vec::new();
        let mut outlets: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut stack: Vec<(&str, Option<usize>, Option<usize>)> = Vec::new();
        for token in tokens(&source) {
            let Token::Tag(tag) = token else { continue };
            if tag.closing {
                if let Some(index) = stack.iter().rposition(|(name, ..)| *name == tag.name) {
                    stack.truncate(index);
                }
                continue;
            }
            let attribute = |name| {
                tag.attributes
                    .iter()
                    .find(|attr| attr.name == name)
                    .and_then(|attr| attr.value.as_ref())
                    .map(|range| &source[range.clone()])
                    .filter(|value| !value.is_empty())
            };
            let mut region = stack.last().and_then(|(_, region, _)| *region);
            let mut component = None;
            if let Some(path) = components.get(tag.name) {
                let index = calls.len();
                calls.push(Call {
                    start: tag.span.start,
                    target: canonical_uri(&crate::util::path_to_uri(path)),
                    enclosing: region,
                    args: tag
                        .attributes
                        .iter()
                        .filter(|attr| {
                            !matches!(
                                attr.name,
                                ":if" | ":elseif" | ":else" | ":foreach" | ":forelse" | ":apply"
                            ) && !attr.name.starts_with("::")
                        })
                        .map(|attr| Argument {
                            name: crate::blade::component_tags::camel_case_attr_name(
                                attr.name.trim_start_matches(':'),
                            ),
                            value: attr.value.clone(),
                            expression: attr.name.starts_with(':'),
                        })
                        .collect(),
                    foreach: tag
                        .attributes
                        .iter()
                        .find(|attr| attr.name == ":foreach")
                        .and_then(|attr| attr.value.clone()),
                    slots: if tag.self_closing {
                        Vec::new()
                    } else {
                        vec![(tag.span.start, "default".into())]
                    },
                });
                region = Some(tag.span.start);
                component = Some(index);
            } else if tag.name == "x-slot" {
                if let Some(index) = stack.last().and_then(|(_, _, component)| *component) {
                    if !tag.self_closing {
                        calls[index].slots.push((
                            tag.span.start,
                            attribute("name").unwrap_or("default").into(),
                        ));
                    }
                    region = Some(tag.span.start);
                } else {
                    let name = attribute("define")
                        .or_else(|| attribute("name"))
                        .unwrap_or("default");
                    outlets.entry(name.into()).or_default().push(tag.span.start);
                }
            }
            if !tag.self_closing {
                stack.push((tag.name, region, component));
            }
        }
        Self {
            uri,
            source,
            calls,
            outlets,
        }
    }
}

/// Propagate view data, component arguments, and invocation-specific slots.
pub(super) fn infer(
    backend: &Backend,
    templates: &[String],
    roots: HashMap<String, Scope>,
) -> HashMap<String, Scope> {
    let components = backend.tempest_components();
    if components.is_empty() {
        return roots;
    }
    let aliases: HashMap<_, _> = backend
        .open_files
        .read()
        .keys()
        .filter(|uri| super::is_view(uri))
        .map(|uri| (canonical_uri(uri), uri.clone()))
        .collect();
    let mut pending: VecDeque<_> = templates.iter().map(|uri| canonical_uri(uri)).collect();
    let mut nodes = BTreeMap::new();
    while let Some(key) = pending.pop_front() {
        if nodes.contains_key(&key) {
            continue;
        }
        let uri = aliases.get(&key).unwrap_or(&key);
        let Some(source) = backend.get_file_content_arc(uri) else {
            continue;
        };
        let node = Template::read(uri.clone(), source, &components);
        pending.extend(node.calls.iter().map(|call| call.target.clone()));
        if backend.symbol_map_for(uri).is_none() {
            backend.update_ast(uri, &node.source);
        }
        nodes.insert(key, node);
    }
    let mut incoming: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    for (uri, node) in &nodes {
        for (index, call) in node.calls.iter().enumerate() {
            incoming
                .entry(call.target.clone())
                .or_default()
                .push((uri.clone(), index));
        }
    }
    let mut passed: HashMap<(String, usize), Scope> = HashMap::new();
    let mut scopes = roots.clone();
    let mut pending: VecDeque<_> = nodes.keys().cloned().collect();
    let mut queued: BTreeSet<_> = nodes.keys().cloned().collect();
    let mut visits: HashMap<String, usize> = HashMap::new();
    while let Some(uri) = pending.pop_front() {
        queued.remove(&uri);
        let count = visits.entry(uri.clone()).or_default();
        // Recursive components can construct ever deeper array shapes. Keep
        // inference bounded just as the shared expression resolver does.
        if *count >= 32 {
            continue;
        }
        *count += 1;
        let node = &nodes[&uri];
        let mut scope = roots.get(&uri).cloned().unwrap_or_default();
        for caller in incoming.get(&uri).into_iter().flatten() {
            if let Some(data) = passed.get(caller) {
                scope.merge(data.clone());
            }
        }
        if let Some(previous) = scopes.get(&uri) {
            scope.regions = previous.regions.clone();
        }
        backend.publish_tempest_scope(&node.uri, &node.source, &scope);
        let mut regions = BTreeMap::new();
        for (index, call) in node.calls.iter().enumerate() {
            let Some(target) = nodes.get(&call.target) else {
                continue;
            };
            let mut data = arguments(backend, node, call, &scope);
            let slots = slot_scopes(backend, target, &data);
            data.slot_reads = slot_reads(node, target, call, &scope, &slots);
            let key = (uri.clone(), index);
            if passed.get(&key) != Some(&data) {
                passed.insert(key, data.clone());
                if queued.insert(call.target.clone()) {
                    pending.push_back(call.target.clone());
                }
            }
            for (start, name) in &call.slots {
                regions.insert(
                    *start,
                    slots.get(name).cloned().unwrap_or_else(|| data.clone()),
                );
            }
        }
        if scope.regions != regions {
            scope.regions = regions;
            if queued.insert(uri.clone()) {
                pending.push_back(uri.clone());
            }
        }
        backend.publish_tempest_scope(&node.uri, &node.source, &scope);
        scopes.insert(uri, scope);
    }
    scopes
}

fn arguments(backend: &Backend, node: &Template, call: &Call, parent: &Scope) -> Scope {
    let surrounding = call
        .enclosing
        .and_then(|start| parent.regions.get(&start))
        .unwrap_or(parent);
    let mut scope = Scope {
        vars: surrounding.globals.clone(),
        globals: surrounding.globals.clone(),
        bound_this: Some(surrounding.this_type().unwrap_or_else(PhpType::object)),
        ..Scope::default()
    };
    let (php, map) = super::preprocessor::preprocess_with_scope(&node.source, Some(parent));
    let file = backend.file_context(&node.uri);
    let loaders = backend.file_loaders(&file);
    let _engine = crate::type_engine::call_resolution::activate_type_engine_caches();
    let _parse = crate::parser::with_parse_cache(&php);
    let mut attrs = Vec::new();
    crate::parser::with_parsed_program(&php, "tempest_component_arguments", |program, content| {
        let mut expressions = PhpExpressions::default();
        Expressions.walk_program(program, &mut expressions);
        for arg in &call.args {
            let ty = if !arg.expression {
                PhpType::literal_string_value(
                    arg.value
                        .as_ref()
                        .map_or("", |range| &node.source[range.clone()]),
                )
            } else if let Some(range) = arg
                .value
                .as_ref()
                .filter(|range| !node.source[(*range).clone()].trim().is_empty())
            {
                let start = range.start + node.source[range.clone()].len()
                    - node.source[range.clone()].trim_start().len();
                let position = map.blade_to_php(offset_to_position(&node.source, start));
                let offset = position_to_offset(content, position);
                expressions
                    .values
                    .get(&{ offset })
                    .map(|expr| super::view_data::expression_type(expr, content, &loaders))
                    .unwrap_or_else(PhpType::mixed)
            } else {
                PhpType::true_()
            };
            attrs.push(ty.clone());
            if variable_name(&arg.name) {
                scope.vars.insert(arg.name.clone(), ty);
            }
        }
        if let Some(range) = &call.foreach {
            let at = position_to_offset(
                content,
                map.blade_to_php(offset_to_position(&node.source, call.start)),
            );
            let ctx = loaders.var_ctx_at(content, at);
            let start = position_to_offset(
                content,
                map.blade_to_php(offset_to_position(&node.source, range.start)),
            );
            let end = position_to_offset(
                content,
                map.blade_to_php(offset_to_position(&node.source, range.end)),
            );
            for var in expressions.loop_variables {
                if (start..end).contains(&var.span.start.offset)
                    && let Some(ty) =
                        crate::type_engine::variable::resolution::resolve_variable_php_type(
                            ctx.var_query(bytes_to_str(var.name), at),
                        )
                {
                    let name = bytes_to_str(var.name).trim_start_matches('$');
                    let ty = qualify(&ty);
                    scope.vars.insert(name.to_owned(), ty.clone());
                    scope.globals.insert(name.to_owned(), ty);
                }
            }
        }
    });
    scope.vars.insert(
        "attributes".into(),
        PhpType::generic(
            "\\Tempest\\Support\\Arr\\ImmutableArray",
            vec![
                PhpType::string(),
                crate::blade::call_site_inference::join_call_site_types(attrs),
            ],
        ),
    );
    scope.vars.insert(
        "slots".into(),
        PhpType::generic(
            "\\Tempest\\Support\\Arr\\ImmutableArray",
            vec![PhpType::string(), PhpType::parse("\\Tempest\\View\\Slot")],
        ),
    );
    scope
}

fn slot_scopes(backend: &Backend, node: &Template, data: &Scope) -> BTreeMap<String, Scope> {
    if node.outlets.is_empty() {
        return BTreeMap::new();
    }
    let (php, map) = super::preprocessor::preprocess_with_scope(&node.source, Some(data));
    let file = backend.file_context(&node.uri);
    let loaders = backend.file_loaders(&file);
    let _engine = crate::type_engine::call_resolution::activate_type_engine_caches();
    let _parse = crate::parser::with_parse_cache(&php);
    crate::parser::with_parsed_program(&php, "tempest_slot_scope", |program, content| {
        let mut names = BTreeSet::new();
        Variables.walk_program(program, &mut names);
        let mut result: BTreeMap<String, Scope> = BTreeMap::new();
        for (name, outlets) in &node.outlets {
            for start in outlets {
                let offset = position_to_offset(
                    content,
                    map.blade_to_php(offset_to_position(&node.source, *start)),
                );
                let mut ctx = loaders.var_ctx_at(content, offset);
                // This is a particular invocation's scope. The published
                // symbol map describes the union of all invocations.
                ctx.file = ctx.file.with_uri(None);
                let mut scope = Scope {
                    globals: data.globals.clone(),
                    bound_this: data.this_type(),
                    ..Scope::default()
                };
                for name in &names {
                    if let Some(ty) =
                        crate::type_engine::variable::resolution::resolve_variable_php_type(
                            ctx.var_query(name, offset),
                        )
                    {
                        scope
                            .vars
                            .insert(name.trim_start_matches('$').into(), qualify(&ty));
                    }
                }
                result.entry(name.clone()).or_default().merge(scope);
            }
        }
        result
    })
}

fn qualify(ty: &PhpType) -> PhpType {
    ty.resolve_names(&|name| format!("\\{}", name.trim_start_matches('\\')))
}

/// A supplied slot reads variables at the component's outlet. Record the
/// actual reads, including captures by user closures, using the same scope
/// collector as unused-variable diagnostics. Unrelated locals stay unused.
fn slot_reads(
    caller: &Template,
    target: &Template,
    call: &Call,
    parent: &Scope,
    slots: &BTreeMap<String, Scope>,
) -> BTreeMap<usize, BTreeSet<String>> {
    let mut reads: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    if call.slots.is_empty() {
        return reads;
    }
    let (php, map) = super::preprocessor::preprocess_with_scope(&caller.source, Some(parent));
    crate::parser::with_parsed_program(&php, "tempest_slot_reads", |program, content| {
        let scope = crate::scope_collector::build_scope_map_for_offset(
            program.statements.as_slice(),
            content.len() as u32 - 2,
            content.len() as u32,
        );
        for (start, name) in &call.slots {
            if !parent.regions.contains_key(start) {
                continue;
            }
            let Some(bindings) = slots.get(name) else {
                continue;
            };
            let offset = position_to_offset(
                content,
                map.blade_to_php(offset_to_position(&caller.source, *start)),
            );
            let Some(frame) = scope.frames.iter().find(|frame| frame.start > offset) else {
                continue;
            };
            for access in &scope.accesses {
                if access.offset < frame.start
                    || access.offset >= frame.end
                    || !matches!(
                        access.kind,
                        crate::scope_collector::AccessKind::Read
                            | crate::scope_collector::AccessKind::ReadWrite
                    )
                    || frame
                        .nested_frames_at(&scope.frames, access.offset)
                        .any(|nested| {
                            matches!(
                                nested.kind,
                                crate::scope_collector::FrameKind::Closure
                                    | crate::scope_collector::FrameKind::Function
                                    | crate::scope_collector::FrameKind::Method
                            ) || nested.parameters.contains(&access.name)
                        })
                {
                    continue;
                }
                let variable = access.name.trim_start_matches('$');
                if bindings.vars.contains_key(variable) {
                    for outlet in &target.outlets[name] {
                        reads
                            .entry(*outlet)
                            .or_default()
                            .insert(variable.to_owned());
                    }
                }
            }
        }
    });
    reads
}

#[derive(Default)]
struct PhpExpressions<'ast, 'arena> {
    values: BTreeMap<u32, &'ast Expression<'arena>>,
    loop_variables: Vec<&'ast DirectVariable<'arena>>,
}

struct Expressions;
impl<'ast, 'arena> Walker<'ast, 'arena, PhpExpressions<'ast, 'arena>> for Expressions {
    fn walk_in_expression(
        &self,
        expr: &'ast Expression<'arena>,
        expressions: &mut PhpExpressions<'ast, 'arena>,
    ) {
        expressions
            .values
            .entry(expr.span().start.offset)
            .or_insert(expr);
    }

    fn walk_in_foreach(
        &self,
        loop_: &'ast Foreach<'arena>,
        expressions: &mut PhpExpressions<'ast, 'arena>,
    ) {
        for target in loop_
            .target
            .key()
            .into_iter()
            .chain(std::iter::once(loop_.target.value()))
        {
            if let Expression::Variable(Variable::Direct(var)) = target {
                expressions.loop_variables.push(var);
            }
        }
    }
}

struct Variables;
impl<'ast, 'arena> Walker<'ast, 'arena, BTreeSet<&'ast str>> for Variables {
    fn walk_in_direct_variable(
        &self,
        var: &'ast DirectVariable<'arena>,
        names: &mut BTreeSet<&'ast str>,
    ) {
        let name = bytes_to_str(var.name);
        if variable_name(name.trim_start_matches('$')) {
            names.insert(name);
        }
    }
}
