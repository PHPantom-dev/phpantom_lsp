//! Dynamic attribute arrays passed to models with Eloquent's default guards.

use mago_span::HasSpan;
use mago_syntax::cst::{ArgumentList, Call, ClassLikeMemberSelector, Expression, Literal, Program};
use mago_syntax::walker::Walker;
use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity};

use super::helpers::{FileDiagnosticContext, make_diagnostic};
use crate::Backend;
use crate::atom::bytes_to_str;
use crate::call_args::{argument_at, argument_for_parameter};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of_named};
use crate::inheritance::find_declaring_ancestor;
use crate::parser::{array_literal_elements, unwrap_parens, with_parsed_program};
use crate::php_type::TypeKind;
use crate::resolution::FileLoaders;
use crate::symbol_map::SymbolMap;
use crate::type_engine::resolver::resolve_target_classes;
use crate::types::{AccessKind, ClassInfo, ResolvedType, model_declaration as decl};
use crate::virtual_members::laravel::{
    ELOQUENT_BUILDER_FQN, ELOQUENT_MODEL_FQN, ELOQUENT_RELATION_FQN, model_of_receiver,
};

fn lookup_or_create(method: &str) -> bool {
    ["updateOrCreate", "firstOrCreate", "firstOrNew"]
        .iter()
        .any(|name| method.eq_ignore_ascii_case(name))
}

fn instance_only(method: &str) -> bool {
    method.eq_ignore_ascii_case("fill") || method.eq_ignore_ascii_case("update")
}

fn assigns(method: &str) -> bool {
    instance_only(method) || method.eq_ignore_ascii_case("create") || lookup_or_create(method)
}

/// Whether this file can contain a call affected by mass-assignment guards.
pub(super) fn mentions_assignment(map: &SymbolMap) -> bool {
    map.member_access_indices.keys().any(|name| assigns(name))
}

struct NamedMethodCalls;

impl<'ast, 'arena: 'ast, F> Walker<'ast, 'arena, F> for NamedMethodCalls
where
    F: FnMut(&'ast Expression<'arena>, &'arena str, AccessKind, &'ast ArgumentList<'arena>),
{
    fn walk_in_call(&self, node: &'ast Call<'arena>, visit: &mut F) {
        let (object, selector, access, args) = match node {
            Call::Method(c) => (c.object, &c.method, AccessKind::Arrow, &c.argument_list),
            Call::NullSafeMethod(c) => (c.object, &c.method, AccessKind::Arrow, &c.argument_list),
            Call::StaticMethod(c) => (
                c.class,
                &c.method,
                AccessKind::DoubleColon,
                &c.argument_list,
            ),
            _ => return,
        };
        if let ClassLikeMemberSelector::Identifier(method) = selector {
            visit(object, bytes_to_str(method.value), access, args);
        }
    }
}

fn visit_method_calls<'a>(
    program: &'a Program<'a>,
    mut visit: impl FnMut(&'a Expression<'a>, &'a str, AccessKind, &'a ArgumentList<'a>),
) {
    for statement in program.statements.iter() {
        NamedMethodCalls.walk_statement(statement, &mut visit);
    }
}

/// Both arrays feed the new model on the create branch. Report the call
/// once, on the first dynamic argument in parameter order.
fn dynamic_attributes<'a>(method: &str, args: &'a ArgumentList<'a>) -> Option<&'a Expression<'a>> {
    if !assigns(method) {
        return None;
    }
    let dynamic = |index, name: &[u8]| {
        let arg = argument_for_parameter(args, index, name)?;
        let expr = unwrap_parens(arg.value());
        (!arg.is_unpacked()
            && !matches!(expr, Expression::Literal(_))
            && array_literal_elements(expr).is_none())
        .then_some(arg.value())
    };
    dynamic(0, b"attributes").or_else(|| {
        if lookup_or_create(method) {
            dynamic(1, b"values")
        } else {
            None
        }
    })
}

fn declares_policy(class: &ClassInfo) -> bool {
    class.laravel().is_some_and(|meta| {
        meta.declared
            & (decl::FILLABLE
                | decl::GUARDED
                | decl::FILLABLE_ATTRIBUTE
                | decl::GUARDED_ATTRIBUTE
                | decl::UNGUARDED_ATTRIBUTE
                | decl::PIVOT)
            != 0
    }) || ["fillable", "guarded"].iter().any(|name| {
        class
            .get_property(name)
            .is_some_and(|property| !property.is_virtual)
    })
}

fn overrides(class: &ClassInfo, method: &str) -> bool {
    class.get_method(method).is_some_and(|m| !m.is_virtual)
}

fn guarded_model(
    receiver: &ResolvedType,
    instance: bool,
    method: &str,
    loader: &ResolvedClassLoader<'_>,
) -> Option<std::sync::Arc<ClassInfo>> {
    let model = model_of_receiver(receiver, loader)?;
    let direct = receiver
        .class_info
        .as_ref()
        .is_some_and(|c| c.fqn() == model.fqn());
    if let TypeKind::Generic(generic) = receiver.type_string.kind()
        && !direct
    {
        // Relation<TRelated, TParent> writes TRelated. The declaring model
        // must never stand in for an unknown related model.
        if instance_only(method)
            || generic.args.first()?.base_name()? != model.fqn().as_str()
            || ![ELOQUENT_BUILDER_FQN, ELOQUENT_RELATION_FQN]
                .iter()
                .any(|base| is_subtype_of_named(&receiver.type_string, base, loader))
        {
            return None;
        }
        let class = loader(Fqn::new(&generic.name))?;
        let framework = |c: &ClassInfo| c.fqn().starts_with("Illuminate\\Database\\Eloquent\\");
        let boundary = |c: &ClassInfo| framework(c) || overrides(c, method);
        if !framework(&class)
            && (overrides(&class, method)
                || find_declaring_ancestor(&class, loader, &boundary)
                    .is_some_and(|(_, c)| !framework(&c)))
        {
            return None;
        }
    } else if instance_only(method) && !instance {
        return None;
    }
    // Stop before Model, whose GuardsAttributes trait supplies the defaults.
    let custom = |c: &ClassInfo| {
        declares_policy(c) || overrides(c, "fill") || (direct && overrides(c, method))
    };
    let boundary = |c: &ClassInfo| c.fqn() == ELOQUENT_MODEL_FQN || custom(c);
    if model.is_abstract
        || model.fqn() == ELOQUENT_MODEL_FQN
        || custom(&model)
        || find_declaring_ancestor(&model, loader, &boundary)
            .is_some_and(|(_, c)| c.fqn() != ELOQUENT_MODEL_FQN)
    {
        return None;
    }
    Some(model)
}

impl Backend {
    /// Report dynamic attribute arrays on models that retain the framework defaults.
    pub(crate) fn collect_mass_assignment_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if !mentions_assignment(&ctx.symbol_map) {
            return;
        }
        let loaders = self.file_loaders(&ctx.file);
        let unguarded = std::cell::LazyCell::new(|| self.project_unguards_models(uri));
        with_parsed_program(content, "eloquent_mass_assignment", |program, content| {
            // Argument spans of `Model::unguarded(...)` calls. The walk is
            // pre-order, so an enclosing call is recorded before the calls
            // its callback makes.
            let mut unguarded_spans: Vec<(u32, u32)> = Vec::new();
            visit_method_calls(program, |object, method, access, args| {
                let span = object.span();
                if access == AccessKind::DoubleColon && method.eq_ignore_ascii_case("unguarded") {
                    if is_model_class(&loaders, content, object, access) {
                        let args = args.span();
                        unguarded_spans.push((args.start.offset, args.end.offset));
                    }
                    return;
                }
                let Some(value) = dynamic_attributes(method, args) else {
                    return;
                };
                let at = span.start.offset;
                if unguarded_spans
                    .iter()
                    .any(|&(start, end)| start <= at && at < end)
                {
                    return;
                }
                let subject = &content[span.start.offset as usize..span.end.offset as usize];
                let rctx = loaders.resolution_ctx_at(content, span.start.offset);
                let targets = resolve_target_classes(subject, access, &rctx);
                let [receiver] = targets.as_slice() else {
                    return;
                };
                let instance = access == AccessKind::Arrow
                    || (matches!(object, Expression::Self_(_) | Expression::Static(_))
                        && rctx.current_class.is_some()
                        && !ctx.symbol_map.is_in_static_method(span.start.offset));
                let Some(model) =
                    guarded_model(receiver, instance, method, loaders.resolved_class_loader())
                else {
                    return;
                };
                if *unguarded {
                    return;
                }
                let span = value.span();
                if let Some(range) = self.offset_range_to_lsp_range(
                    uri,
                    content,
                    span.start.offset as usize,
                    span.end.offset as usize,
                ) {
                    let message = format!(
                        "{} is totally guarded. Mass assigning a non-empty attribute array can throw MassAssignmentException.",
                        model.fqn()
                    );
                    out.push(make_diagnostic(
                        range,
                        DiagnosticSeverity::Warning,
                        "laravel_mass_assignment",
                        message,
                    ));
                }
            });
        });
    }

    fn project_unguards_models(&self, uri: &str) -> bool {
        let scope = self.project_scope_for(uri);
        self.user_file_symbol_maps_for_methods(&["unguard"])
            .into_iter()
            .filter(|(file, _)| scope.as_ref().is_none_or(|scope| scope.sees(file)))
            .filter(|(_, map)| mentions_unguard(map))
            .any(|(file, _)| {
                self.reference_file_content_arc(&file)
                    .is_some_and(|text| self.file_unguards_models(&file, &text))
            })
    }

    fn file_unguards_models(&self, file: &str, text: &str) -> bool {
        let context = self.file_context(file);
        let loaders = self.file_loaders(&context);
        with_parsed_program(text, "eloquent_unguard", |program, content| {
            let mut found = false;
            visit_method_calls(program, |object, method, access, args| {
                if found
                    || access != AccessKind::DoubleColon
                    || !method.eq_ignore_ascii_case("unguard")
                    || matches!(
                        argument_at(args, 0, b"state").map(unwrap_parens),
                        Some(Expression::Literal(Literal::False(_)))
                    )
                {
                    return;
                }
                found = is_model_class(&loaders, content, object, access);
            });
            found
        })
    }
}

/// Whether the class a static call is made on is an Eloquent model.
fn is_model_class(
    loaders: &FileLoaders<'_>,
    content: &str,
    object: &Expression<'_>,
    access: AccessKind,
) -> bool {
    let span = object.span();
    let subject = &content[span.start.offset as usize..span.end.offset as usize];
    let rctx = loaders.resolution_ctx_at(content, span.start.offset);
    resolve_target_classes(subject, access, &rctx)
        .iter()
        .any(|r| {
            is_subtype_of_named(
                &r.type_string,
                ELOQUENT_MODEL_FQN,
                loaders.resolved_class_loader(),
            )
        })
}

/// Candidate files for the project-wide guard setting, including during save invalidation.
pub(super) fn mentions_unguard(map: &SymbolMap) -> bool {
    map.member_access_indices
        .keys()
        .any(|name| name.eq_ignore_ascii_case("unguard"))
}
