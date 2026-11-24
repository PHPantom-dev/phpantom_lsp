//! Authorization gate abilities: `Gate::define()` declares one, and the
//! checks (`Gate::allows()`, `$user->can()`, `$this->authorize()`, `@can`,
//! `can:` middleware) name one, along with the model the check is about.

use super::*;
use crate::symbol_map::{MiddlewarePart, ReceiverClass, TypedReceiverSite};
use crate::virtual_members::laravel::helpers::string_literal_at_range;

/// What a recognised ability span is doing with the name it holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::symbol_map::extraction) enum AbilityRole {
    /// `Gate::define('name', …)` declares the ability, so the span is a
    /// declaration site rather than a use of one.
    Definition,
    /// A check that has to name a real ability.
    Check,
    /// `Gate::has('name')` asks whether the ability is registered at all, so
    /// a name that is not registered is the question rather than a mistake.
    Existence,
}

/// Methods on Laravel's `Gate` whose first argument is an ability name (or a
/// list of them), and whose second is the model the check is about.
///
/// Compared case-insensitively without lowercasing into a `String`: this runs
/// for every method call in every file.
pub(crate) fn is_gate_check_method(member_name: &str) -> bool {
    const GATE_CHECK_METHODS: &[&str] = &[
        "allows",
        "denies",
        "check",
        "any",
        "none",
        "authorize",
        "inspect",
        "has",
    ];
    GATE_CHECK_METHODS
        .iter()
        .any(|name| member_name.eq_ignore_ascii_case(name))
}

/// What a `Gate` method does with the ability it is handed.
///
/// `has()` is the framework's own "is this ability registered?" predicate, so
/// a name it does not find is the answer the call was written to get rather
/// than a typo.  The span is still emitted — completion, hover, and
/// go-to-definition all work inside it — but it is not judged.
pub(in crate::symbol_map::extraction) fn gate_ability_role(member_name: &str) -> AbilityRole {
    if member_name.eq_ignore_ascii_case("has") {
        AbilityRole::Existence
    } else {
        AbilityRole::Check
    }
}

/// Whether a static-call subject names Laravel's `Gate` facade.
pub(in crate::symbol_map::extraction) fn is_gate_facade(clean_subject: &str) -> bool {
    matches_laravel_facade(clean_subject, "Gate")
}

/// Whether an instance-method chain roots at the `Gate` facade, as
/// `Gate::forUser($user)->allows('update', $post)` does.
pub(in crate::symbol_map::extraction) fn chain_roots_at_gate(expr: &Expression<'_>) -> bool {
    chain_roots_at_facade(
        expr,
        crate::symbol_map::laravel_resources::FACADE_CHAIN_DEPTH,
        &|name| is_gate_facade(name),
    )
}

/// Whether a receiver plainly reads as the authenticated user, which is what
/// makes `->can('update', $post)` an authorization check rather than a
/// same-named method on an unrelated object.
///
/// Matches a variable whose name ends in `user` (`$user`, `$authUser`), a
/// property of that shape (`$this->user`), and any `user()` call
/// (`auth()->user()`, `Auth::user()`, `$request->user()`).  Anything else is
/// left alone: `can()` is too ordinary a method name to claim on sight.
pub(in crate::symbol_map::extraction) fn receiver_is_user_like(expr: &Expression<'_>) -> bool {
    fn ends_with_user(name: &str) -> bool {
        let name = name.trim_start_matches('$');
        let bytes = name.as_bytes();
        bytes.len() >= 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b"user")
    }

    match expr {
        Expression::Variable(Variable::Direct(dv)) => ends_with_user(bytes_to_str(dv.name)),
        Expression::Access(Access::Property(access)) => match &access.property {
            ClassLikeMemberSelector::Identifier(ident) => ends_with_user(bytes_to_str(ident.value)),
            _ => false,
        },
        Expression::Call(Call::Method(mc)) => match &mc.method {
            ClassLikeMemberSelector::Identifier(ident) => {
                bytes_to_str(ident.value).eq_ignore_ascii_case("user")
            }
            _ => false,
        },
        Expression::Call(Call::StaticMethod(sc)) => match &sc.method {
            ClassLikeMemberSelector::Identifier(ident) => {
                bytes_to_str(ident.value).eq_ignore_ascii_case("user")
            }
            _ => false,
        },
        _ => false,
    }
}

/// Emit a [`crate::symbol_map::StringKeyKind::GateAbility`] span for the
/// argument at `ability_index`, which Laravel accepts as either a single
/// string or an array of strings, and record the model named at
/// `model_index` (when there is one) as the checked subject.
pub(in crate::symbol_map::extraction) fn try_emit_gate_ability_spans(
    argument_list: &ArgumentList<'_>,
    ability_index: usize,
    model_index: Option<usize>,
    role: AbilityRole,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
    gate_subjects: &mut Vec<crate::symbol_map::GateSubject>,
) {
    let Some(ability_arg) = argument_list.arguments.iter().nth(ability_index) else {
        return;
    };

    let model = model_index.and_then(|i| argument_list.arguments.get(i));
    for_each_gate_ability(
        ability_arg.value(),
        model.map(Argument::value),
        content,
        gate_subjects,
        |key, start, end| {
            spans.push(SymbolSpan {
                start,
                end,
                kind: SymbolKind::StringKey {
                    kind: crate::symbol_map::StringKeyKind::GateAbility,
                    key: key.to_owned(),
                    is_write: role == AbilityRole::Definition,
                    is_optional: role == AbilityRole::Existence,
                },
            });
        },
    );
}

/// Keep the checked model at each ability's offset, including sites whose
/// receiver type will only be available after indexing.
pub(super) fn for_each_gate_ability(
    abilities: &Expression<'_>,
    model: Option<&Expression<'_>>,
    content: &str,
    subjects: &mut Vec<crate::symbol_map::GateSubject>,
    mut emit: impl FnMut(&str, u32, u32),
) {
    let subject =
        std::cell::LazyCell::new(|| model.and_then(|expr| gate_model_subject(expr, content)));
    for_each_value(abilities, |expr| {
        if let Some((key, start, end)) = string_literal_at_range(expr, content) {
            emit(key, start, end);
            if let Some((text, is_static)) = &*subject {
                subjects.push(crate::symbol_map::GateSubject {
                    ability_start: start,
                    subject_text: text.clone(),
                    is_static: *is_static,
                });
            }
        }
    });
}

/// The subject text of a gate check's model argument: `Post` for
/// `Post::class` (a class name), `$post` for a variable, `Post` for
/// `new Post`.  Anything else names no model we can resolve.
fn gate_model_subject(
    expr: &Expression<'_>,
    content: &str,
) -> Option<(crate::symbol_map::SubjectText, bool)> {
    let span = expr.span();
    match expr {
        Expression::Access(Access::ClassConstant(cca)) if crate::parser::is_class_fetch(cca) => {
            let name = expr_to_subject_text(cca.class);
            let class_span = cca.class.span();
            (!name.is_empty()).then(|| {
                (
                    SubjectText::new(
                        name,
                        class_span.start.offset,
                        class_span.end.offset,
                        content,
                    ),
                    true,
                )
            })
        }
        Expression::Variable(Variable::Direct(dv)) => Some((
            SubjectText::new(
                bytes_to_str(dv.name).to_string(),
                span.start.offset,
                span.end.offset,
                content,
            ),
            false,
        )),
        Expression::Instantiation(inst) => {
            let name = expr_to_subject_text(inst.class);
            (!name.is_empty()).then(|| (SubjectText::owned(name), true))
        }
        _ => None,
    }
}

/// Route calls and attributes identify middleware directly. Controller
/// calls need their receiver confirmed before the same names become spans.
pub(in crate::symbol_map::extraction) enum MiddlewareSpans<'a> {
    Known(&'a mut Vec<SymbolSpan>),
    Controller(&'a mut Vec<TypedReceiverSite>),
}

impl MiddlewareSpans<'_> {
    fn push(&mut self, part: MiddlewarePart, key: &str, start: u32, end: u32) {
        if key.is_empty() {
            return;
        }
        match self {
            Self::Known(spans) => spans.push(SymbolSpan {
                start,
                end,
                kind: SymbolKind::StringKey {
                    kind: part.controller_target().1,
                    key: key.to_owned(),
                    is_write: false,
                    is_optional: false,
                },
            }),
            Self::Controller(sites) => sites.push(TypedReceiverSite {
                start,
                end,
                key: key.to_owned(),
                receiver: ReceiverClass::LaravelControllerMiddleware(part),
                is_write: false,
                is_optional: false,
                guards: 0,
            }),
        }
    }
}

/// Emit the navigable parts of the Laravel middleware strings a call's
/// `middleware` argument names.
pub(in crate::symbol_map::extraction) fn try_emit_middleware_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: MiddlewareSpans<'_>,
) {
    if let Some(middleware) = argument_expr_for_parameter(argument_list, "middleware") {
        emit_middleware_value_spans(middleware, content, spans);
    }
}

/// Emit the navigable parts of a middleware string, or of each string in
/// an array of them: the alias or group name, the ability of
/// `can:ability,Model`, and the guards of `auth:guard[,guard]`.
pub(in crate::symbol_map::extraction) fn emit_middleware_value_spans(
    middleware: &Expression<'_>,
    content: &str,
    mut spans: MiddlewareSpans<'_>,
) {
    // Laravel reads a middleware array's values whatever their keys.
    let Some(elements) = crate::parser::array_literal_elements(middleware) else {
        push_middleware_parameter_spans(middleware, content, &mut spans);
        return;
    };
    for element in elements.iter() {
        let value = match element {
            ArrayElement::KeyValue(element) => element.value,
            ArrayElement::Value(element) => element.value,
            ArrayElement::Variadic(_) | ArrayElement::Missing(_) => continue,
        };
        push_middleware_parameter_spans(value, content, &mut spans);
    }
}

fn push_middleware_parameter_spans(
    expr: &Expression<'_>,
    content: &str,
    spans: &mut MiddlewareSpans<'_>,
) {
    let Some((text, inner_start, _)) = string_literal_at_range(expr, content) else {
        return;
    };
    let alias = text.split_once(':').map_or(text, |(alias, _)| alias);
    // A name with a namespace separator is a middleware class, which the
    // router instantiates without looking it up.
    if !alias.contains('\\') {
        spans.push(
            MiddlewarePart::Alias,
            alias,
            inner_start,
            inner_start + alias.len() as u32,
        );
    }
    let Some((alias, parameters)) = text.split_once(':') else {
        return;
    };
    let parameter_start = inner_start + alias.len() as u32 + 1;
    if alias == "can" {
        let ability = parameters.split(',').next().unwrap_or(parameters);
        spans.push(
            MiddlewarePart::Ability,
            ability,
            parameter_start,
            parameter_start + ability.len() as u32,
        );
    } else if alias == "throttle" {
        if let Some((decoded, _, end)) =
            crate::virtual_members::laravel::helpers::resolved_string_literal(expr, content)
            && let Some(name) = decoded.strip_prefix("throttle:")
            && !name.is_empty()
            && crate::virtual_members::laravel::rate_limiters::is_named_throttle_parameter(name)
        {
            spans.push(MiddlewarePart::Limiter, name, parameter_start, end as u32);
        }
    } else if crate::symbol_map::laravel_resources::middleware_resource(&text[..alias.len() + 1])
        == Some(crate::symbol_map::LaravelConfigResource::AuthGuard)
    {
        let mut offset = parameter_start;
        for guard in parameters.split(',') {
            spans.push(
                MiddlewarePart::Guard,
                guard,
                offset,
                offset + guard.len() as u32,
            );
            offset += guard.len() as u32 + 1;
        }
    }
}
