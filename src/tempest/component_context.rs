//! Component arguments and slot bodies use different PHP scopes.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ops::Range;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::php_type::{PhpType, TypeKind};
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
    target: Option<String>,
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
            let target = components.get(tag.name);
            if tag.name == "x-component" || target.is_some() {
                let index = calls.len();
                calls.push(Call {
                    start: tag.span.start,
                    target: target
                        .filter(|_| tag.name != "x-component")
                        .map(|path| canonical_uri(&crate::util::path_to_uri(path))),
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
                        .map(|attr| {
                            let mut name = crate::blade::component_tags::camel_case_attr_name(
                                attr.name.trim_start_matches(':'),
                            );
                            if let Some(first) = name.get_mut(..1) {
                                first.make_ascii_lowercase();
                            }
                            Argument {
                                name,
                                value: attr.value.clone(),
                                expression: attr.name.starts_with(':'),
                            }
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
    let aliases: HashMap<_, _> = backend
        .open_files
        .read()
        .keys()
        .filter(|uri| super::is_view(uri))
        .map(|uri| (canonical_uri(uri), uri.clone()))
        .collect();
    let mut nodes = BTreeMap::new();
    let mut incoming: HashMap<String, BTreeMap<(String, usize), Scope>> = HashMap::new();
    let mut outgoing: HashMap<(String, usize), BTreeSet<String>> = HashMap::new();
    let mut scopes = roots.clone();
    let mut queued: BTreeSet<_> = templates.iter().map(|uri| canonical_uri(uri)).collect();
    let mut pending: VecDeque<_> = queued.iter().cloned().collect();
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
        let Some(node) = load_template(backend, &uri, &aliases, &components, &mut nodes) else {
            continue;
        };
        let mut scope = roots.get(&uri).cloned().unwrap_or_default();
        for data in incoming
            .get(&uri)
            .into_iter()
            .flat_map(|calls| calls.values())
        {
            scope.merge(data.clone());
        }
        if let Some(previous) = scopes.get(&uri) {
            scope.regions = previous.regions.clone();
        }
        backend.publish_tempest_scope(&node.uri, &node.source, &scope);
        let mut regions: BTreeMap<usize, Scope> = BTreeMap::new();
        for (index, call) in node.calls.iter().enumerate() {
            let data = arguments(backend, &node, call, &scope);
            let targets: BTreeSet<_> = if let Some(target) = &call.target {
                std::iter::once(target.clone()).collect()
            } else {
                data.vars
                    .get("is")
                    .into_iter()
                    .flat_map(component_names)
                    .filter_map(|name| components.get(name.as_ref()))
                    .map(|path| canonical_uri(&crate::util::path_to_uri(path)))
                    .collect()
            };
            let key = (uri.clone(), index);
            // A selector may change as caller data arrives. Remove its old
            // contribution so a previously selected component loses stale types.
            for previous in outgoing.get(&key).into_iter().flatten() {
                if !targets.contains(previous) {
                    incoming.get_mut(previous).unwrap().remove(&key);
                    if queued.insert(previous.clone()) {
                        pending.push_back(previous.clone());
                    }
                }
            }
            let mut resolved = BTreeSet::new();
            for target_uri in targets {
                let Some(target) =
                    load_template(backend, &target_uri, &aliases, &components, &mut nodes)
                else {
                    continue;
                };
                let mut data = data.clone();
                let slots = slot_scopes(backend, &target, &data);
                data.slot_reads = slot_reads(&node, &target, call, &scope, &slots);
                let callers = incoming.entry(target_uri.clone()).or_default();
                if callers.get(&key) != Some(&data) {
                    callers.insert(key.clone(), data.clone());
                    if queued.insert(target_uri.clone()) {
                        pending.push_back(target_uri.clone());
                    }
                }
                for (start, name) in &call.slots {
                    regions
                        .entry(*start)
                        .or_default()
                        .merge(slots.get(name).cloned().unwrap_or_else(|| data.clone()));
                }
                resolved.insert(target_uri);
            }
            for (start, _) in &call.slots {
                regions.entry(*start).or_insert_with(|| data.clone());
            }
            outgoing.insert(key, resolved);
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

fn load_template(
    backend: &Backend,
    key: &str,
    aliases: &HashMap<String, String>,
    components: &BTreeMap<String, std::path::PathBuf>,
    nodes: &mut BTreeMap<String, Arc<Template>>,
) -> Option<Arc<Template>> {
    if let Some(node) = nodes.get(key) {
        return Some(Arc::clone(node));
    }
    let uri = aliases.get(key).map_or(key, String::as_str);
    let source = backend.get_file_content_arc(uri)?;
    let node = Arc::new(Template::read(uri.to_owned(), source, components));
    if backend.symbol_map_for(uri).is_none() {
        backend.update_ast(uri, &node.source);
    }
    nodes.insert(key.to_owned(), Arc::clone(&node));
    Some(node)
}

/// Only literal alternatives identify a component; an arbitrary string must
/// not send data to every registered component.
pub(super) fn component_names(ty: &PhpType) -> Vec<std::borrow::Cow<'_, str>> {
    fn collect<'a>(ty: &'a PhpType, names: &mut Vec<std::borrow::Cow<'a, str>>) {
        match ty.kind() {
            TypeKind::Nullable(inner) => collect(inner, names),
            TypeKind::Union(members) => {
                for member in members {
                    collect(member, names);
                }
            }
            _ => {
                if let Some(name) = ty.as_literal().and_then(|value| value.string_content()) {
                    names.push(name);
                }
            }
        }
    }
    let mut names = Vec::new();
    collect(ty, &mut names);
    names
}

/// Read a selector through the same PHP lowering and expression engine as
/// component arguments. Navigation only claims standalone string literals;
/// variables and constants retain their ordinary PHP definitions.
pub(super) fn attribute_type(
    backend: &Backend,
    uri: &str,
    source: &str,
    range: &Range<usize>,
    literal_only: bool,
) -> Option<PhpType> {
    let php = backend.analysable_content(uri)?;
    let map = backend.template_source_map(uri)?;
    let file = backend.file_context(uri);
    let loaders = backend.file_loaders(&file);
    let _engine = crate::type_engine::call_resolution::activate_type_engine_caches();
    crate::parser::with_parsed_program(&php, "tempest_component_selector", |program, content| {
        let mut expressions = PhpExpressions::default();
        Expressions.walk_program(program, &mut expressions);
        let expr = expressions.at(source, range, &map, content)?;
        if literal_only && !matches!(expr, Expression::Literal(Literal::String(_))) {
            return None;
        }
        Some(super::view_data::expression_type(expr, content, &loaders))
    })
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
                expressions
                    .at(&node.source, range, &map, content)
                    .map(|expr| super::view_data::expression_type(expr, content, &loaders))
                    .unwrap_or_else(PhpType::mixed)
            } else {
                PhpType::true_()
            };
            if call.target.is_none() {
                forward_dynamic_argument(&ty, &arg.name, &mut scope, &mut attrs);
            } else {
                attrs.push(ty.clone());
                if variable_name(&arg.name) {
                    scope.vars.insert(arg.name.clone(), ty);
                }
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
    if call.target.is_none() {
        scope.bound_this = Some(
            Scope {
                vars: scope.globals.clone(),
                ..Scope::default()
            }
            .this_type()
            .unwrap_or_else(PhpType::object),
        );
    }
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

/// Tempest's dynamic wrapper rebuilds an HTML tag: scalar attributes become
/// strings, false/null disappear, and objects/arrays enter the new view's data.
fn forward_dynamic_argument(ty: &PhpType, name: &str, scope: &mut Scope, attrs: &mut Vec<PhpType>) {
    #[derive(Default)]
    struct Forwarded {
        attributes: Vec<PhpType>,
        globals: Vec<PhpType>,
        omitted: bool,
    }
    impl Forwarded {
        fn push(&mut self, ty: &PhpType) {
            match ty.kind() {
                TypeKind::Union(members) => {
                    for member in members {
                        self.push(member);
                    }
                }
                TypeKind::Nullable(inner) => {
                    self.omitted = true;
                    self.push(inner);
                }
                _ if ty.is_null() || ty.is_false() => self.omitted = true,
                _ if ty.is_true() || ty.is_bool() => {
                    self.omitted |= !ty.is_true();
                    self.attributes.push(PhpType::literal_string_value(""));
                }
                _ if ty.is_string_subtype() => self.attributes.push(ty.clone()),
                _ if ty.is_int_subtype() || ty.is_float_subtype() => {
                    self.attributes.push(
                        ty.as_literal()
                            .and_then(|value| value.parse_i64())
                            .map(|value| PhpType::literal_string_value(value.to_string()))
                            .unwrap_or_else(PhpType::numeric_string),
                    );
                }
                _ if !ty.is_mixed()
                    && (ty.is_object_like()
                        || ty.is_array_like()
                        || ty.is_closure()
                        || matches!(ty.kind(), TypeKind::Intersection(_))) =>
                {
                    self.globals.push(ty.clone())
                }
                _ => {
                    self.attributes.push(PhpType::mixed());
                    self.globals.push(PhpType::mixed());
                }
            }
        }
    }
    let mut forwarded = Forwarded::default();
    forwarded.push(ty);
    attrs.extend(forwarded.attributes.iter().cloned());
    if !variable_name(name) {
        return;
    }
    let inherited = scope.globals.get(name);
    let mut values = forwarded.attributes.clone();
    values.extend(forwarded.globals.iter().cloned());
    if forwarded.omitted {
        values.extend(inherited.cloned());
    }
    if !values.is_empty() {
        scope.vars.insert(name.to_owned(), PhpType::union(values));
    }
    if !forwarded.globals.is_empty() {
        if forwarded.omitted || !forwarded.attributes.is_empty() {
            forwarded.globals.extend(inherited.cloned());
        }
        scope
            .globals
            .insert(name.to_owned(), PhpType::union(forwarded.globals));
    }
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

impl<'ast, 'arena> PhpExpressions<'ast, 'arena> {
    fn at(
        &self,
        source: &str,
        range: &Range<usize>,
        map: &crate::template::source_map::TemplateSourceMap,
        content: &str,
    ) -> Option<&'ast Expression<'arena>> {
        let value = &source[range.clone()];
        if value.trim().is_empty() {
            return None;
        }
        let start = range.start + value.len() - value.trim_start().len();
        let position = map.blade_to_php(offset_to_position(source, start));
        self.values
            .get(&{ position_to_offset(content, position) })
            .copied()
    }
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
