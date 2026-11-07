//! The object behind Tempest's renderer proxy and its explicitly stored data.

use std::collections::BTreeMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::Backend;
use crate::atom::{atom, bytes_to_str};
use crate::php_type::PhpType;

use super::inference::Scope;
use super::view_data::{arguments, expression_type, path_definition, shape_vars};

#[derive(Clone)]
pub(super) struct CustomView {
    /// Canonical URI of the declared view path.
    pub(super) target: String,
    /// Explicit data and the custom object exposed through `$this`.
    pub(super) scope: Scope,
}

/// Find indexed View implementations without resolving unrelated classes.
pub(super) fn collect(backend: &Backend) -> BTreeMap<String, CustomView> {
    // Follow the indexed ancestry graph without loading unrelated frameworks'
    // parents and traits just because this workspace contains one view.
    let mut candidates: Vec<_> = backend
        .symbols
        .uri_classes_index
        .read()
        .iter()
        .flat_map(|(uri, classes)| {
            classes
                .iter()
                .filter(|class| !class.interfaces.is_empty() || class.parent_class.is_some())
                .map(|class| (uri.clone(), class.clone()))
        })
        .collect();
    let mut known = vec![atom("Tempest\\View\\View")];
    let mut files: BTreeMap<String, Vec<std::sync::Arc<crate::types::ClassInfo>>> = BTreeMap::new();
    loop {
        let before = candidates.len();
        candidates.retain(|(uri, class)| {
            let matches = class
                .interfaces
                .iter()
                .chain(class.parent_class.iter())
                .any(|parent| known.iter().any(|name| name.eq_ignore_ascii_case(parent)));
            if matches {
                known.push(class.fqn());
                files.entry(uri.clone()).or_default().push(class.clone());
            }
            !matches
        });
        if candidates.len() == before {
            break;
        }
    }
    let mut result = BTreeMap::new();
    let mut inherited = Vec::new();
    for (uri, classes) in files {
        let file = backend.file_context(&uri);
        let loaders = backend.file_loaders(&file);
        let Some(content) = backend.analysable_content(&uri) else {
            continue;
        };
        let _engine = crate::type_engine::call_resolution::activate_type_engine_caches();
        let _parse = crate::parser::with_parse_cache(&content);
        crate::parser::with_parsed_program(&content, "tempest_custom_views", |program, content| {
            let mut values = Vec::new();
            ViewValues.walk_program(program, &mut values);
            for class in classes {
                let mut path = None;
                let mut scope = Scope::default();
                for value in &values {
                    let offset = value.offset();
                    if offset < class.start_offset || offset >= class.end_offset {
                        continue;
                    }
                    // A nested anonymous class has its own $this.
                    if file.classes.iter().any(|inner| {
                        inner.start_offset > class.start_offset
                            && inner.start_offset <= offset
                            && offset < inner.end_offset
                    }) {
                        continue;
                    }
                    match value {
                        Value::Property("path", expr) => {
                            path = path_definition(backend, &uri, expr, content, &loaders);
                        }
                        Value::Property("data", expr) => {
                            shape_vars(&expression_type(expr, content, &loaders), &mut scope.vars)
                        }
                        Value::Data(args) => {
                            arguments(args, false, content, &loaders, &mut scope.vars)
                        }
                        _ => {}
                    }
                }
                let fqn = class.fqn().to_string();
                if let Some(path) = path {
                    scope.globals = scope.vars.clone();
                    bind_this(backend, &class, &mut scope);
                    result.insert(
                        fqn,
                        CustomView {
                            target: path.uri.to_string(),
                            scope,
                        },
                    );
                } else if let Some(parent) = class.parent_class {
                    inherited.push((class.clone(), parent.to_string()));
                }
            }
        });
    }
    // Parent declarations may be in files visited later. Each pass either
    // resolves a class or stops; malformed inheritance cycles cannot spin.
    loop {
        let before = inherited.len();
        inherited.retain(|(class, parent)| {
            let Some(mut view) = result.get(parent).cloned() else {
                return true;
            };
            bind_this(backend, class, &mut view.scope);
            result.insert(class.fqn().to_string(), view);
            false
        });
        if inherited.len() == before {
            break;
        }
    }
    result
}

/// Declared View properties take precedence over same-named data entries.
pub(super) fn bind_this(backend: &Backend, class: &crate::types::ClassInfo, scope: &mut Scope) {
    let loader = backend.resolved_class_loader_with(&[]);
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        class,
        &loader,
        Some(&backend.resolved_class_cache),
    );
    let mut fallback = Scope::default();
    for (name, ty) in &scope.vars {
        if !merged.has_property(name) {
            fallback.vars.insert(name.clone(), ty.clone());
        }
    }
    let class = PhpType::named(atom(&format!("\\{}", class.fqn())));
    scope.bound_this = Some(match fallback.this_type() {
        Some(data) => PhpType::intersection(vec![class, data]),
        None => class,
    });
}

enum Value<'ast, 'arena> {
    Property(&'ast str, &'ast Expression<'arena>),
    Data(&'ast ArgumentList<'arena>),
}

impl Value<'_, '_> {
    fn offset(&self) -> u32 {
        match self {
            Self::Property(_, expr) => expr.span().start.offset,
            Self::Data(args) => args.span().start.offset,
        }
    }
}

fn is_this(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Variable(Variable::Direct(var)) if var.name == b"$this")
}

struct ViewValues;

impl<'ast, 'arena> Walker<'ast, 'arena, Vec<Value<'ast, 'arena>>> for ViewValues {
    fn walk_in_assignment(
        &self,
        assignment: &'ast Assignment<'arena>,
        values: &mut Vec<Value<'ast, 'arena>>,
    ) {
        if assignment.operator.is_assign()
            && let Expression::Access(Access::Property(access)) = assignment.lhs
            && is_this(access.object)
            && let ClassLikeMemberSelector::Identifier(name) = &access.property
        {
            values.push(Value::Property(bytes_to_str(name.value), assignment.rhs));
        }
    }

    fn walk_in_property_concrete_item(
        &self,
        item: &'ast PropertyConcreteItem<'arena>,
        values: &mut Vec<Value<'ast, 'arena>>,
    ) {
        values.push(Value::Property(
            bytes_to_str(item.variable.name).trim_start_matches('$'),
            item.value,
        ));
    }

    fn walk_in_method_call(
        &self,
        call: &'ast MethodCall<'arena>,
        values: &mut Vec<Value<'ast, 'arena>>,
    ) {
        if is_this(call.object)
            && matches!(&call.method, ClassLikeMemberSelector::Identifier(name) if name.value.eq_ignore_ascii_case(b"data"))
        {
            values.push(Value::Data(&call.argument_list));
        }
    }
}
