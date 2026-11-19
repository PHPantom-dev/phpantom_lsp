//! Literal URI transformations in a project's RouteDecorator implementations.

use std::borrow::Cow;
use std::ops::Deref;

use mago_span::HasSpan;
use mago_syntax::{cst::*, walker::Walker};

use crate::Backend;
use crate::atom::{Atom, AtomMap, atom, bytes_to_str};
use crate::class_lookup::{Fqn, is_subtype_of};
use crate::types::FileContext;

use super::routes::{RouteAttribute, RouteDecorator};

/// The indexed declaration with the URI its decorators produce for this request.
pub(super) struct ResolvedRoute<'a> {
    declaration: &'a RouteAttribute,
    /// The final URI, borrowing the indexed literal when no changes are needed.
    pub(super) path: Cow<'a, str>,
}

impl Deref for ResolvedRoute<'_> {
    type Target = RouteAttribute;

    fn deref(&self) -> &Self::Target {
        self.declaration
    }
}

#[derive(Clone, Copy)]
struct Prefix {
    value: Atom,
    /// Prefix::decorate joins path segments; a PHP concatenation preserves slashes.
    join: bool,
}

/// Resolve each attribute class once, without retaining stale bodies across edits.
#[derive(Default)]
pub(super) struct Decorators {
    prefixes: AtomMap<Option<Prefix>>,
}

impl Decorators {
    /// Apply the indexed attributes without copying the declaration's other data.
    pub(super) fn resolve<'a>(
        &mut self,
        backend: &Backend,
        declaration: &'a RouteAttribute,
    ) -> ResolvedRoute<'a> {
        if !declaration.decorators.iter().any(|decorator| {
            matches!(decorator, RouteDecorator::Custom(class)
                if self.prefix(backend, *class).is_some())
        }) {
            return ResolvedRoute {
                declaration,
                path: Cow::Borrowed(&declaration.path),
            };
        }
        let mut path = Cow::Borrowed(
            declaration
                .original_path
                .as_deref()
                .unwrap_or(&declaration.path),
        );
        for decorator in &declaration.decorators {
            let prefix = match decorator {
                RouteDecorator::Prefix(value) => Some(Prefix {
                    value: *value,
                    join: true,
                }),
                RouteDecorator::Custom(class) => self.prefix(backend, *class),
            };
            if let Some(prefix) = prefix {
                path = Cow::Owned(if prefix.join {
                    format!(
                        "{}/{}",
                        prefix.value.trim_end_matches('/'),
                        path.trim_start_matches('/')
                    )
                } else {
                    format!("{}{path}", prefix.value)
                });
            }
        }
        if !path.starts_with('/') {
            path.to_mut().insert(0, '/');
        }
        ResolvedRoute { declaration, path }
    }

    fn prefix(&mut self, backend: &Backend, class: Atom) -> Option<Prefix> {
        *self
            .prefixes
            .entry(class)
            .or_insert_with(|| read_prefix(backend, &class))
    }
}

fn read_prefix(backend: &Backend, name: &str) -> Option<Prefix> {
    let loader = backend.resolved_class_loader_with(&[]);
    let class = loader(Fqn::new(name))?;
    if !is_subtype_of(&class, "Tempest\\Router\\RouteDecorator", &loader) {
        return None;
    }
    let declaring = crate::hover::find_declaring_class(
        &class,
        "decorate",
        &crate::hover::MemberKindForOrigin::Method,
        &loader,
    );
    let method = declaring.get_method_ci("decorate")?;
    let uri = backend.class_declaration_uri(&declaring.fqn())?;
    let content = backend.get_file_content_arc(&uri)?;
    let file = backend.file_context(&uri);
    let _parse = crate::parser::with_parse_cache_arc(content.clone());
    crate::parser::with_parsed_program(&content, "tempest_route_decorator", |program, _| {
        let mut found = (method.name_offset, None);
        MethodPrefix { file: &file }.walk_program(program, &mut found);
        found.1
    })
}

struct MethodPrefix<'a> {
    file: &'a FileContext,
}

impl<'ast, 'arena> Walker<'ast, 'arena, (u32, Option<Prefix>)> for MethodPrefix<'_> {
    fn walk_in_method(&self, method: &'ast Method<'arena>, found: &mut (u32, Option<Prefix>)) {
        if method.name.span.start.offset == found.0 {
            found.1 = method_prefix(method, self.file);
        }
    }
}

fn method_prefix(method: &Method<'_>, file: &FileContext) -> Option<Prefix> {
    let route = method.parameter_list.parameters.first()?.variable.name;
    let MethodBody::Concrete(body) = &method.body else {
        return None;
    };
    match body.statements.as_slice() {
        [Statement::Return(ret)] => delegated_prefix(ret.value?, route, file),
        [Statement::Expression(statement), Statement::Return(ret)]
            if ret.value.is_some_and(|expr| is_variable(expr, route)) =>
        {
            let Expression::Assignment(assignment) = statement.expression else {
                return None;
            };
            if !assignment.operator.is_assign() || !is_route_uri(assignment.lhs, route) {
                return None;
            }
            let Expression::Binary(binary) = crate::parser::unwrap_parens(assignment.rhs) else {
                return None;
            };
            if !binary.operator.is_concatenation() || !is_route_uri(binary.rhs, route) {
                return None;
            }
            Some(Prefix {
                value: string_literal(binary.lhs)?,
                join: false,
            })
        }
        _ => None,
    }
}

fn delegated_prefix(expr: &Expression<'_>, route: &[u8], file: &FileContext) -> Option<Prefix> {
    let Expression::Call(Call::Method(call)) = crate::parser::unwrap_parens(expr) else {
        return None;
    };
    if !matches!(&call.method, ClassLikeMemberSelector::Identifier(name) if name.value.eq_ignore_ascii_case(b"decorate"))
        || !is_variable(single_argument(&call.argument_list, b"route")?, route)
    {
        return None;
    }
    let Expression::Instantiation(new) = crate::parser::unwrap_parens(call.object) else {
        return None;
    };
    let Expression::Identifier(name) = new.class else {
        return None;
    };
    if !file
        .resolve_name_at(bytes_to_str(name.value()), name.span().start.offset)
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("Tempest\\Router\\Prefix")
    {
        return None;
    }
    Some(Prefix {
        value: string_literal(single_argument(new.argument_list.as_ref()?, b"prefix")?)?,
        join: true,
    })
}

fn single_argument<'a>(args: &'a ArgumentList<'a>, name: &[u8]) -> Option<&'a Expression<'a>> {
    match args.arguments.as_slice() {
        [Argument::Positional(arg)] if arg.ellipsis.is_none() => Some(arg.value),
        [Argument::Named(arg)] if arg.name.value == name => Some(arg.value),
        _ => None,
    }
}

fn string_literal(expr: &Expression<'_>) -> Option<Atom> {
    let Expression::Literal(Literal::String(literal)) = crate::parser::unwrap_parens(expr) else {
        return None;
    };
    Some(atom(crate::atom::literal_bytes_to_str(literal.value?)?))
}

fn is_variable(expr: &Expression<'_>, name: &[u8]) -> bool {
    matches!(crate::parser::unwrap_parens(expr), Expression::Variable(Variable::Direct(var)) if var.name == name)
}

fn is_route_uri(expr: &Expression<'_>, route: &[u8]) -> bool {
    matches!(crate::parser::unwrap_parens(expr), Expression::Access(Access::Property(access))
        if is_variable(access.object, route)
        && matches!(&access.property, ClassLikeMemberSelector::Identifier(name) if name.value == b"uri"))
}

#[cfg(test)]
mod tests;
